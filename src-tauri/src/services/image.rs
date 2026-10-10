use std::path::Path;

use base64::Engine;
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::adapters::adapter_for;
use crate::canonical::{
    AdapterContext, CanonicalRequest, CanonicalResponse, ImagePayload, ImageResult,
};
use crate::credentials::SecretStore;
use crate::db;
use crate::domain::{
    capability_enabled, GenerateImageInput, MediaRef, Modality, ProviderRecord, ProviderType,
    ReferenceImage,
};
use crate::error::AppError;
use crate::media;
use crate::schema::{is_empty_value, normalize_parameters, reject_exclusive};
use crate::services::history::{self, NewImageTask, StoredOutput};
use crate::services::providers::load_secret;

const ROLES: [&str; 7] = [
    "General",
    "Character",
    "Style",
    "Pose",
    "Composition",
    "Structure",
    "Control",
];

struct Runtime<'a> {
    pool: &'a SqlitePool,
    http: &'a Client,
    secrets: &'a dyn SecretStore,
    data_dir: &'a Path,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageFile {
    pub mime_type: String,
    pub base64: String,
}

#[allow(dead_code)]
pub async fn execute(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    data_dir: &Path,
    input: GenerateImageInput,
) -> Result<ImageResult, AppError> {
    execute_with_cancel(pool, http, secrets, data_dir, input, None, None).await
}

