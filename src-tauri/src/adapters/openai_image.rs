use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use base64::Engine;
use serde_json::{json, Map, Value};

use super::openai::{data_url_parts, raw_http, request_headers};
use crate::canonical::{AdapterContext, CanonicalResponse, ImageBinary, ImagePayload, ImageResult};
use crate::domain::ModelDefinition;
use crate::error::{AppError, PROVIDER};
use crate::http_util::{self, join_url};
use crate::media;
use crate::redact::{redact_header_map, redact_url, shrink_json};

const SKIPPED: &[&str] = &["model", "prompt", "width", "height", "batch_size"];
const MAX_IMAGE_RESPONSE_BYTES: usize = media::MAX_IMAGE_BYTES * 4 / 3 + 64 * 1024;

pub async fn execute(
    ctx: &AdapterContext,
    payload: ImagePayload,
) -> Result<CanonicalResponse, AppError> {
    let model = ctx
        .model
        .as_ref()
        .ok_or_else(|| AppError::validation("缺少模型定义"))?;
    let started = Instant::now();
    let editing = payload.source_image.is_some()
        || payload.mask.is_some()
        || !payload.reference_images.is_empty();
    let body = build_image_body(
        &model.model_id,
        &payload.prompt,
        &payload.parameters,
        payload.extra.as_ref(),
    )?;
    let (raw_request, response_body, status) = if editing {
        post_edits(ctx, model, &payload, &body).await?
    } else {
        post_generations(ctx, model, &body).await?
    };
    let binaries = read_images(ctx, model, &response_body).await?;
    if binaries.is_empty() {
        return Err(AppError::provider(
            "响应里没有图像",
            status,
            &ctx.provider.name,
            Some(&model.name),
            Some(shrink_json(&response_body)),
        ));
    }
    Ok(CanonicalResponse::Image(ImageResult {
        images: Vec::new(),
        latency_ms: elapsed_ms(started),
        provider_id: ctx.provider.id.clone(),
        provider_name: ctx.provider.name.clone(),
        model_definition_id: model.id.clone(),
        model_name: model.name.clone(),
        model_id: model.model_id.clone(),
        raw_request,
        raw_response: shrink_json(&response_body),
        status,
        binaries,
    }))
}

pub(crate) fn build_image_body(
    model_id: &str,
    prompt: &str,
    parameters: &Map<String, Value>,
    extra: Option<&Value>,
) -> Result<Map<String, Value>, AppError> {
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(model_id.to_string()));
    body.insert("prompt".to_string(), Value::String(prompt.to_string()));
    for (key, value) in parameters {
        if key == "model" || key == "prompt" {
            return Err(AppError::validation(format!("参数键 {key} 由协议保留")));
        }
        if SKIPPED.contains(&key.as_str()) {
            continue;
        }
        body.insert(key.clone(), value.clone());
    }
    if !body.contains_key("size") {
        if let (Some(width), Some(height)) = (
            parameters.get("width").and_then(whole_u32),
            parameters.get("height").and_then(whole_u32),
        ) {
            body.insert(
                "size".to_string(),
                Value::String(format!("{width}x{height}")),
            );
        }
    }
    if !body.contains_key("n") {
        if let Some(batch) = parameters.get("batch_size").and_then(whole_u32) {
            body.insert("n".to_string(), json!(batch));
        }
    }
    if let Some(extra) = extra {
        let extra = extra
            .as_object()
            .ok_or_else(|| AppError::validation("Extra JSON 必须是对象"))?;
        for (key, value) in extra {
            if key == "model" || key == "prompt" {
                return Err(AppError::validation("Extra JSON 不能覆盖 model 或 prompt"));
            }
            body.insert(key.clone(), value.clone());
        }
    }
    Ok(body)
}

async fn post_generations(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    body: &Map<String, Value>,
) -> Result<(Value, Value, u16), AppError> {
    let path = ctx
        .model
        .as_ref()
        .and_then(|item| item.transport.as_ref())
        .and_then(|item| item.image_path.clone())
        .unwrap_or_else(|| "images/generations".to_string());
    let url = join_url(&ctx.provider.base_url, &path)?;
    let headers = request_headers(ctx);
    let payload = Value::Object(body.clone());
    let mut builder = ctx.http.post(&url).json(&payload);
    for (key, value) in &headers {
        builder = builder.header(key, value);
    }
    let response = http_util::send(
        builder,
        std::time::Duration::from_millis(ctx.provider.timeout_ms),
        &ctx.provider.name,
        Some(&model.name),
    )
    .await?;
    let (status, response_body) = http_util::read_response_with_limit(
        response,
        MAX_IMAGE_RESPONSE_BYTES,
        &ctx.provider.name,
        Some(&model.name),
    )
    .await?;
    let mut raw = raw_http("POST", &url, &headers, &payload);
    if let Some(stored) = raw.get_mut("body") {
        *stored = shrink_json(stored);
    }
    Ok((raw, response_body, status))
}

