use std::collections::BTreeMap;
use std::time::Instant;

use async_trait::async_trait;
use serde_json::{json, Map, Value};

use super::exchange::{self, elapsed_ms};
use super::media_job::{self, audio_binary, image_binary, video_binary};
use super::ProviderAdapter;
use crate::canonical::{
    AdapterContext, AudioPayload, AudioResult, CanonicalRequest, CanonicalResponse,
    ConnectionReport, ImagePayload, ImageResult, RemoteModel, TextResult, VideoPayload,
    VideoResult,
};
use crate::domain::ModelDefinition;
use crate::error::AppError;
use crate::http_util;
use crate::redact::shrink_json;

pub struct ReplicateAdapter;

pub const REPLICATE: ReplicateAdapter = ReplicateAdapter;

#[async_trait]
impl ProviderAdapter for ReplicateAdapter {
    async fn test_connection(&self, ctx: &AdapterContext) -> Result<ConnectionReport, AppError> {
        let started = Instant::now();
        let _ = self.list_models(ctx).await?;
        Ok(ConnectionReport {
            latency_ms: elapsed_ms(started),
            message: "已连接".to_string(),
        })
    }

    async fn list_models(&self, ctx: &AdapterContext) -> Result<Vec<RemoteModel>, AppError> {
        let _ = exchange::require_key(ctx)?;
        let path = ctx.models_path.as_deref().unwrap_or("v1/models");
        let url = http_util::join_url(&ctx.provider.base_url, path)?;
        let headers = headers_of(ctx);
        let (_raw, response) = exchange::send(ctx, None, "GET", &url, &headers, None).await?;
        let (_status, body) = http_util::read_response(response, &ctx.provider.name, None).await?;
        let items = body
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut models = Vec::new();
        for item in items {
            let owner = item.get("owner").and_then(Value::as_str).unwrap_or("");
            let name = item.get("name").and_then(Value::as_str).unwrap_or("");
            if owner.is_empty() || name.is_empty() {
                continue;
            }
            models.push(RemoteModel {
                id: format!("{owner}/{name}"),
                name: name.to_string(),
            });
        }
        Ok(models)
    }

    async fn execute(
        &self,
        ctx: &AdapterContext,
        request: CanonicalRequest,
    ) -> Result<CanonicalResponse, AppError> {
        let model = ctx
            .model
            .as_ref()
            .ok_or_else(|| AppError::validation("缺少模型定义"))?;
        let started = Instant::now();
        let input = match &request {
            CanonicalRequest::Text(payload) => {
                let mut input = media_job::seed_input(&payload.parameters, payload.extra.as_ref())?;
                media_job::put_text(&mut input, "prompt", media_job::text_prompt(payload)?);
                input
            }
            CanonicalRequest::Image(payload) => image_input(ctx, model, payload).await?,
            CanonicalRequest::Video(payload) => video_input(ctx, model, payload).await?,
            CanonicalRequest::Audio(payload) => audio_input(ctx, model, payload).await?,
        };
        let (url, body) = prediction_request(&model.model_id, &input)?;
        let endpoint = http_util::join_url(&ctx.provider.base_url, &url)?;
        let headers = headers_of(ctx);
        let (raw, response) = exchange::send(
            ctx,
            Some(&model.name),
            "POST",
            &endpoint,
            &headers,
            Some(&body),
        )
        .await?;
        let (status, created) =
            http_util::read_response(response, &ctx.provider.name, Some(&model.name)).await?;
        let finished = finish_prediction(ctx, model, &headers, created).await?;
        let output = finished.get("output").cloned().unwrap_or(Value::Null);
        materialize(
            ctx, model, request, started, status, raw, &finished, &output,
        )
        .await
    }
}

fn headers_of(ctx: &AdapterContext) -> BTreeMap<String, String> {
    let mut headers = ctx.provider.headers.clone();
    if let Some(secret) = &ctx.api_key {
        headers.insert(
            "Authorization".to_string(),
            format!("Bearer {}", secret.expose()),
        );
    }
    headers
}

fn prediction_request(
    model_id: &str,
    input: &Map<String, Value>,
) -> Result<(String, Value), AppError> {
    let body = json!({ "input": input });
    if let Some((name, version)) = model_id.split_once(':') {
        if version.trim().is_empty() {
            return Err(AppError::validation("Replicate 模型版本为空"));
        }
        let _ = name;
        return Ok((
            "v1/predictions".to_string(),
            json!({ "version": version, "input": input }),
        ));
    }
    if !model_id.contains('/') {
        return Err(AppError::validation(
            "Replicate 模型需要 owner/name 或 owner/name:version",
        ));
    }
    Ok((format!("v1/models/{model_id}/predictions"), body))
}

