use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{Map, Value};

use super::exchange;
use crate::canonical::{
    AdapterContext, AudioBinary, AudioPayload, AudioTask, ImageBinary, TextPayload, VideoBinary,
    VideoPayload,
};
use crate::domain::ModelDefinition;
use crate::error::AppError;
use crate::http_util;
use crate::media;

pub fn seed_input(
    parameters: &Map<String, Value>,
    extra: Option<&Value>,
) -> Result<Map<String, Value>, AppError> {
    let mut input = parameters.clone();
    if let Some(extra) = extra {
        let extra = extra
            .as_object()
            .ok_or_else(|| AppError::validation("Extra JSON 必须是对象"))?;
        for (key, value) in extra {
            if input.contains_key(key) {
                return Err(AppError::validation("Extra JSON 不能覆盖模板字段"));
            }
            input.insert(key.clone(), value.clone());
        }
    }
    Ok(input)
}

pub fn put_text(input: &mut Map<String, Value>, key: &str, value: impl AsRef<str>) {
    let value = value.as_ref().trim();
    if !value.is_empty() {
        input.insert(key.to_string(), Value::String(value.to_string()));
    }
}

pub fn text_prompt(payload: &TextPayload) -> Result<String, AppError> {
    if !payload.tools.is_empty() {
        return Err(AppError::unsupported("这个适配器不执行工具调用"));
    }
    let mut lines = Vec::new();
    if let Some(system) = payload
        .system_prompt
        .as_ref()
        .map(|text| text.trim())
        .filter(|text| !text.is_empty())
    {
        lines.push(format!("System: {system}"));
    }
    for turn in &payload.turns {
        if turn.text.trim().is_empty() {
            continue;
        }
        lines.push(format!("{}: {}", turn.role, turn.text.trim()));
    }
    let prompt = lines.join("\n");
    if prompt.is_empty() {
        return Err(AppError::validation("没有可发送的消息"));
    }
    Ok(prompt)
}

pub fn video_prompt(payload: &VideoPayload, input: &mut Map<String, Value>) {
    put_text(input, "prompt", &payload.prompt);
    if let Some(negative) = &payload.negative_prompt {
        put_text(input, "negative_prompt", negative);
    }
}

pub fn audio_fields(payload: &AudioPayload, input: &mut Map<String, Value>) {
    put_text(input, "prompt", &payload.text);
    put_text(input, "text", &payload.text);
    if let Some(voice) = &payload.voice {
        put_text(input, "voice", voice);
    }
    if let Some(transcript) = &payload.reference_transcript {
        put_text(input, "reference_transcript", transcript);
    }
    if payload.task == AudioTask::Stt {
        input.remove("prompt");
        if !payload.text.trim().is_empty() {
            put_text(input, "prompt", &payload.text);
        }
    }
}

