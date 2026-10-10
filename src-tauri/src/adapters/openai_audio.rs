use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use super::openai::{raw_http, request_headers};
use crate::canonical::{
    AdapterContext, AudioBinary, AudioPayload, AudioResult, AudioTask, CanonicalResponse,
};
use crate::error::AppError;
use crate::http_util::{self, join_url};
use crate::media;
use crate::redact::shrink_json;

pub async fn execute(
    ctx: &AdapterContext,
    payload: AudioPayload,
) -> Result<CanonicalResponse, AppError> {
    match payload.task {
        AudioTask::Sts => Err(AppError::unsupported("这个语音接口不执行语音转语音")),
        AudioTask::Clone => Err(AppError::unsupported(
            "这个语音接口不接受参考音频，请改用 Generic HTTP",
        )),
        AudioTask::Stt => transcribe(ctx, payload).await,
        AudioTask::Tts => speak(ctx, payload).await,
    }
}

async fn speak(ctx: &AdapterContext, payload: AudioPayload) -> Result<CanonicalResponse, AppError> {
    let model = ctx
        .model
        .as_ref()
        .ok_or_else(|| AppError::validation("缺少模型定义"))?;
    let started = Instant::now();
    let body = build_speech_body(&model.model_id, &payload)?;
    let path = model
        .transport
        .as_ref()
        .and_then(|item| item.speech_path.clone())
        .unwrap_or_else(|| "audio/speech".to_string());
    let url = join_url(&ctx.provider.base_url, &path)?;
    let headers = request_headers(ctx);
    let payload_json = Value::Object(body);
    let raw_request = {
        let mut raw = raw_http("POST", &url, &headers, &payload_json);
        if let Some(stored) = raw.get_mut("body") {
            *stored = shrink_json(stored);
        }
        raw
    };
    let mut builder = ctx.http.post(&url).json(&payload_json);
    for (key, value) in &headers {
        builder = builder.header(key, value);
    }
    let response = http_util::send(
        builder,
        Duration::from_millis(ctx.provider.timeout_ms),
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
    let bytes = tokio::select! {
        _ = super::exchange::wait_for_cancel(&ctx.cancelled) => {
            return Err(AppError::validation("任务已取消"));
        }
        bytes = http_util::read_body_with_limit(
            response,
            crate::media::MAX_AUDIO_BYTES,
            &ctx.provider.name,
            Some(&model.name),
        ) => bytes?,
    };
    let mime_type = media::describe_audio(&bytes).map_err(|message| {
        AppError::provider(
            message,
            status,
            &ctx.provider.name,
            Some(&model.name),
            Some(json!({ "bytes": bytes.len() })),
        )
    })?;
    Ok(CanonicalResponse::Audio(AudioResult {
        audios: Vec::new(),
        latency_ms: elapsed_ms(started),
        provider_id: ctx.provider.id.clone(),
        provider_name: ctx.provider.name.clone(),
        model_definition_id: model.id.clone(),
        model_name: model.name.clone(),
        model_id: model.model_id.clone(),
        raw_request,
        raw_response: json!({ "bytes": bytes.len(), "mimeType": mime_type }),
        status,
        transcript: String::new(),
        transcript_path: None,
        binaries: vec![AudioBinary { bytes, mime_type }],
    }))
}

async fn transcribe(
    ctx: &AdapterContext,
    payload: AudioPayload,
) -> Result<CanonicalResponse, AppError> {
    let model = ctx
        .model
        .as_ref()
        .ok_or_else(|| AppError::validation("缺少模型定义"))?;
    let source = payload
        .source_audio
        .as_ref()
        .ok_or_else(|| AppError::validation("请选择源音频"))?;
    let bytes = http_util::read_file_bounded(
        std::path::Path::new(source.path.trim()),
        media::MAX_AUDIO_BYTES,
    )
    .await
    .map_err(|err| match err {
        http_util::BoundedFileReadError::TooLarge => AppError::validation("音频超过 40MB"),
        http_util::BoundedFileReadError::Io(err) => {
            AppError::validation(format!("无法读取源音频: {err}"))
        }
    })?;
    let mime = media::describe_audio(&bytes).map_err(AppError::validation)?;
    let started = Instant::now();
    let timestamp = flag(&payload.parameters, "timestamp");
    let word_timestamps = flag(&payload.parameters, "word_timestamps")
        || flag(&payload.parameters, "wordTimestamp")
        || flag(&payload.parameters, "word_timestamp");
    let speaker = flag(&payload.parameters, "speaker");
    let verbose = timestamp || word_timestamps || speaker;
    let format = if verbose { "verbose_json" } else { "json" };
    let prompt = payload.text.trim();
    let language = payload
        .parameters
        .get("language")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned);
    let url = join_url(&ctx.provider.base_url, "audio/transcriptions")?;
    let headers = request_headers(ctx);
    let mut logged = Map::new();
    logged.insert("model".to_string(), Value::String(model.model_id.clone()));
    logged.insert(
        "response_format".to_string(),
        Value::String(format.to_string()),
    );
    logged.insert("file".to_string(), Value::String("<audio>".to_string()));
    if let Some(language) = &language {
        logged.insert("language".to_string(), Value::String(language.clone()));
    }
    if !prompt.is_empty() {
        logged.insert("prompt".to_string(), Value::String(prompt.to_string()));
    }
    let raw_request = raw_http("POST", &url, &headers, &Value::Object(logged));
    let filename = match mime.as_str() {
        "audio/mpeg" => "audio.mp3",
        "audio/wav" => "audio.wav",
        "audio/ogg" => "audio.ogg",
        "audio/flac" => "audio.flac",
        "audio/mp4" => "audio.m4a",
        _ => "audio.bin",
    };
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename)
        .mime_str(&mime)
        .map_err(|_| AppError::validation("不是受支持的音频"))?;
    let mut form = reqwest::multipart::Form::new()
        .text("model", model.model_id.clone())
        .text("response_format", format)
        .part("file", part);
    if let Some(language) = language {
        form = form.text("language", language);
    }
    if !prompt.is_empty() {
        form = form.text("prompt", prompt.to_string());
    }
    if word_timestamps {
        form = form.text("timestamp_granularities[]", "word");
        form = form.text("timestamp_granularities[]", "segment");
    }
    let mut builder = ctx.http.post(&url).multipart(form);
    for (key, value) in &headers {
        builder = builder.header(key, value);
    }
    let response = http_util::send(
        builder,
        Duration::from_millis(ctx.provider.timeout_ms),
        &ctx.provider.name,
        Some(&model.name),
    )
    .await?;
    let status = response.status().as_u16();
    let text = String::from_utf8_lossy(
        &http_util::read_body(response, &ctx.provider.name, Some(&model.name)).await?,
    )
    .into_owned();
    if !(200..300).contains(&status) {
        return Err(http_util::error_from_status(
            status,
            &text,
            &ctx.provider.name,
            Some(&model.name),
        ));
    }
    let body: Value = serde_json::from_str(&text).map_err(|_| {
        AppError::provider(
            "响应不是 JSON",
            status,
            &ctx.provider.name,
            Some(&model.name),
            Some(shrink_json(&Value::String(text.clone()))),
        )
    })?;
    let transcript = transcript_of(&body, flag(&payload.parameters, "timestamp"));
    if transcript.trim().is_empty() {
        return Err(AppError::provider(
            "响应里没有文本",
            status,
            &ctx.provider.name,
            Some(&model.name),
            Some(shrink_json(&body)),
        ));
    }
    Ok(CanonicalResponse::Audio(AudioResult {
        audios: Vec::new(),
        latency_ms: elapsed_ms(started),
        provider_id: ctx.provider.id.clone(),
        provider_name: ctx.provider.name.clone(),
        model_definition_id: model.id.clone(),
        model_name: model.name.clone(),
        model_id: model.model_id.clone(),
        raw_request,
        raw_response: shrink_json(&body),
        status,
        transcript,
        transcript_path: None,
        binaries: Vec::new(),
    }))
}