pub async fn execute_with_cancel(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    data_dir: &Path,
    input: GenerateImageInput,
    task_id: Option<String>,
    cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Result<ImageResult, AppError> {
    let model = db::get_model(pool, &input.model_definition_id).await?;
    let provider = db::get_provider(pool, &model.provider_id).await?;
    let runtime = Runtime {
        pool,
        http,
        secrets,
        data_dir,
    };
    execute_with(&runtime, &provider, &model, input, task_id, cancel_flag)
        .await
        .map_err(|err| err.annotate(&provider.name, &model.name))
}

pub async fn read_file(path: &str) -> Result<ImageFile, AppError> {
    let file = Path::new(path);
    if !file.is_file() {
        return Err(AppError::validation("找不到图像"));
    }
    let bytes = crate::http_util::read_file_bounded(file, media::MAX_IMAGE_BYTES)
        .await
        .map_err(|err| match err {
            crate::http_util::BoundedFileReadError::TooLarge => {
                AppError::validation("图像超过 15MB")
            }
            crate::http_util::BoundedFileReadError::Io(err) => {
                AppError::validation(format!("无法读取图像: {err}"))
            }
        })?;
    let mime = media::sniff_mime(&bytes).ok_or_else(|| AppError::validation("不是受支持的图像"))?;
    Ok(ImageFile {
        mime_type: mime.to_string(),
        base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

async fn execute_with(
    runtime: &Runtime<'_>,
    provider: &ProviderRecord,
    model: &crate::domain::ModelDefinition,
    input: GenerateImageInput,
    task_id: Option<String>,
    cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Result<ImageResult, AppError> {
    if !model.enabled {
        return Err(AppError::validation("模型已停用"));
    }
    if !provider.enabled {
        return Err(AppError::validation("Provider 已停用"));
    }
    if model.modality != Modality::Image {
        return Err(AppError::unsupported("当前入口只执行图像生成"));
    }
    let prompt = input.prompt.trim().to_string();
    if prompt.chars().count() > 20_000 {
        return Err(AppError::validation("提示词过长"));
    }
    if prompt.is_empty() && provider.provider_type != ProviderType::Comfyui {
        return Err(AppError::validation("提示词不能为空"));
    }
    let negative = input
        .negative_prompt
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned);
    if negative
        .as_ref()
        .is_some_and(|text| text.chars().count() > 20_000)
    {
        return Err(AppError::validation("反向提示词过长"));
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
    let parameters =
        normalize_parameters(&model.parameters, &input.parameters, &model.constraints)?;
    for definition in &model.parameters {
        if definition.kind.is_media()
            && parameters
                .get(&definition.key)
                .is_some_and(|value| !is_empty_value(Some(value)))
        {
            return Err(AppError::unsupported(format!(
                "参数「{}」是媒体输入。图像生成请使用原图、参考图或蒙版",
                definition.label
            )));
        }
    }
    let references = clean_references(&input.reference_images)?;
    let source = input
        .source_image
        .clone()
        .filter(|item| !item.path.trim().is_empty());
    let wants_mask = input
        .mask_base64
        .as_ref()
        .is_some_and(|text| !text.trim().is_empty())
        || input
            .mask_path
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty());
    check_capabilities(model, source.is_some(), !references.is_empty(), wants_mask)?;
    let mut present = Vec::new();
    if source.is_some() {
        present.push("sourceImage");
    }
    if wants_mask {
        present.push("mask");
    }
    if !references.is_empty() {
        present.push("referenceImages");
    }
    for key in parameters.keys() {
        present.push(key.as_str());
    }
    reject_exclusive(&model.constraints, &present)?;
    if !matches!(
        provider.provider_type,
        ProviderType::Openai
            | ProviderType::OpenaiCompatible
            | ProviderType::Comfyui
            | ProviderType::Replicate
            | ProviderType::Fal
    ) {
        return Err(AppError::unsupported(
            "图像生成目前由 OpenAI、OpenAI Compatible、ComfyUI、Replicate 与 fal.ai 适配器执行",
        ));
    }
    let api_key = load_secret(runtime.secrets, provider.credential_id.as_deref())?;
    if provider.provider_type.requires_api_key() && api_key.is_none() {
        return Err(AppError::auth(
            "缺少 API Key",
            &provider.name,
            Some(&model.name),
        ));
    }
    if provider.provider_type == ProviderType::Comfyui {
        require_workflow(model, &prompt)?;
    }
    if let Some(source) = &source {
        crate::adapters::data_url_parts(&source.path)?;
    }
    for image in &references {
        crate::adapters::data_url_parts(&image.path)?;
    }
    let task_id = task_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let mask = prepare_mask(runtime.data_dir, &task_id, &input).await?;
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
    if let Some(flag) = cancel_flag {
        ctx = ctx.with_cancelled(flag);
    }
    let draft = Draft {
        prompt: prompt.clone(),
        negative_prompt: negative.clone(),
        source_image: source.clone(),
        mask: mask.clone(),
        reference_images: references.clone(),
        extra: input.extra.clone().filter(|value| !value.is_null()),
        parameters: parameters.clone(),
    };
    let request = CanonicalRequest::Image(ImagePayload {
        prompt,
        negative_prompt: negative,
        source_image: source,
        reference_images: references,
        mask,
        parameters,
        extra: draft.extra.clone(),
    });

    history::record_image_task(
        runtime.pool,
        history::NewImageTask {
            id: task_id.clone(),
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            model_id: model.id.clone(),
            model_name: model.name.clone(),
            model_api_id: model.model_id.clone(),
            prompt: draft.prompt.clone(),
            negative_prompt: draft.negative_prompt.clone(),
            source_image: draft.source_image.clone(),
            mask: draft.mask.clone(),
            reference_images: draft.reference_images.clone(),
            extra: draft.extra.clone(),
            parameters: draft.parameters.clone(),
            status: "running".to_string(),
            outputs: Vec::new(),
            raw_request: None,
            raw_response: None,
            error: None,
        },
    )
    .await?;

    let outcome = adapter_for(provider.provider_type)
        .execute(&ctx, request)
        .await;
    let outcome = match outcome {
        Ok(CanonicalResponse::Image(mut result)) => {
            match persist(runtime.data_dir, &task_id, &mut result).await {
                Ok(()) => Ok(CanonicalResponse::Image(result)),
                Err(mut err) => {
                    err.raw = Some(Box::new(result.raw_response));
                    Err(err)
                }
            }
        }
        Ok(CanonicalResponse::Text(_)) => Err(AppError::unsupported("图像入口收到了文本结果")),
        Ok(CanonicalResponse::Video(_)) => Err(AppError::unsupported("图像入口收到了视频结果")),
        Ok(CanonicalResponse::Audio(_)) => Err(AppError::unsupported("图像入口收到了音频结果")),
        Err(err) => Err(err),
    };
    let remember_res = remember(runtime, provider, model, &task_id, draft, &outcome).await;
    let result = match outcome? {
        CanonicalResponse::Image(result) => result,
        CanonicalResponse::Text(_) => return Err(AppError::unsupported("图像入口收到了文本结果")),
        CanonicalResponse::Video(_) => return Err(AppError::unsupported("图像入口收到了视频结果")),
        CanonicalResponse::Audio(_) => return Err(AppError::unsupported("图像入口收到了音频结果")),
    };
    remember_res?;
    Ok(result)
}

fn check_capabilities(
    model: &crate::domain::ModelDefinition,
    has_source: bool,
    has_references: bool,
    has_mask: bool,
) -> Result<(), AppError> {
    if has_references && !capability_enabled(&model.capabilities, "referenceImages") {
        return Err(AppError::unsupported("该模型没有参考图能力"));
    }
    if has_mask {
        if !capability_enabled(&model.capabilities, "mask") {
            return Err(AppError::unsupported("该模型没有蒙版能力"));
        }
        if !has_source {
            return Err(AppError::validation("蒙版需要原图"));
        }
    }
    if has_source
        && !capability_enabled(&model.capabilities, "imageToImage")
        && !capability_enabled(&model.capabilities, "edit")
        && !capability_enabled(&model.capabilities, "mask")
    {
        return Err(AppError::unsupported("该模型不能接收原图"));
    }
    if !has_source
        && !has_references
        && !has_mask
        && !capability_enabled(&model.capabilities, "generate")
    {
        return Err(AppError::unsupported("该模型没有文生图能力"));
    }
    Ok(())
}

fn clean_references(images: &[ReferenceImage]) -> Result<Vec<ReferenceImage>, AppError> {
    if images.len() > 8 {
        return Err(AppError::validation("一次最多 8 张参考图"));
    }
    let mut cleaned = Vec::new();
    for image in images {
        if image.enabled == Some(false) {
            continue;
        }
        let path = image.path.trim();
        if path.is_empty() {
            continue;
        }
        let role = match image
            .role
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            None => None,
            Some(role) if ROLES.contains(&role) => Some(role.to_string()),
            Some(_) => return Err(AppError::validation("参考图角色无效")),
        };
        if image.weight.is_some_and(|value| !value.is_finite()) {
            return Err(AppError::validation("参考图权重无效"));
        }
        let description = image
            .description
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned);
        if description
            .as_ref()
            .is_some_and(|text| text.chars().count() > 2_000)
        {
            return Err(AppError::validation("参考图说明过长"));
        }
        cleaned.push(ReferenceImage {
            path: path.to_string(),
            role,
            weight: image.weight,
            description,
            enabled: Some(true),
        });
    }
    Ok(cleaned)
}

fn require_workflow(model: &crate::domain::ModelDefinition, prompt: &str) -> Result<(), AppError> {
    let workflow = model
        .transport
        .as_ref()
        .and_then(|item| item.workflow.as_ref());
    if !workflow.is_some_and(Value::is_object) {
        return Err(AppError::validation("模型没有 ComfyUI Workflow"));
    }
    if prompt.is_empty() {
        return Ok(());
    }
    let bound = model
        .transport
        .as_ref()
        .and_then(|item| item.bindings.as_ref())
        .is_some_and(|items| items.iter().any(|item| item.key == "prompt"));
    if bound {
        Ok(())
    } else {
        Err(AppError::validation("工作流没有绑定提示词"))
    }
}

async fn prepare_mask(
    data_dir: &Path,
    task_id: &str,
    input: &GenerateImageInput,
) -> Result<Option<MediaRef>, AppError> {
    if let Some(encoded) = input
        .mask_base64
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        crate::storage_util::validate_task_id(task_id)?;
        let bytes = decode_mask(encoded)?;
        let dir = data_dir.join("images");
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|err| AppError::storage(format!("无法创建数据目录: {err}")))?;
        let path = dir.join(format!("{task_id}-mask.png"));
        crate::storage_util::ensure_within_dir(data_dir, &path)?;
        crate::storage_util::atomic_write_async(&path, &bytes).await?;
        return Ok(Some(MediaRef::from_path(path.to_string_lossy())));
    }
    let Some(path) = input
        .mask_path
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    else {
        return Ok(None);
    };
    let bytes = crate::http_util::read_file_bounded_sync(Path::new(path), 8 * 1024 * 1024)
        .map_err(|err| match err {
            crate::http_util::BoundedFileReadError::TooLarge => {
                AppError::validation("蒙版超过 8MB")
            }
            crate::http_util::BoundedFileReadError::Io(_) => {
                AppError::validation(format!("找不到蒙版: {path}"))
            }
        })?;
    if media::sniff_mime(&bytes) != Some("image/png") {
        return Err(AppError::validation("蒙版必须是 PNG"));
    }
    Ok(Some(MediaRef::from_path(path)))
}

