use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use reqwest::Client;
use serde_json::Value;
use sqlx::SqlitePool;

use crate::adapters::adapter_for;
use crate::canonical::{
    AdapterContext, CanonicalRequest, CanonicalResponse, TextPayload, TextResult, TextTurn,
};
use crate::credentials::SecretStore;
use crate::db;
use crate::domain::{capability_enabled, GenerateTextInput, MediaRef, Modality};
use crate::error::AppError;
use crate::schema::{is_empty_value, normalize_parameters};
use crate::services::history::{self, NewTextTask};
use crate::services::providers::load_secret;

struct Runtime<'a> {
    pool: &'a SqlitePool,
    http: &'a Client,
    secrets: &'a dyn SecretStore,
    data_dir: &'a Path,
}

pub async fn execute(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    data_dir: &Path,
    input: GenerateTextInput,
) -> Result<TextResult, AppError> {
    execute_with_cancel(pool, http, secrets, data_dir, input, None).await
}

pub async fn execute_with_cancel(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    data_dir: &Path,
    input: GenerateTextInput,
    cancel_flag: Option<Arc<AtomicBool>>,
) -> Result<TextResult, AppError> {
    let model = db::get_model(pool, &input.model_definition_id).await?;
    let provider = db::get_provider(pool, &model.provider_id).await?;
    let runtime = Runtime {
        pool,
        http,
        secrets,
        data_dir,
    };
    execute_with(&runtime, &provider, &model, input, cancel_flag)
        .await
        .map_err(|err| err.annotate(&provider.name, &model.name))
}

async fn execute_with(
    runtime: &Runtime<'_>,
    provider: &crate::domain::ProviderRecord,
    model: &crate::domain::ModelDefinition,
    input: GenerateTextInput,
    cancel_flag: Option<Arc<AtomicBool>>,
) -> Result<TextResult, AppError> {
    if !model.enabled {
        return Err(AppError::validation("模型已停用"));
    }
    if !provider.enabled {
        return Err(AppError::validation("Provider 已停用"));
    }
    if model.modality != Modality::Text {
        return Err(AppError::unsupported("当前入口只执行文本生成"));
    }
    if input.prompt.trim().is_empty() {
        return Err(AppError::validation("提示词不能为空"));
    }
    if input.prompt.chars().count() > 100_000 {
        return Err(AppError::validation("提示词过长"));
    }
    if input.images.len() > 8 {
        return Err(AppError::validation("一次最多附加 8 张图像"));
    }
    if let Some(extra) = &input.extra {
        if !extra.is_object() {
            return Err(AppError::validation("Extra JSON 必须是对象"));
        }
        let size = serde_json::to_string(extra)
            .map(|text| text.len())
            .unwrap_or(0);
        if size > 100_000 {
            return Err(AppError::validation("Extra JSON 过大"));
        }
    }
    let system_prompt = input
        .system_prompt
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned);
    let parameters =
        normalize_parameters(&model.parameters, &input.parameters, &model.constraints)?;
    for definition in &model.parameters {
        if definition.kind.is_media()
            && parameters
                .get(&definition.key)
                .is_some_and(|value| !is_empty_value(Some(value)))
        {
            return Err(AppError::unsupported(format!(
                "参数「{}」是媒体输入。文本生成请使用模型的图像输入能力，或清空该参数",
                definition.label
            )));
        }
    }
    if !input.images.is_empty() && !capability_enabled(&model.capabilities, "vision") {
        return Err(AppError::unsupported("该模型没有图像输入能力"));
    }
    let api_key = load_secret(runtime.secrets, provider.credential_id.as_deref())?;
    if provider.provider_type.requires_api_key() && api_key.is_none() {
        return Err(AppError::auth(
            "缺少 API Key",
            &provider.name,
            Some(&model.name),
        ));
    }
    let (chat_path, models_path) = match &model.transport {
        Some(transport) => (transport.chat_path.clone(), transport.models_path.clone()),
        None => (None, None),
    };
    let mut ctx = AdapterContext::new(
        runtime.http.clone(),
        provider.clone(),
        Some(model.clone()),
        api_key,
        chat_path,
        models_path,
    );
    if let Some(cf) = cancel_flag {
        ctx.cancelled = cf;
    }
    let draft = Draft {
        prompt: input.prompt.clone(),
        system_prompt: system_prompt.clone(),
        images: input.images.clone(),
        extra: input.extra.clone().filter(|value| !value.is_null()),
        parameters: parameters.clone(),
    };
    let request = CanonicalRequest::Text(TextPayload {
        system_prompt,
        turns: vec![TextTurn::plain("user", input.prompt, input.images)],
        parameters,
        extra: draft.extra.clone(),
        tools: Vec::new(),
    });
    let outcome = adapter_for(provider.provider_type)
        .execute(&ctx, request)
        .await;
    let record_res = remember(runtime, provider, model, draft, &outcome).await;
    let res = match outcome? {
        CanonicalResponse::Text(result) => result,
        CanonicalResponse::Image(_) => return Err(AppError::unsupported("文本入口收到了图像结果")),
        CanonicalResponse::Video(_) => return Err(AppError::unsupported("文本入口收到了视频结果")),
        CanonicalResponse::Audio(_) => return Err(AppError::unsupported("文本入口收到了音频结果")),
    };
    if let Err(record_err) = record_res {
        return Err(AppError::storage(format!(
            "文本生成成功，但持久化历史记录失败: {}。生成结果预览: {}",
            record_err.message,
            res.text.chars().take(100).collect::<String>()
        )));
    }
    Ok(res)
}

struct Draft {
    prompt: String,
    system_prompt: Option<String>,
    images: Vec<MediaRef>,
    extra: Option<Value>,
    parameters: serde_json::Map<String, Value>,
}

async fn remember(
    runtime: &Runtime<'_>,
    provider: &crate::domain::ProviderRecord,
    model: &crate::domain::ModelDefinition,
    draft: Draft,
    outcome: &Result<CanonicalResponse, AppError>,
) -> Result<String, AppError> {
    let (status, output_text, raw_request, raw_response, error) = match outcome {
        Ok(CanonicalResponse::Text(result)) => (
            "completed",
            (!result.text.is_empty()).then(|| result.text.clone()),
            Some(result.raw_request.clone()),
            Some(result.raw_response.clone()),
            None,
        ),
        Ok(CanonicalResponse::Image(_)) => (
            "failed",
            None,
            None,
            None,
            Some("文本入口收到了图像结果".to_string()),
        ),
        Ok(CanonicalResponse::Video(_)) => (
            "failed",
            None,
            None,
            None,
            Some("文本入口收到了视频结果".to_string()),
        ),
        Ok(CanonicalResponse::Audio(_)) => (
            "failed",
            None,
            None,
            None,
            Some("文本入口收到了音频结果".to_string()),
        ),
        Err(err) => (
            "failed",
            None,
            None,
            err.raw.as_deref().cloned(),
            Some(err.message.clone()),
        ),
    };
    history::record_text_task(
        runtime.pool,
        runtime.data_dir,
        NewTextTask {
            kind: "generate".to_string(),
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            model_id: model.id.clone(),
            model_name: model.name.clone(),
            model_api_id: model.model_id.clone(),
            prompt: draft.prompt,
            system_prompt: draft.system_prompt,
            images: draft.images,
            extra: draft.extra,
            conversation_id: None,
            parameters: draft.parameters,
            status: status.to_string(),
            output_text,
            raw_request,
            raw_response,
            error,
        },
    )
    .await
}