fn build_speech_body(
    model_id: &str,
    payload: &AudioPayload,
) -> Result<Map<String, Value>, AppError> {
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(model_id.to_string()));
    body.insert("input".to_string(), Value::String(payload.text.clone()));
    if let Some(voice) = payload
        .voice
        .as_ref()
        .map(|text| text.trim())
        .filter(|text| !text.is_empty())
    {
        body.insert("voice".to_string(), Value::String(voice.to_string()));
    }
    for (key, value) in &payload.parameters {
        if key == "model" || key == "input" || key == "voice" {
            return Err(AppError::validation(format!("参数键 {key} 由协议保留")));
        }
        body.insert(key.clone(), value.clone());
    }
    if let Some(extra) = &payload.extra {
        let extra = extra
            .as_object()
            .ok_or_else(|| AppError::validation("Extra JSON 必须是对象"))?;
        for (key, value) in extra {
            if body.contains_key(key) {
                return Err(AppError::validation("Extra JSON 不能覆盖模板字段"));
            }
            body.insert(key.clone(), value.clone());
        }
    }
    Ok(body)
}

fn flag(parameters: &Map<String, Value>, key: &str) -> bool {
    match parameters.get(key) {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_f64().is_some_and(|number| number != 0.0),
        Some(Value::String(value)) => matches!(value.trim(), "1" | "true" | "yes"),
        _ => false,
    }
}