fn decode_mask(value: &str) -> Result<Vec<u8>, AppError> {
    decode_mask_with_limit(value, 8 * 1024 * 1024)
}

fn decode_mask_with_limit(value: &str, max_bytes: usize) -> Result<Vec<u8>, AppError> {
    let payload = value
        .split_once(',')
        .map(|(_, rest)| rest)
        .unwrap_or(value)
        .trim();
    if payload.len() > max_bytes.div_ceil(3) * 4 {
        return Err(mask_too_large_error(max_bytes));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|_| AppError::validation("蒙版不是有效的 PNG"))?;
    if bytes.len() > max_bytes {
        return Err(mask_too_large_error(max_bytes));
    }
    if media::sniff_mime(&bytes) != Some("image/png") {
        return Err(AppError::validation("蒙版必须是 PNG"));
    }
    Ok(bytes)
}

fn mask_too_large_error(max_bytes: usize) -> AppError {
    AppError::validation(format!("蒙版超过 {}MB", max_bytes.div_ceil(1024 * 1024)))
}

async fn persist(data_dir: &Path, task_id: &str, result: &mut ImageResult) -> Result<(), AppError> {
    crate::storage_util::validate_task_id(task_id)?;
    let dir = data_dir.join("images");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|err| AppError::storage(format!("无法创建数据目录: {err}")))?;
    let mut saved: Vec<crate::canonical::GeneratedImage> = Vec::new();
    let total_binaries = result.binaries.len();
    for (index, binary) in result.binaries.iter().enumerate() {
        let path = dir.join(format!(
            "{task_id}-{index}.{}",
            media::extension_for(&binary.mime_type)
        ));
        crate::storage_util::ensure_within_dir(data_dir, &path)?;
        if binary.bytes.is_empty() {
            for s in &saved {
                let _ = tokio::fs::remove_file(&s.path).await;
            }
            return Err(AppError::validation("生成的图像内容为空 (0 字节)"));
        }
        if media::sniff_mime(&binary.bytes).is_none() {
            for s in &saved {
                let _ = tokio::fs::remove_file(&s.path).await;
            }
            return Err(AppError::validation("生成的内容不是合法的图像格式"));
        }
        if let Err(err) = crate::storage_util::atomic_write_async(&path, &binary.bytes).await {
            for s in &saved {
                let _ = tokio::fs::remove_file(&s.path).await;
            }
            return Err(err);
        }
        saved.push(crate::canonical::GeneratedImage {
            path: path.to_string_lossy().to_string(),
            mime_type: binary.mime_type.clone(),
            width: binary.width,
            height: binary.height,
        });
    }
    result.binaries.clear();
    if saved.len() < total_binaries || saved.is_empty() {
        for s in &saved {
            let _ = tokio::fs::remove_file(&s.path).await;
        }
        return Err(AppError::storage("图像文件未能完整保存"));
    }
    result.images = saved;
    Ok(())
}