async fn finish_prediction(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    headers: &BTreeMap<String, String>,
    created: Value,
) -> Result<Value, AppError> {
    let state = created
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if state == "succeeded" {
        return Ok(created);
    }
    if matches!(state.as_str(), "failed" | "canceled" | "aborted") {
        return Err(AppError::provider(
            "远程任务失败",
            200,
            &ctx.provider.name,
            Some(&model.name),
            Some(shrink_json(&created)),
        ));
    }
    let url = created
        .pointer("/urls/get")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::provider(
                "Replicate 没有返回任务地址",
                200,
                &ctx.provider.name,
                Some(&model.name),
                Some(shrink_json(&created)),
            )
        })?;
    media_job::poll_json(
        ctx,
        model,
        url,
        headers,
        &["succeeded"],
        &["failed", "canceled", "aborted"],
    )
    .await
}

async fn upload(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    path: &str,
    label: &str,
) -> Result<String, AppError> {
    let (mime, bytes) = media_job::read_local(path, label)?;
    let filename = std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("upload");
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename.to_string())
        .mime_str(&mime)
        .map_err(|_| AppError::validation("文件类型无效"))?;
    let form = reqwest::multipart::Form::new().part("content", part);
    let url = http_util::join_url(&ctx.provider.base_url, "v1/files")?;
    let mut builder = ctx.http.post(url);
    for (key, value) in headers_of(ctx) {
        builder = builder.header(key, value);
    }
    let response = http_util::send(
        builder.multipart(form),
        std::time::Duration::from_millis(ctx.provider.timeout_ms),
        &ctx.provider.name,
        Some(&model.name),
    )
    .await?;
    let (_status, body) =
        http_util::read_response(response, &ctx.provider.name, Some(&model.name)).await?;
    body.pointer("/urls/get")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            AppError::provider(
                "Replicate 没有返回上传地址",
                200,
                &ctx.provider.name,
                Some(&model.name),
                Some(shrink_json(&body)),
            )
        })
}

async fn image_input(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    payload: &ImagePayload,
) -> Result<Map<String, Value>, AppError> {
    let mut input = media_job::seed_input(&payload.parameters, payload.extra.as_ref())?;
    media_job::put_text(&mut input, "prompt", &payload.prompt);
    if let Some(negative) = &payload.negative_prompt {
        media_job::put_text(&mut input, "negative_prompt", negative);
    }
    if let Some(source) = &payload.source_image {
        let url = upload(ctx, model, &source.path, "原图").await?;
        media_job::put_text(&mut input, "image", url);
    }
    if let Some(mask) = &payload.mask {
        let url = upload(ctx, model, &mask.path, "蒙版").await?;
        media_job::put_text(&mut input, "mask", url);
    }
    Ok(input)
}

async fn video_input(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    payload: &VideoPayload,
) -> Result<Map<String, Value>, AppError> {
    let mut input = media_job::seed_input(&payload.parameters, payload.extra.as_ref())?;
    media_job::video_prompt(payload, &mut input);
    if let Some(frame) = &payload.first_frame {
        media_job::put_text(
            &mut input,
            "image",
            upload(ctx, model, &frame.path, "首帧").await?,
        );
    }
    if let Some(frame) = &payload.last_frame {
        media_job::put_text(
            &mut input,
            "last_image",
            upload(ctx, model, &frame.path, "尾帧").await?,
        );
    }
    Ok(input)
}

