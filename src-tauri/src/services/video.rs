use std::path::Path;

use base64::Engine;
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::adapters::adapter_for;
use crate::canonical::{
    AdapterContext, CanonicalRequest, CanonicalResponse, VideoPayload, VideoResult,
};
use crate::credentials::SecretStore;
use crate::db;
use crate::domain::{
    capability_enabled, capability_limit, GenerateVideoInput, MediaRef, Modality, ProviderRecord,
    ProviderType, ReferenceImage,
};
use crate::error::AppError;
use crate::media;
use crate::schema::{is_empty_value, normalize_parameters, reject_exclusive};
use crate::services::history::{self, NewVideoTask, StoredOutput};
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
pub struct VideoFile {
    pub mime_type: String,
    pub base64: String,
}

#[allow(dead_code)]
pub async fn execute(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    data_dir: &Path,
    input: GenerateVideoInput,
) -> Result<VideoResult, AppError> {
    execute_with_cancel(pool, http, secrets, data_dir, input, None, None).await
}

pub async fn execute_with_cancel(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    data_dir: &Path,
    input: GenerateVideoInput,
    task_id: Option<String>,
    cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Result<VideoResult, AppError> {
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

pub async fn read_file(path: &str) -> Result<VideoFile, AppError> {
    let file = Path::new(path);
    if !file.is_file() {
        return Err(AppError::validation("找不到视频"));
    }
    let bytes = crate::http_util::read_file_bounded(file, media::MAX_VIDEO_PREVIEW_BYTES)
        .await
        .map_err(|err| match err {
            crate::http_util::BoundedFileReadError::TooLarge => {
                AppError::validation("视频超过 32MB，无法在页面内预览")
            }
            crate::http_util::BoundedFileReadError::Io(err) => {
                AppError::validation(format!("无法读取视频: {err}"))
            }
        })?;
    let mime =
        media::sniff_video(&bytes).ok_or_else(|| AppError::validation("不是受支持的视频"))?;
    Ok(VideoFile {
        mime_type: mime.to_string(),
        base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

async fn execute_with(
    runtime: &Runtime<'_>,
    provider: &ProviderRecord,
    model: &crate::domain::ModelDefinition,
    input: GenerateVideoInput,
    task_id: Option<String>,
    cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Result<VideoResult, AppError> {
    if !model.enabled {
        return Err(AppError::validation("模型已停用"));
    }
    if !provider.enabled {
        return Err(AppError::validation("Provider 已停用"));
    }
    if model.modality != Modality::Video {
        return Err(AppError::unsupported("当前入口只执行视频生成"));
    }
    let prompt = input.prompt.trim().to_string();
    if prompt.chars().count() > 20_000 {
        return Err(AppError::validation("提示词过长"));
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
                "参数「{}」是媒体输入。视频生成请使用首帧、尾帧、参考图、参考视频或参考音频",
                definition.label
            )));
        }
    }
    let references = clean_references(
        &input.reference_images,
        capability_limit(&model.capabilities, "referenceImages", 8).clamp(1, 8),
    )?;
    let videos = clean_media(
        &input.reference_videos,
        capability_limit(&model.capabilities, "referenceVideos", 4).clamp(1, 8),
        "参考视频",
    )?;
    let audios = clean_media(
        &input.reference_audios,
        capability_limit(&model.capabilities, "referenceAudio", 4).clamp(1, 8),
        "参考音频",
    )?;
    let first = input
        .first_frame
        .clone()
        .filter(|item| item.enabled != Some(false) && !item.path.trim().is_empty());
    let last = input
        .last_frame
        .clone()
        .filter(|item| item.enabled != Some(false) && !item.path.trim().is_empty());
    check_capabilities(
        model,
        first.is_some(),
        last.is_some(),
        !references.is_empty(),
        !videos.is_empty(),
        !audios.is_empty(),
    )?;
    let mut present = Vec::new();
    if first.is_some() {
        present.push("firstFrame");
    }
    if last.is_some() {
        present.push("lastFrame");
    }
    if !references.is_empty() {
        present.push("referenceImages");
    }
    if !videos.is_empty() {
        present.push("referenceVideos");
    }
    if !audios.is_empty() {
        present.push("referenceAudio");
    }
    for key in parameters.keys() {
        present.push(key.as_str());
    }
    reject_exclusive(&model.constraints, &present)?;
    if !matches!(
        provider.provider_type,
        ProviderType::Comfyui
            | ProviderType::GenericHttp
            | ProviderType::Replicate
            | ProviderType::Fal
    ) {
        return Err(AppError::unsupported(
            "视频生成目前由 ComfyUI、Generic HTTP、Replicate 与 fal.ai 适配器执行",
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
    if provider.provider_type == ProviderType::GenericHttp {
        let recipe = model
            .transport
            .as_ref()
            .and_then(|item| item.http.as_ref())
            .ok_or_else(|| AppError::validation("模型没有 HTTP 请求模板"))?;
        let text = serde_json::to_string(recipe).unwrap_or_default();
        if text.contains("{{apiKey}}") && api_key.is_none() {
            return Err(AppError::auth(
                "缺少 API Key",
                &provider.name,
                Some(&model.name),
            ));
        }
        if let (Some(extra), Some(body)) = (
            input.extra.as_ref().and_then(Value::as_object),
            recipe.body.as_ref().and_then(Value::as_object),
        ) {
            if extra.keys().any(|key| body.contains_key(key)) {
                return Err(AppError::validation("Extra JSON 不能覆盖模板字段"));
            }
        } else if input.extra.is_some() && recipe.body.is_none() {
            return Err(AppError::validation("Extra JSON 需要模板正文是对象"));
        }
    }
    if let Some(frame) = &first {
        crate::adapters::data_url_parts(&frame.path)?;
    }
    if let Some(frame) = &last {
        crate::adapters::data_url_parts(&frame.path)?;
    }
    for image in &references {
        crate::adapters::data_url_parts(&image.path)?;
    }
    ensure_media(&videos, true)?;
    ensure_media(&audios, false)?;
    let task_id = task_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
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
        first_frame: first.clone(),
        last_frame: last.clone(),
        reference_images: references.clone(),
        reference_videos: videos.clone(),
        reference_audios: audios.clone(),
        extra: input.extra.clone().filter(|value| !value.is_null()),
        parameters: parameters.clone(),
    };
    let request = CanonicalRequest::Video(VideoPayload {
        prompt,
        negative_prompt: negative,
        first_frame: first,
        last_frame: last,
        reference_images: references,
        reference_videos: videos,
        reference_audios: audios,
        parameters,
        extra: draft.extra.clone(),
    });

    history::record_video_task(
        runtime.pool,
        NewVideoTask {
            id: task_id.clone(),
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            model_id: model.id.clone(),
            model_name: model.name.clone(),
            model_api_id: model.model_id.clone(),
            prompt: draft.prompt.clone(),
            negative_prompt: draft.negative_prompt.clone(),
            first_frame: draft.first_frame.clone(),
            last_frame: draft.last_frame.clone(),
            reference_images: draft.reference_images.clone(),
            reference_videos: draft.reference_videos.clone(),
            reference_audios: draft.reference_audios.clone(),
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
        Ok(CanonicalResponse::Video(mut result)) => {
            match persist(runtime.data_dir, &task_id, &mut result).await {
                Ok(()) => Ok(CanonicalResponse::Video(result)),
                Err(mut err) => {
                    err.raw = Some(Box::new(result.raw_response));
                    Err(err)
                }
            }
        }
        Ok(CanonicalResponse::Text(_))
        | Ok(CanonicalResponse::Image(_))
        | Ok(CanonicalResponse::Audio(_)) => Err(AppError::unsupported("视频入口收到了其他结果")),
        Err(err) => Err(err),
    };
    let remember_res = remember(runtime, provider, model, &task_id, draft, &outcome).await;
    let result = match outcome? {
        CanonicalResponse::Video(result) => result,
        CanonicalResponse::Text(_) | CanonicalResponse::Image(_) | CanonicalResponse::Audio(_) => {
            return Err(AppError::unsupported("视频入口收到了其他结果"));
        }
    };
    remember_res?;
    Ok(result)
}

fn check_capabilities(
    model: &crate::domain::ModelDefinition,
    has_first: bool,
    has_last: bool,
    has_references: bool,
    has_videos: bool,
    has_audios: bool,
) -> Result<(), AppError> {
    if has_references && !capability_enabled(&model.capabilities, "referenceImages") {
        return Err(AppError::unsupported("该模型没有参考图能力"));
    }
    if has_videos && !capability_enabled(&model.capabilities, "referenceVideos") {
        return Err(AppError::unsupported("该模型没有参考视频能力"));
    }
    if has_audios && !capability_enabled(&model.capabilities, "referenceAudio") {
        return Err(AppError::unsupported("该模型没有参考音频能力"));
    }
    if has_last {
        if !capability_enabled(&model.capabilities, "lastFrame") {
            return Err(AppError::unsupported("该模型没有尾帧能力"));
        }
        if !has_first {
            return Err(AppError::validation("尾帧需要首帧"));
        }
    }
    if has_first
        && !capability_enabled(&model.capabilities, "firstFrame")
        && !capability_enabled(&model.capabilities, "imageToVideo")
    {
        return Err(AppError::unsupported("该模型不能接收首帧"));
    }
    if !has_first
        && !has_references
        && !has_videos
        && !has_audios
        && !capability_enabled(&model.capabilities, "textToVideo")
    {
        return Err(AppError::unsupported("该模型没有文生视频能力"));
    }
    Ok(())
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

fn clean_references(
    images: &[ReferenceImage],
    limit: usize,
) -> Result<Vec<ReferenceImage>, AppError> {
    if images.len() > limit {
        return Err(AppError::validation(format!("一次最多 {limit} 张参考图")));
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
        if let Some(role) = image
            .role
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            if !ROLES.contains(&role) {
                return Err(AppError::validation("参考图角色无效"));
            }
        }
        if image
            .weight
            .is_some_and(|weight| !weight.is_finite() || !(0.0..=10.0).contains(&weight))
        {
            return Err(AppError::validation("参考图权重需要在 0 到 10 之间"));
        }
        if image
            .description
            .as_ref()
            .is_some_and(|text| text.chars().count() > 2_000)
        {
            return Err(AppError::validation("参考图说明过长"));
        }
        cleaned.push(ReferenceImage {
            path: path.to_string(),
            role: image
                .role
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned),
            weight: image.weight,
            description: image
                .description
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned),
            enabled: Some(true),
        });
    }
    Ok(cleaned)
}

fn clean_media(items: &[MediaRef], limit: usize, label: &str) -> Result<Vec<MediaRef>, AppError> {
    if items.len() > limit {
        return Err(AppError::validation(format!("一次最多 {limit} 个{label}")));
    }
    let mut cleaned = Vec::new();
    for item in items {
        if item.enabled == Some(false) {
            continue;
        }
        let path = item.path.trim();
        if path.is_empty() {
            continue;
        }
        cleaned.push(MediaRef {
            path: path.to_string(),
            role: item.role.clone(),
            weight: item.weight,
            description: item.description.clone(),
            enabled: Some(true),
        });
    }
    Ok(cleaned)
}

fn ensure_media(items: &[MediaRef], video: bool) -> Result<(), AppError> {
    for item in items {
        let (limit, label, too_large) = if video {
            (
                media::MAX_VIDEO_PREVIEW_BYTES,
                "参考视频",
                "参考视频超过 32MB",
            )
        } else {
            (
                media::MAX_AUDIO_PREVIEW_BYTES,
                "参考音频",
                "参考音频超过 20MB",
            )
        };
        let bytes = crate::http_util::read_file_bounded_sync(Path::new(&item.path), limit)
            .map_err(|err| match err {
                crate::http_util::BoundedFileReadError::TooLarge => AppError::validation(too_large),
                crate::http_util::BoundedFileReadError::Io(err) => {
                    AppError::validation(format!("无法读取{label}: {err}"))
                }
            })?;
        if video {
            if media::sniff_video(&bytes).is_none() {
                return Err(AppError::validation("参考视频格式不受支持"));
            }
        } else if media::sniff_audio(&bytes).is_none() {
            return Err(AppError::validation("参考音频格式不受支持"));
        }
    }
    Ok(())
}

async fn persist(data_dir: &Path, task_id: &str, result: &mut VideoResult) -> Result<(), AppError> {
    crate::storage_util::validate_task_id(task_id)?;
    let dir = data_dir.join("videos");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|err| AppError::storage(format!("无法创建数据目录: {err}")))?;
    let mut saved: Vec<crate::canonical::GeneratedVideo> = Vec::new();
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
            return Err(AppError::validation("生成的视频内容为空 (0 字节)"));
        }
        if media::sniff_video(&binary.bytes).is_none() {
            for s in &saved {
                let _ = tokio::fs::remove_file(&s.path).await;
            }
            return Err(AppError::validation("生成的内容不是合法的视频格式"));
        }
        if let Err(err) = crate::storage_util::atomic_write_async(&path, &binary.bytes).await {
            for s in &saved {
                let _ = tokio::fs::remove_file(&s.path).await;
            }
            return Err(err);
        }
        saved.push(crate::canonical::GeneratedVideo {
            path: path.to_string_lossy().to_string(),
            mime_type: binary.mime_type.clone(),
        });
    }
    result.binaries.clear();
    if saved.len() < total_binaries || saved.is_empty() {
        for s in &saved {
            let _ = tokio::fs::remove_file(&s.path).await;
        }
        return Err(AppError::storage("视频文件未能完整保存"));
    }
    result.videos = saved;
    Ok(())
}