async fn post_edits(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    payload: &ImagePayload,
    body: &Map<String, Value>,
) -> Result<(Value, Value, u16), AppError> {
    let path = ctx
        .model
        .as_ref()
        .and_then(|item| item.transport.as_ref())
        .and_then(|item| item.image_edit_path.clone())
        .unwrap_or_else(|| "images/edits".to_string());
    let url = join_url(&ctx.provider.base_url, &path)?;
    let headers = request_headers(ctx);
    let mut files = Vec::new();
    if let Some(source) = &payload.source_image {
        files.push(read_part(&source.path, "image")?);
    }
    for image in &payload.reference_images {
        files.push(read_part(&image.path, "image")?);
    }
    let mask = payload
        .mask
        .as_ref()
        .map(|item| read_part(&item.path, "mask"))
        .transpose()?;
    if let Some(part) = &mask {
        if part.mime != "image/png" {
            return Err(AppError::validation("蒙版必须是 PNG"));
        }
    }
    let mut form = reqwest::multipart::Form::new();
    let mut raw_body = Map::new();
    for (key, value) in body {
        let text = form_text(value)?;
        raw_body.insert(key.clone(), Value::String(text.clone()));
        form = form.text(key.clone(), text);
    }
    for part in &files {
        form = form.part("image", part_of(part)?);
    }
    raw_body.insert(
        "image".to_string(),
        Value::String(if files.len() == 1 {
            "[file]".to_string()
        } else {
            format!("[file x{}]", files.len())
        }),
    );
    if let Some(part) = &mask {
        form = form.part("mask", part_of(part)?);
        raw_body.insert("mask".to_string(), Value::String("[file]".to_string()));
    }
    let mut builder = ctx.http.post(&url);
    for (key, value) in &headers {
        builder = builder.header(key, value);
    }
    let response = http_util::send(
        builder.multipart(form),
        std::time::Duration::from_millis(ctx.provider.timeout_ms),
        &ctx.provider.name,
        Some(&model.name),
    )
    .await?;
    let (status, response_body) = http_util::read_response_with_limit(
        response,
        MAX_IMAGE_RESPONSE_BYTES,
        &ctx.provider.name,
        Some(&model.name),
    )
    .await?;
    Ok((
        raw_multipart(&url, &headers, &Value::Object(raw_body)),
        response_body,
        status,
    ))
}

struct FilePart {
    name: String,
    mime: &'static str,
    bytes: Vec<u8>,
}

fn read_part(path: &str, fallback: &str) -> Result<FilePart, AppError> {
    let (mime, bytes) = data_url_parts(path)?;
    let name = Path::new(path)
        .file_name()
        .and_then(|item| item.to_str())
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{fallback}.{ext}", ext = media::extension_for(mime)));
    Ok(FilePart { name, mime, bytes })
}

fn part_of(part: &FilePart) -> Result<reqwest::multipart::Part, AppError> {
    reqwest::multipart::Part::bytes(part.bytes.clone())
        .file_name(part.name.clone())
        .mime_str(part.mime)
        .map_err(|_| AppError::validation("图像类型无效"))
}

fn form_text(value: &Value) -> Result<String, AppError> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(number) => Ok(number.to_string()),
        Value::Bool(flag) => Ok(flag.to_string()),
        _ => Err(AppError::validation(
            "编辑请求的附加字段必须是字符串、数字或布尔",
        )),
    }
}

async fn read_images(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    body: &Value,
) -> Result<Vec<ImageBinary>, AppError> {
    let data = body.get("data").and_then(Value::as_array).ok_or_else(|| {
        AppError::provider(
            "响应里没有图像",
            200,
            &ctx.provider.name,
            Some(&model.name),
            Some(shrink_json(body)),
        )
    })?;
    let mut images = Vec::new();
    for item in data.iter().take(8) {
        if let Some(encoded) = item.get("b64_json").and_then(Value::as_str) {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded.trim())
                .map_err(|_| AppError::new(PROVIDER, "响应里的图像无法解码"))?;
            images.push(binary_from(bytes)?);
        } else if let Some(url) = item.get("url").and_then(Value::as_str) {
            let bytes = fetch_bytes(ctx, model, url).await?;
            images.push(binary_from(bytes)?);
        }
    }
    Ok(images)
}