pub fn output_strings(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() => vec![text.clone()],
        Value::Array(items) => items.iter().flat_map(output_strings).collect(),
        Value::Object(map) => {
            for key in [
                "output", "url", "image", "images", "video", "audio", "text", "result",
            ] {
                if let Some(found) = map.get(key) {
                    let nested = output_strings(found);
                    if !nested.is_empty() {
                        return nested;
                    }
                }
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

pub fn is_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://") || value.starts_with("data:")
}

pub async fn load_reference(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    reference: &str,
    max_bytes: usize,
) -> Result<Vec<u8>, AppError> {
    if let Some(rest) = reference.strip_prefix("data:") {
        let encoded = rest.split_once(',').map(|(_, data)| data).unwrap_or("");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| AppError::validation("结果数据无法解码"))?;
        if bytes.len() > max_bytes {
            return Err(AppError::validation(format!(
                "结果数据超过 {}MB 上限",
                max_bytes / (1024 * 1024)
            )));
        }
        return Ok(bytes);
    }
    exchange::download(ctx, &model.name, reference, max_bytes).await
}

pub async fn poll_json(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    url: &str,
    headers: &std::collections::BTreeMap<String, String>,
    done: &[&str],
    failed: &[&str],
) -> Result<Value, AppError> {
    let started = Instant::now();
    let budget = Duration::from_millis(ctx.provider.timeout_ms.clamp(5_000, 600_000));
    let mut interval = Duration::from_millis(700);
    loop {
        if ctx.cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(AppError::validation("任务已取消"));
        }
        if started.elapsed() > budget {
            return Err(AppError::timeout(&ctx.provider.name, Some(&model.name)));
        }
        let (_raw, response) =
            exchange::send(ctx, Some(&model.name), "GET", url, headers, None).await?;
        let (status, body) =
            http_util::read_response(response, &ctx.provider.name, Some(&model.name)).await?;
        let state = body
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        if done.iter().any(|item| *item == state) {
            return Ok(body);
        }
        if failed.iter().any(|item| *item == state) {
            let message = body
                .pointer("/error")
                .and_then(Value::as_str)
                .or_else(|| body.pointer("/error/message").and_then(Value::as_str))
                .unwrap_or("远程任务失败");
            return Err(AppError::provider(
                message,
                status,
                &ctx.provider.name,
                Some(&model.name),
                Some(crate::redact::shrink_json(&body)),
            ));
        }

        // 快速响应取消：分小片 sleep 并周期性检测 cancel 状态
        let slice = Duration::from_millis(100);
        let mut waited = Duration::ZERO;
        while waited < interval {
            if ctx.cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(AppError::validation("任务已取消"));
            }
            tokio::select! {
                _ = super::exchange::wait_for_cancel(&ctx.cancelled) => {
                    return Err(AppError::validation("任务已取消"));
                }
                _ = tokio::time::sleep(slice) => {}
            }
            waited += slice;
        }
        interval = (interval + Duration::from_millis(50)).min(Duration::from_millis(2000));
    }
}

pub fn image_binary(bytes: Vec<u8>) -> Result<ImageBinary, AppError> {
    let (mime, width, height) = media::describe(&bytes).map_err(AppError::validation)?;
    Ok(ImageBinary {
        bytes,
        mime_type: mime,
        width,
        height,
    })
}

pub fn video_binary(bytes: Vec<u8>) -> Result<VideoBinary, AppError> {
    let mime = media::describe_video(&bytes).map_err(AppError::validation)?;
    Ok(VideoBinary {
        bytes,
        mime_type: mime,
    })
}

pub fn audio_binary(bytes: Vec<u8>) -> Result<AudioBinary, AppError> {
    let mime = media::describe_audio(&bytes).map_err(AppError::validation)?;
    Ok(AudioBinary {
        bytes,
        mime_type: mime,
    })
}

pub fn read_local(path: &str, label: &str) -> Result<(String, Vec<u8>), AppError> {
    if let Some(rest) = path.strip_prefix("data:") {
        if let Some((header, encoded)) = rest.split_once(',') {
            let mime = header
                .split(';')
                .next()
                .unwrap_or("application/octet-stream")
                .to_string();
            let encoded = encoded.trim();
            const MAX_MEDIA_BYTES: usize = 80 * 1024 * 1024;
            if encoded.len() > MAX_MEDIA_BYTES.div_ceil(3) * 4 {
                return Err(AppError::validation(format!("{label}超过 80MB")));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| AppError::validation(format!("{label}的 data URL 无法解码")))?;
            if bytes.len() > MAX_MEDIA_BYTES {
                return Err(AppError::validation(format!("{label}超过 80MB")));
            }
            return Ok((mime, bytes));
        }
    }
    const MAX_MEDIA_BYTES: usize = 80 * 1024 * 1024;
    let bytes =
        crate::http_util::read_file_bounded_sync(std::path::Path::new(path), MAX_MEDIA_BYTES)
            .map_err(|err| match err {
                crate::http_util::BoundedFileReadError::TooLarge => {
                    AppError::validation(format!("{label}超过 80MB"))
                }
                crate::http_util::BoundedFileReadError::Io(err) => {
                    AppError::validation(format!("无法读取{label}: {err}"))
                }
            })?;
    let mime = media::sniff_mime(&bytes)
        .or_else(|| media::sniff_video(&bytes))
        .or_else(|| media::sniff_audio(&bytes))
        .ok_or_else(|| AppError::validation(format!("{label}格式不受支持")))?
        .to_string();
    Ok((mime, bytes))
}

pub fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}