async fn audio_input(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    payload: &AudioPayload,
) -> Result<Map<String, Value>, AppError> {
    let mut input = media_job::seed_input(&payload.parameters, payload.extra.as_ref())?;
    media_job::audio_fields(payload, &mut input);
    if let Some(source) = &payload.source_audio {
        media_job::put_text(
            &mut input,
            "audio",
            upload(ctx, model, &source.path, "源音频").await?,
        );
    }
    if let Some(reference) = &payload.reference_audio {
        media_job::put_text(
            &mut input,
            "reference_audio",
            upload(ctx, model, &reference.path, "参考音频").await?,
        );
    }
    Ok(input)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn materialize(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    request: CanonicalRequest,
    started: Instant,
    status: u16,
    raw_request: Value,
    finished: &Value,
    output: &Value,
) -> Result<CanonicalResponse, AppError> {
    let strings = media_job::output_strings(output);
    let raw_response = shrink_json(finished);
    match request {
        CanonicalRequest::Text(_) => {
            let text = if strings.iter().all(|item| !media_job::is_url(item)) {
                strings.join("")
            } else {
                let bytes = media_job::load_reference(
                    ctx,
                    model,
                    strings.first().map(String::as_str).unwrap_or(""),
                    crate::http_util::MAX_RESPONSE_BYTES,
                )
                .await?;
                String::from_utf8_lossy(&bytes).trim().to_string()
            };
            if text.is_empty() {
                return Err(AppError::provider(
                    "响应里没有文本",
                    status,
                    &ctx.provider.name,
                    Some(&model.name),
                    Some(raw_response),
                ));
            }
            Ok(CanonicalResponse::Text(TextResult {
                text,
                usage: None,
                latency_ms: elapsed_ms(started),
                provider_id: ctx.provider.id.clone(),
                provider_name: ctx.provider.name.clone(),
                model_definition_id: model.id.clone(),
                model_name: model.name.clone(),
                model_id: model.model_id.clone(),
                raw_request,
                raw_response,
                status,
                stopped: false,
                reasoning: String::new(),
                interrupted: None,
                tool_calls: Vec::new(),
            }))
        }
        CanonicalRequest::Image(_) => {
            let binaries = load_kind(
                ctx,
                model,
                &strings,
                image_binary,
                "图像",
                crate::media::MAX_IMAGE_BYTES,
            )
            .await?;
            Ok(CanonicalResponse::Image(ImageResult {
                images: Vec::new(),
                latency_ms: elapsed_ms(started),
                provider_id: ctx.provider.id.clone(),
                provider_name: ctx.provider.name.clone(),
                model_definition_id: model.id.clone(),
                model_name: model.name.clone(),
                model_id: model.model_id.clone(),
                raw_request,
                raw_response,
                status,
                binaries,
            }))
        }
        CanonicalRequest::Video(_) => {
            let binaries = load_kind(
                ctx,
                model,
                &strings,
                video_binary,
                "视频",
                crate::media::MAX_VIDEO_BYTES,
            )
            .await?;
            Ok(CanonicalResponse::Video(VideoResult {
                videos: Vec::new(),
                latency_ms: elapsed_ms(started),
                provider_id: ctx.provider.id.clone(),
                provider_name: ctx.provider.name.clone(),
                model_definition_id: model.id.clone(),
                model_name: model.name.clone(),
                model_id: model.model_id.clone(),
                raw_request,
                raw_response,
                status,
                binaries,
            }))
        }
        CanonicalRequest::Audio(payload) => {
            if payload.task == crate::canonical::AudioTask::Stt {
                let transcript = transcript_of(ctx, model, &strings).await?;
                return Ok(CanonicalResponse::Audio(empty_audio(
                    ctx,
                    model,
                    started,
                    status,
                    raw_request,
                    raw_response,
                    transcript,
                )));
            }
            let binaries = load_kind(
                ctx,
                model,
                &strings,
                audio_binary,
                "音频",
                crate::media::MAX_AUDIO_BYTES,
            )
            .await?;
            let mut result = empty_audio(
                ctx,
                model,
                started,
                status,
                raw_request,
                raw_response,
                String::new(),
            );
            result.binaries = binaries;
            Ok(CanonicalResponse::Audio(result))
        }
    }
}

async fn transcript_of(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    strings: &[String],
) -> Result<String, AppError> {
    if strings.is_empty() {
        return Err(AppError::provider(
            "响应里没有文本",
            200,
            &ctx.provider.name,
            Some(&model.name),
            None,
        ));
    }
    if strings.iter().all(|item| !media_job::is_url(item)) {
        return Ok(strings.join("\n"));
    }
    let bytes = media_job::load_reference(
        ctx,
        model,
        &strings[0],
        crate::http_util::MAX_RESPONSE_BYTES,
    )
    .await?;
    Ok(String::from_utf8_lossy(&bytes).trim().to_string())
}

async fn load_kind<T, F>(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    strings: &[String],
    convert: F,
    label: &str,
    max_bytes: usize,
) -> Result<Vec<T>, AppError>
where
    F: Fn(Vec<u8>) -> Result<T, AppError>,
{
    if strings.is_empty() {
        return Err(AppError::provider(
            format!("响应里没有{label}"),
            200,
            &ctx.provider.name,
            Some(&model.name),
            None,
        ));
    }
    let mut items = Vec::new();
    for item in strings.iter().take(4) {
        let bytes = media_job::load_reference(ctx, model, item, max_bytes).await?;
        items.push(convert(bytes).map_err(|err| err.annotate(&ctx.provider.name, &model.name))?);
    }
    Ok(items)
}

fn empty_audio(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    started: Instant,
    status: u16,
    raw_request: Value,
    raw_response: Value,
    transcript: String,
) -> AudioResult {
    AudioResult {
        audios: Vec::new(),
        latency_ms: elapsed_ms(started),
        provider_id: ctx.provider.id.clone(),
        provider_name: ctx.provider.name.clone(),
        model_definition_id: model.id.clone(),
        model_name: model.name.clone(),
        model_id: model.model_id.clone(),
        raw_request,
        raw_response,
        status,
        transcript,
        transcript_path: None,
        binaries: Vec::new(),
    }
}