fn transcript_of(body: &Value, timestamps: bool) -> String {
    let plain = body
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if !timestamps {
        return plain;
    }
    let Some(segments) = body.get("segments").and_then(Value::as_array) else {
        return plain;
    };
    let mut lines = Vec::new();
    for segment in segments {
        let text = segment
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if text.is_empty() {
            continue;
        }
        let start = segment.get("start").and_then(Value::as_f64).unwrap_or(0.0);
        let end = segment.get("end").and_then(Value::as_f64).unwrap_or(0.0);
        let stamp = format!("[{} --> {}]", clock(start), clock(end));
        if let Some(speaker) = segment
            .get("speaker")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            lines.push(format!("{stamp} {speaker}: {text}"));
        } else {
            lines.push(format!("{stamp} {text}"));
        }
    }
    if lines.is_empty() {
        plain
    } else {
        lines.join("\n")
    }
}

fn clock(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let millis = total_ms % 1000;
    let total_seconds = total_ms / 1000;
    let secs = total_seconds % 60;
    let minutes = (total_seconds / 60) % 60;
    let hours = total_seconds / 3600;
    format!("{hours:02}:{minutes:02}:{secs:02}.{millis:03}")
}

pub fn srt_clock(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let millis = total_ms % 1000;
    let total_seconds = total_ms / 1000;
    let secs = total_seconds % 60;
    let minutes = (total_seconds / 60) % 60;
    let hours = total_seconds / 3600;
    format!("{hours:02}:{minutes:02}:{secs:02},{millis:03}")
}

pub fn vtt_clock(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let millis = total_ms % 1000;
    let total_seconds = total_ms / 1000;
    let secs = total_seconds % 60;
    let minutes = (total_seconds / 60) % 60;
    let hours = total_seconds / 3600;
    format!("{hours:02}:{minutes:02}:{secs:02}.{millis:03}")
}

pub fn generate_srt(body: &Value) -> Option<String> {
    let segments = body.get("segments")?.as_array()?;
    if segments.is_empty() {
        return None;
    }
    let mut entries = Vec::new();
    let mut index = 1;
    for segment in segments {
        let text = segment.get("text").and_then(Value::as_str)?.trim();
        if text.is_empty() {
            continue;
        }
        let start = segment.get("start").and_then(Value::as_f64).unwrap_or(0.0);
        let end = segment.get("end").and_then(Value::as_f64).unwrap_or(0.0);
        let speaker = segment
            .get("speaker")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let subtitle_text = if let Some(spk) = speaker {
            format!("[{spk}] {text}")
        } else {
            text.to_string()
        };
        entries.push(format!(
            "{}\n{} --> {}\n{}",
            index,
            srt_clock(start),
            srt_clock(end),
            subtitle_text
        ));
        index += 1;
    }
    if entries.is_empty() {
        None
    } else {
        Some(entries.join("\n\n"))
    }
}

pub fn generate_vtt(body: &Value) -> Option<String> {
    let segments = body.get("segments")?.as_array()?;
    if segments.is_empty() {
        return None;
    }
    let mut entries = vec!["WEBVTT\n".to_string()];
    for segment in segments {
        let text = segment.get("text").and_then(Value::as_str)?.trim();
        if text.is_empty() {
            continue;
        }
        let start = segment.get("start").and_then(Value::as_f64).unwrap_or(0.0);
        let end = segment.get("end").and_then(Value::as_f64).unwrap_or(0.0);
        let speaker = segment
            .get("speaker")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let subtitle_text = if let Some(spk) = speaker {
            format!("<v {spk}>{text}</v>")
        } else {
            text.to_string()
        };
        entries.push(format!(
            "{} --> {}\n{}",
            vtt_clock(start),
            vtt_clock(end),
            subtitle_text
        ));
    }
    if entries.len() <= 1 {
        None
    } else {
        Some(entries.join("\n"))
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}