struct Draft {
    prompt: String,
    negative_prompt: Option<String>,
    source_image: Option<MediaRef>,
    mask: Option<MediaRef>,
    reference_images: Vec<ReferenceImage>,
    extra: Option<Value>,
    parameters: serde_json::Map<String, Value>,
}

async fn remember(
    runtime: &Runtime<'_>,
    provider: &ProviderRecord,
    model: &crate::domain::ModelDefinition,
    task_id: &str,
    draft: Draft,
    outcome: &Result<CanonicalResponse, AppError>,
) -> Result<(), AppError> {
    let (status, outputs, raw_request, raw_response, error) = match outcome {
        Ok(CanonicalResponse::Image(result)) => (
            "completed",
            result
                .images
                .iter()
                .map(|image| StoredOutput {
                    path: image.path.clone(),
                    mime_type: image.mime_type.clone(),
                    width: image.width,
                    height: image.height,
                })
                .collect(),
            Some(result.raw_request.clone()),
            Some(result.raw_response.clone()),
            None,
        ),
        Ok(CanonicalResponse::Text(_)) => (
            "failed",
            Vec::new(),
            None,
            None,
            Some("图像入口收到了文本结果".to_string()),
        ),
        Ok(CanonicalResponse::Video(_)) => (
            "failed",
            Vec::new(),
            None,
            None,
            Some("图像入口收到了视频结果".to_string()),
        ),
        Ok(CanonicalResponse::Audio(_)) => (
            "failed",
            Vec::new(),
            None,
            None,
            Some("图像入口收到了音频结果".to_string()),
        ),
        Err(err) => {
            let status = if err.message.contains("取消") || err.message.contains("停止") {
                "cancelled"
            } else {
                "failed"
            };
            (
                status,
                Vec::new(),
                None,
                err.raw.as_deref().cloned(),
                Some(err.message.clone()),
            )
        }
    };
    let record_result = history::record_image_task(
        runtime.pool,
        NewImageTask {
            id: task_id.to_string(),
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            model_id: model.id.clone(),
            model_name: model.name.clone(),
            model_api_id: model.model_id.clone(),
            prompt: draft.prompt,
            negative_prompt: draft.negative_prompt,
            source_image: draft.source_image,
            mask: draft.mask,
            reference_images: draft.reference_images,
            extra: draft.extra,
            parameters: draft.parameters,
            status: status.to_string(),
            outputs,
            raw_request,
            raw_response,
            error,
        },
    )
    .await
    .map(|_| ());
    if record_result.is_err() {
        let paths: Vec<String> = match outcome {
            Ok(CanonicalResponse::Image(result)) => result
                .images
                .iter()
                .map(|image| image.path.clone())
                .collect(),
            _ => Vec::new(),
        };
        if let Err(err) = history::remove_unreferenced_files(runtime.pool, &paths).await {
            crate::logging::global_logger().warn(
                "image",
                None,
                Some(task_id),
                Some(&provider.id),
                Some(&model.id),
                "orphan_output_cleanup_failed",
                &err.message,
            );
        }
    }
    record_result
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn mask_payload_size_is_checked_before_base64_decode() {
        let oversized = format!("data:image/png;base64,{}", "A".repeat(12));
        let error = decode_mask_with_limit(&oversized, 4).unwrap_err();
        assert!(error.message.contains("超过 1MB"));
    }

    #[test]
    fn valid_mask_payload_at_limit_is_accepted() {
        let png_header = b"\x89PNG\r\n\x1a\n";
        let encoded = base64::engine::general_purpose::STANDARD.encode(png_header);
        let decoded = decode_mask_with_limit(&encoded, png_header.len()).unwrap();
        assert_eq!(decoded, png_header);
    }

    #[tokio::test]
    async fn persist_rejects_empty_binary_and_cleans_up() {
        let temp_dir =
            std::env::temp_dir().join(format!("ai-wb-test-img-{}", uuid::Uuid::new_v4()));
        let task_id = uuid::Uuid::new_v4().to_string();
        let mut result = ImageResult {
            images: vec![],
            latency_ms: 0,
            provider_id: "p".into(),
            provider_name: "p".into(),
            model_definition_id: "m".into(),
            model_name: "m".into(),
            model_id: "m".into(),
            raw_request: serde_json::json!({}),
            raw_response: serde_json::json!({}),
            status: 200,
            binaries: vec![crate::canonical::ImageBinary {
                bytes: vec![],
                mime_type: "image/png".to_string(),
                width: None,
                height: None,
            }],
        };
        let err = persist(&temp_dir, &task_id, &mut result).await.unwrap_err();
        assert!(err.message.contains("为空 (0 字节)"));
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }

    #[tokio::test]
    async fn persist_rejects_invalid_media_and_cleans_up() {
        let temp_dir =
            std::env::temp_dir().join(format!("ai-wb-test-img-bad-{}", uuid::Uuid::new_v4()));
        let task_id = uuid::Uuid::new_v4().to_string();
        let mut result = ImageResult {
            images: vec![],
            latency_ms: 0,
            provider_id: "p".into(),
            provider_name: "p".into(),
            model_definition_id: "m".into(),
            model_name: "m".into(),
            model_id: "m".into(),
            raw_request: serde_json::json!({}),
            raw_response: serde_json::json!({}),
            status: 200,
            binaries: vec![crate::canonical::ImageBinary {
                bytes: vec![1, 2, 3, 4, 5],
                mime_type: "image/png".to_string(),
                width: None,
                height: None,
            }],
        };
        let err = persist(&temp_dir, &task_id, &mut result).await.unwrap_err();
        assert!(err.message.contains("不是合法的图像格式"));
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
