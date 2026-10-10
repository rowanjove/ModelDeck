use std::collections::BTreeMap;
use std::time::Instant;

use async_trait::async_trait;
use serde_json::Value;

use super::exchange::{self, elapsed_ms};
use super::media_job;
use super::replicate::materialize;
use super::ProviderAdapter;
use crate::canonical::{
    AdapterContext, AudioPayload, CanonicalRequest, CanonicalResponse, ConnectionReport,
    ImagePayload, RemoteModel, VideoPayload,
};
use crate::domain::ModelDefinition;
use crate::error::AppError;
use crate::http_util;
use crate::redact::shrink_json;

pub struct FalAdapter;

pub const FAL: FalAdapter = FalAdapter;

#[async_trait]
impl ProviderAdapter for FalAdapter {
    async fn test_connection(&self, ctx: &AdapterContext) -> Result<ConnectionReport, AppError> {
        let started = Instant::now();
        let _ = exchange::require_key(ctx)?;
        let url = ctx.provider.base_url.clone();
        let headers = headers_of(ctx);
        let (_raw, response) = exchange::send(ctx, None, "GET", &url, &headers, None).await?;
        let status = response.status().as_u16();
        if status == 401 || status == 403 {
            let text = String::from_utf8_lossy(
                &http_util::read_body(response, &ctx.provider.name, None).await?,
            )
            .into_owned();
            return Err(http_util::error_from_status(
                status,
                &text,
                &ctx.provider.name,
                None,
            ));
        }
        Ok(ConnectionReport {
            latency_ms: elapsed_ms(started),
            message: "已连接".to_string(),
        })
    }

    async fn list_models(&self, ctx: &AdapterContext) -> Result<Vec<RemoteModel>, AppError> {
        let _ = exchange::require_key(ctx)?;
        let Some(path) = ctx.models_path.as_deref() else {
            return Err(AppError::unsupported(
                "fal.ai 没有统一模型列表，请在模型里填写路径，例如 fal-ai/flux/schnell",
            ));
        };
        let url = if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else {
            http_util::join_url(&ctx.provider.base_url, path)?
        };
        let headers = headers_of(ctx);
        let (_raw, response) = exchange::send(ctx, None, "GET", &url, &headers, None).await?;
        let (_status, body) = http_util::read_response(response, &ctx.provider.name, None).await?;
        let items = body
            .get("models")
            .or_else(|| body.get("data"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(items
            .iter()
            .filter_map(|item| {
                let id = item.get("id").and_then(Value::as_str)?;
                let name = item.get("name").and_then(Value::as_str).unwrap_or(id);
                Some(RemoteModel {
                    id: id.to_string(),
                    name: name.to_string(),
                })
            })
            .collect())
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
            CanonicalRequest::Image(payload) => image_input(payload)?,
            CanonicalRequest::Video(payload) => video_input(payload)?,
            CanonicalRequest::Audio(payload) => audio_input(payload)?,
        };
        let endpoint = http_util::join_url(
            &ctx.provider.base_url,
            model.model_id.trim_start_matches('/'),
        )?;
        let headers = headers_of(ctx);
        let body = Value::Object(input);
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
        let finished = finish_queue(ctx, model, &headers, created).await?;
        let output = finished
            .get("response")
            .or_else(|| finished.get("output"))
            .or_else(|| finished.get("payload"))
            .cloned()
            .unwrap_or_else(|| finished.clone());
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
            format!("Key {}", secret.expose()),
        );
    }
    headers
}

async fn finish_queue(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    headers: &BTreeMap<String, String>,
    created: Value,
) -> Result<Value, AppError> {
    if created.get("images").is_some()
        || created.get("video").is_some()
        || created.get("audio").is_some()
        || created.get("text").is_some()
        || created.get("output").is_some()
    {
        return Ok(created);
    }
    let status_url = created
        .get("status_url")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::provider(
                "fal.ai 没有返回任务地址",
                200,
                &ctx.provider.name,
                Some(&model.name),
                Some(shrink_json(&created)),
            )
        })?;
    let status = media_job::poll_json(
        ctx,
        model,
        status_url,
        headers,
        &["completed"],
        &["failed", "error"],
    )
    .await?;
    let response_url = created
        .get("response_url")
        .and_then(Value::as_str)
        .or_else(|| status.get("response_url").and_then(Value::as_str))
        .ok_or_else(|| {
            AppError::provider(
                "fal.ai 没有返回结果地址",
                200,
                &ctx.provider.name,
                Some(&model.name),
                Some(shrink_json(&status)),
            )
        })?;
    let (_raw, response) =
        exchange::send(ctx, Some(&model.name), "GET", response_url, headers, None).await?;
    let (_status, body) =
        http_util::read_response(response, &ctx.provider.name, Some(&model.name)).await?;
    Ok(body)
}

fn image_input(payload: &ImagePayload) -> Result<serde_json::Map<String, Value>, AppError> {
    let mut input = media_job::seed_input(&payload.parameters, payload.extra.as_ref())?;
    media_job::put_text(&mut input, "prompt", &payload.prompt);
    if let Some(negative) = &payload.negative_prompt {
        media_job::put_text(&mut input, "negative_prompt", negative);
    }
    if let Some(source) = &payload.source_image {
        let (mime, bytes) = media_job::read_local(&source.path, "原图")?;
        media_job::put_text(&mut input, "image_url", media_job::data_url(&mime, &bytes));
    }
    if let Some(mask) = &payload.mask {
        let (mime, bytes) = media_job::read_local(&mask.path, "蒙版")?;
        media_job::put_text(&mut input, "mask_url", media_job::data_url(&mime, &bytes));
    }
    Ok(input)
}

fn video_input(payload: &VideoPayload) -> Result<serde_json::Map<String, Value>, AppError> {
    let mut input = media_job::seed_input(&payload.parameters, payload.extra.as_ref())?;
    media_job::video_prompt(payload, &mut input);
    if let Some(frame) = &payload.first_frame {
        let (mime, bytes) = media_job::read_local(&frame.path, "首帧")?;
        media_job::put_text(&mut input, "image_url", media_job::data_url(&mime, &bytes));
    }
    if let Some(frame) = &payload.last_frame {
        let (mime, bytes) = media_job::read_local(&frame.path, "尾帧")?;
        media_job::put_text(
            &mut input,
            "tail_image_url",
            media_job::data_url(&mime, &bytes),
        );
    }
    Ok(input)
}

fn audio_input(payload: &AudioPayload) -> Result<serde_json::Map<String, Value>, AppError> {
    let mut input = media_job::seed_input(&payload.parameters, payload.extra.as_ref())?;
    media_job::audio_fields(payload, &mut input);
    if let Some(source) = &payload.source_audio {
        let (mime, bytes) = media_job::read_local(&source.path, "源音频")?;
        media_job::put_text(&mut input, "audio_url", media_job::data_url(&mime, &bytes));
    }
    if let Some(reference) = &payload.reference_audio {
        let (mime, bytes) = media_job::read_local(&reference.path, "参考音频")?;
        media_job::put_text(
            &mut input,
            "reference_audio_url",
            media_job::data_url(&mime, &bytes),
        );
    }
    Ok(input)
}