struct Draft {
    prompt: String,
    negative_prompt: Option<String>,
    first_frame: Option<MediaRef>,
    last_frame: Option<MediaRef>,
    reference_images: Vec<ReferenceImage>,
    reference_videos: Vec<MediaRef>,
    reference_audios: Vec<MediaRef>,
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
        Ok(CanonicalResponse::Video(result)) => (
            "completed",
            result
                .videos
                .iter()
                .map(|video| StoredOutput {
                    path: video.path.clone(),
                    mime_type: video.mime_type.clone(),
                    width: None,
                    height: None,
                })
                .collect(),
            Some(result.raw_request.clone()),
            Some(result.raw_response.clone()),
            None,
        ),
        Ok(CanonicalResponse::Text(_))
        | Ok(CanonicalResponse::Image(_))
        | Ok(CanonicalResponse::Audio(_)) => (
            "failed",
            Vec::new(),
            None,
            None,
            Some("视频入口收到了其他结果".to_string()),
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
    let record_result = history::record_video_task(
        runtime.pool,
        NewVideoTask {
            id: task_id.to_string(),
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            model_id: model.id.clone(),
            model_name: model.name.clone(),
            model_api_id: model.model_id.clone(),
            prompt: draft.prompt,
            negative_prompt: draft.negative_prompt,
            first_frame: draft.first_frame,
            last_frame: draft.last_frame,
            reference_images: draft.reference_images,
            reference_videos: draft.reference_videos,
            reference_audios: draft.reference_audios,
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
            Ok(CanonicalResponse::Video(result)) => result
                .videos
                .iter()
                .map(|video| video.path.clone())
                .collect(),
            _ => Vec::new(),
        };
        if let Err(err) = history::remove_unreferenced_files(runtime.pool, &paths).await {
            crate::logging::global_logger().warn(
                "video",
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

    #[tokio::test]
    async fn persist_rejects_empty_binary() {
        let temp_dir =
            std::env::temp_dir().join(format!("ai-wb-test-vid-{}", uuid::Uuid::new_v4()));
        let task_id = uuid::Uuid::new_v4().to_string();
        let mut result = VideoResult {
            videos: vec![],
            latency_ms: 0,
            provider_id: "p".into(),
            provider_name: "p".into(),
            model_definition_id: "m".into(),
            model_name: "m".into(),
            model_id: "m".into(),
            raw_request: serde_json::json!({}),
            raw_response: serde_json::json!({}),
            status: 200,
            binaries: vec![crate::canonical::VideoBinary {
                bytes: vec![],
                mime_type: "video/mp4".to_string(),
            }],
        };
        let err = persist(&temp_dir, &task_id, &mut result).await.unwrap_err();
        assert!(err.message.contains("为空 (0 字节)"));
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }

    #[tokio::test]
    async fn persist_rejects_invalid_media() {
        let temp_dir =
            std::env::temp_dir().join(format!("ai-wb-test-vid-bad-{}", uuid::Uuid::new_v4()));
        let task_id = uuid::Uuid::new_v4().to_string();
        let mut result = VideoResult {
            videos: vec![],
            latency_ms: 0,
            provider_id: "p".into(),
            provider_name: "p".into(),
            model_definition_id: "m".into(),
            model_name: "m".into(),
            model_id: "m".into(),
            raw_request: serde_json::json!({}),
            raw_response: serde_json::json!({}),
            status: 200,
            binaries: vec![crate::canonical::VideoBinary {
                bytes: vec![1, 2, 3, 4],
                mime_type: "video/mp4".to_string(),
            }],
        };
        let err = persist(&temp_dir, &task_id, &mut result).await.unwrap_err();
        assert!(err.message.contains("不是合法的视频格式"));
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