async fn fetch_bytes(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    url: &str,
) -> Result<Vec<u8>, AppError> {
    let mut builder = ctx.http.get(url);
    if super::exchange::same_origin(&ctx.provider.base_url, url) {
        for (key, value) in request_headers(ctx) {
            builder = builder.header(key, value);
        }
    }
    let response = http_util::send(
        builder,
        std::time::Duration::from_millis(ctx.provider.timeout_ms),
        &ctx.provider.name,
        Some(&model.name),
    )
    .await?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let text = String::from_utf8_lossy(
            &http_util::read_body(response, &ctx.provider.name, Some(&model.name)).await?,
        )
        .into_owned();
        return Err(http_util::error_from_status(
            status,
            &text,
            &ctx.provider.name,
            Some(&model.name),
        ));
    }
    if let Some(content_length) = response.content_length() {
        if content_length > media::MAX_IMAGE_BYTES as u64 {
            return Err(AppError::validation(format!(
                "图像内容超过 {}MB 上限",
                media::MAX_IMAGE_BYTES / (1024 * 1024)
            )));
        }
    }
    tokio::select! {
        _ = super::exchange::wait_for_cancel(&ctx.cancelled) => {
            Err(AppError::validation("任务已取消"))
        }
        bytes = http_util::read_body_with_limit(
            response,
            media::MAX_IMAGE_BYTES,
            &ctx.provider.name,
            Some(&model.name),
        ) => bytes,
    }
}

fn binary_from(bytes: Vec<u8>) -> Result<ImageBinary, AppError> {
    let (mime_type, width, height) =
        media::describe(&bytes).map_err(|message| AppError::new(PROVIDER, message))?;
    Ok(ImageBinary {
        bytes,
        mime_type,
        width,
        height,
    })
}

fn whole_u32(value: &Value) -> Option<u32> {
    let number = value.as_f64()?;
    if !number.is_finite() || number < 0.0 || number.fract() != 0.0 || number > f64::from(u32::MAX)
    {
        return None;
    }
    Some(number as u32)
}

fn raw_multipart(url: &str, headers: &BTreeMap<String, String>, body: &Value) -> Value {
    let mut display = redact_header_map(headers);
    display.insert(
        "Content-Type".to_string(),
        "multipart/form-data".to_string(),
    );
    json!({
        "method": "POST",
        "url": redact_url(url),
        "headers": display,
        "body": shrink_json(body),
    })
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::exchange;
    use crate::error::VALIDATION;

    #[test]
    fn maps_size_and_batch_without_inventing_response_format() {
        let mut parameters = Map::new();
        parameters.insert("width".to_string(), json!(1024));
        parameters.insert("height".to_string(), json!(768));
        parameters.insert("batch_size".to_string(), json!(2));
        parameters.insert("quality".to_string(), json!("high"));
        let body = build_image_body("image-model", "a cat", &parameters, None).unwrap();
        assert_eq!(
            body.get("model").and_then(Value::as_str),
            Some("image-model")
        );
        assert_eq!(body.get("prompt").and_then(Value::as_str), Some("a cat"));
        assert_eq!(body.get("size").and_then(Value::as_str), Some("1024x768"));
        assert_eq!(body.get("n").and_then(Value::as_u64), Some(2));
        assert_eq!(body.get("quality").and_then(Value::as_str), Some("high"));
        assert!(body.get("width").is_none());
        assert!(body.get("response_format").is_none());

        parameters.insert("size".to_string(), json!("512x512"));
        parameters.insert("n".to_string(), json!(1));
        let kept = build_image_body("image-model", "a cat", &parameters, None).unwrap();
        assert_eq!(kept.get("size").and_then(Value::as_str), Some("512x512"));
        assert_eq!(kept.get("n").and_then(Value::as_u64), Some(1));

        let error = build_image_body(
            "image-model",
            "a cat",
            &Map::new(),
            Some(&json!({ "model": "other" })),
        )
        .unwrap_err();
        assert_eq!(error.code, VALIDATION);
    }

    #[test]
    fn only_same_origin_result_urls_receive_provider_credentials() {
        assert!(exchange::same_origin(
            "https://provider.example/api",
            "https://provider.example/generated.png"
        ));
        assert!(!exchange::same_origin(
            "https://provider.example/api",
            "http://provider.example/generated.png"
        ));
        assert!(!exchange::same_origin(
            "https://provider.example/api",
            "https://provider.example:8443/generated.png"
        ));
    }
}
