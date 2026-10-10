use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine;
use serde_json::{json, Map, Value};

use super::ProviderAdapter;
use futures_util::StreamExt;

use crate::canonical::{
    tool_schema, AdapterContext, CanonicalRequest, CanonicalResponse, ConnectionReport, LiveDelta,
    RemoteModel, TextPayload, TextResult, TextTurn, ToolCall, Usage,
};
use crate::error::{AppError, PROVIDER};
use crate::http_util;
use crate::media;
use crate::redact::{redact_header_map, redact_json, redact_url};
use crate::sse::{SseParser, Utf8ChunkDecoder};

pub struct OpenAiCompatibleAdapter;

pub const OPENAI: OpenAiCompatibleAdapter = OpenAiCompatibleAdapter;

const RESERVED: &[&str] = &["model", "messages", "stream"];

#[async_trait]
impl ProviderAdapter for OpenAiCompatibleAdapter {
    async fn test_connection(&self, ctx: &AdapterContext) -> Result<ConnectionReport, AppError> {
        let started = Instant::now();
        let path = ctx.models_path.as_deref().unwrap_or("models");
        let (_status, _body) = get_json(ctx, path).await?;
        Ok(ConnectionReport {
            latency_ms: elapsed_ms(started),
            message: "已连接".to_string(),
        })
    }

    async fn list_models(&self, ctx: &AdapterContext) -> Result<Vec<RemoteModel>, AppError> {
        let path = ctx.models_path.as_deref().unwrap_or("models");
        let (_status, body) = get_json(ctx, path).await?;
        read_models(&body).map_err(|err| err.context(&ctx.provider.name, None))
    }

    async fn execute(
        &self,
        ctx: &AdapterContext,
        request: CanonicalRequest,
    ) -> Result<CanonicalResponse, AppError> {
        let payload = match request {
            CanonicalRequest::Text(payload) => payload,
            CanonicalRequest::Image(payload) => {
                return super::openai_image::execute(ctx, payload).await;
            }
            CanonicalRequest::Video(_) => {
                return Err(AppError::unsupported("OpenAI 适配器目前不执行视频生成"));
            }
            CanonicalRequest::Audio(payload) => {
                return super::openai_audio::execute(ctx, payload).await;
            }
        };
        let model = ctx
            .model
            .as_ref()
            .ok_or_else(|| AppError::validation("缺少模型定义"))?;
        let started = Instant::now();
        let (_body, raw_request, response) = post_chat(ctx, &payload, false).await?;
        let (status, response_body) =
            http_util::read_response(response, &ctx.provider.name, Some(&model.name)).await?;
        let text = extract_text(&response_body)
            .map_err(|err| err.context(&ctx.provider.name, Some(&model.name)))?;
        let reasoning = extract_reasoning(&response_body);
        let mut result = finish(
            ctx,
            model,
            started,
            status,
            raw_request,
            StreamOutcome {
                text,
                reasoning,
                usage: extract_usage(&response_body),
                raw: redact_json(&response_body),
                stopped: false,
                interrupted: None,
            },
        );
        result.tool_calls = extract_tool_calls(&response_body);
        Ok(CanonicalResponse::Text(result))
    }

    async fn stream(
        &self,
        ctx: &AdapterContext,
        request: CanonicalRequest,
        deltas: &(dyn Fn(LiveDelta) + Send + Sync),
    ) -> Result<TextResult, AppError> {
        let CanonicalRequest::Text(payload) = request else {
            return Err(AppError::unsupported("图像生成不支持流式输出"));
        };
        let model = ctx
            .model
            .as_ref()
            .ok_or_else(|| AppError::validation("缺少模型定义"))?;
        let started = Instant::now();
        let (_body, raw_request, response) = post_chat(ctx, &payload, true).await?;
        let status = response.status().as_u16();
        let outcome = read_openai_stream(ctx, model, response, deltas).await?;
        Ok(finish(ctx, model, started, status, raw_request, outcome))
    }
}

pub fn build_chat_body(
    model_id: &str,
    messages: Vec<Value>,
    parameters: &Map<String, Value>,
    extra: Option<&Value>,
    stream: bool,
) -> Result<Value, AppError> {
    let mut body = json!({
        "model": model_id,
        "messages": messages,
        "stream": stream,
    });
    let Some(object) = body.as_object_mut() else {
        return Err(AppError::storage("无法构造请求"));
    };
    for (key, value) in parameters {
        if RESERVED.contains(&key.as_str()) {
            return Err(AppError::validation(format!("参数键 {key} 由协议保留")));
        }
        object.insert(key.clone(), value.clone());
    }
    if let Some(extra) = extra {
        let extra = extra
            .as_object()
            .ok_or_else(|| AppError::validation("Extra JSON 必须是对象"))?;
        for (key, value) in extra {
            if key == "model" || key == "messages" {
                return Err(AppError::validation(
                    "Extra JSON 不能覆盖 model 或 messages",
                ));
            }
            object.insert(key.clone(), value.clone());
        }
    }
    object.insert("stream".to_string(), Value::Bool(stream));
    if let Some(Value::String(format)) = object.get("response_format").cloned() {
        if format == "text" || format.is_empty() {
            object.remove("response_format");
        } else {
            object.insert("response_format".to_string(), json!({ "type": format }));
        }
    }
    Ok(body)
}

fn chat_messages(payload: &TextPayload) -> Result<Vec<Value>, AppError> {
    let mut messages = Vec::new();
    if let Some(system) = payload
        .system_prompt
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        messages.push(json!({ "role": "system", "content": system }));
    }
    if payload.turns.is_empty() {
        return Err(AppError::validation("没有可发送的消息"));
    }
    for turn in &payload.turns {
        messages.push(turn_message(turn)?);
    }
    Ok(messages)
}

fn turn_message(turn: &TextTurn) -> Result<Value, AppError> {
    match turn.role.as_str() {
        "assistant" => {
            if !turn.images.is_empty() {
                return Err(AppError::validation("助手消息不能包含图像"));
            }
            if turn.tool_calls.is_empty() {
                return Ok(json!({ "role": "assistant", "content": turn.text }));
            }
            let content = if turn.text.is_empty() {
                Value::Null
            } else {
                Value::String(turn.text.clone())
            };
            Ok(json!({
                "role": "assistant",
                "content": content,
                "tool_calls": openai_calls(&turn.tool_calls),
            }))
        }
        "tool" => {
            let id = turn
                .tool_call_id
                .as_deref()
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .ok_or_else(|| AppError::validation("工具结果缺少编号"))?;
            Ok(json!({
                "role": "tool",
                "tool_call_id": id,
                "content": turn.text,
            }))
        }
        "user" => Ok(json!({ "role": "user", "content": user_content(turn)? })),
        _ => Err(AppError::validation("消息角色无效")),
    }
}

fn openai_calls(calls: &[ToolCall]) -> Vec<Value> {
    calls
        .iter()
        .map(|call| {
            json!({
                "id": call.id,
                "type": "function",
                "function": {
                    "name": call.name,
                    "arguments": call.arguments,
                }
            })
        })
        .collect()
}

fn user_content(turn: &TextTurn) -> Result<Value, AppError> {
    if turn.images.is_empty() {
        return Ok(Value::String(turn.text.clone()));
    }
    if turn.images.len() > 8 {
        return Err(AppError::validation("一次最多附加 8 张图像"));
    }
    let mut parts = vec![json!({ "type": "text", "text": turn.text })];
    for image in &turn.images {
        parts.push(json!({
            "type": "image_url",
            "image_url": { "url": data_url(&image.path)? }
        }));
    }
    Ok(Value::Array(parts))
}

async fn post_chat(
    ctx: &AdapterContext,
    payload: &TextPayload,
    stream: bool,
) -> Result<(Value, Value, reqwest::Response), AppError> {
    let model = ctx
        .model
        .as_ref()
        .ok_or_else(|| AppError::validation("缺少模型定义"))?;
    let mut body = build_chat_body(
        &model.model_id,
        chat_messages(payload)?,
        &payload.parameters,
        payload.extra.as_ref(),
        stream,
    )?;
    if !payload.tools.is_empty() {
        if let Some(object) = body.as_object_mut() {
            object.insert("tools".to_string(), tool_schema(&payload.tools));
            object.insert("tool_choice".to_string(), json!("auto"));
        }
    }
    let path = ctx.chat_path.as_deref().unwrap_or("chat/completions");
    let url = http_util::join_url(&ctx.provider.base_url, path)?;
    let headers = request_headers(ctx);
    let (raw_request, response) =
        super::exchange::send(ctx, Some(&model.name), "POST", &url, &headers, Some(&body)).await?;
    Ok((body, raw_request, response))
}

struct StreamOutcome {
    text: String,
    reasoning: String,
    usage: Option<Usage>,
    raw: Value,
    stopped: bool,
    interrupted: Option<String>,
}

fn finish(
    ctx: &AdapterContext,
    model: &crate::domain::ModelDefinition,
    started: Instant,
    status: u16,
    raw_request: Value,
    outcome: StreamOutcome,
) -> TextResult {
    TextResult {
        text: outcome.text,
        usage: outcome.usage,
        latency_ms: elapsed_ms(started),
        provider_id: ctx.provider.id.clone(),
        provider_name: ctx.provider.name.clone(),
        model_definition_id: model.id.clone(),
        model_name: model.name.clone(),
        model_id: model.model_id.clone(),
        raw_request,
        raw_response: outcome.raw,
        status,
        stopped: outcome.stopped,
        reasoning: outcome.reasoning,
        interrupted: outcome.interrupted,
        tool_calls: Vec::new(),
    }
}

async fn read_openai_stream(
    ctx: &AdapterContext,
    model: &crate::domain::ModelDefinition,
    response: reqwest::Response,
    deltas: &(dyn Fn(LiveDelta) + Send + Sync),
) -> Result<StreamOutcome, AppError> {
    use std::sync::atomic::Ordering;

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
    let mut stream = response.bytes_stream();
    let mut pending = String::new();
    let mut mode: Option<bool> = None;
    let mut parser = SseParser::default();
    let mut decoder = Utf8ChunkDecoder::new();
    let mut outcome = StreamOutcome {
        text: String::new(),
        reasoning: String::new(),
        usage: None,
        raw: Value::Null,
        stopped: false,
        interrupted: None,
    };
    let mut completed_normally = false;
    while let Some(chunk) = tokio::select! {
        _ = super::exchange::wait_for_cancel(&ctx.cancelled) => None,
        chunk = stream.next() => chunk,
    } {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(err) => {
                let error = http_util::map_reqwest(err, &ctx.provider.name, Some(&model.name));
                if outcome.text.is_empty() && outcome.reasoning.is_empty() {
                    return Err(error);
                }
                outcome.interrupted = Some(error.message);
                break;
            }
        };
        let piece = decoder.decode(&chunk);
        if piece.is_empty() {
            continue;
        }
        if mode.is_none() {
            pending.push_str(&piece);
            match looks_like_sse(&pending) {
                None => continue,
                Some(true) => {
                    mode = Some(true);
                    apply_sse(
                        &pending,
                        &mut parser,
                        deltas,
                        &mut outcome,
                        &mut completed_normally,
                    )?;
                    pending.clear();
                }
                Some(false) => mode = Some(false),
            }
            continue;
        }
        if mode == Some(true) {
            apply_sse(
                &piece,
                &mut parser,
                deltas,
                &mut outcome,
                &mut completed_normally,
            )?;
            if outcome.interrupted.is_some() {
                break;
            }
        } else {
            pending.push_str(&piece);
        }
    }
    outcome.stopped = ctx.cancelled.load(Ordering::Relaxed);
    let trailing = decoder.finish();
    if !outcome.stopped && mode == Some(true) {
        let mut final_chunk = trailing;
        final_chunk.push_str("\n\n");
        let _ = apply_sse(
            &final_chunk,
            &mut parser,
            deltas,
            &mut outcome,
            &mut completed_normally,
        );
    } else if !outcome.stopped && !trailing.is_empty() {
        pending.push_str(&trailing);
    }
    if mode == Some(true)
        && !completed_normally
        && !outcome.stopped
        && outcome.interrupted.is_none()
    {
        if outcome.text.is_empty() && outcome.reasoning.is_empty() {
            return Err(AppError::network(
                "流式响应异常中断 (未收到 [DONE] 或完成原因)",
                &ctx.provider.name,
                Some(&model.name),
            ));
        } else {
            outcome.interrupted = Some("流式响应异常中断 (未收到 [DONE] 或完成原因)".to_string());
        }
    }
    if !outcome.stopped && mode == Some(false) {
        let value = serde_json::from_str::<Value>(&pending).map_err(|_| {
            AppError::provider(
                "响应不是 JSON",
                status,
                &ctx.provider.name,
                Some(&model.name),
                Some(Value::String(pending.chars().take(2000).collect())),
            )
        })?;
        let text = extract_text(&value)
            .map_err(|err| err.context(&ctx.provider.name, Some(&model.name)))?;
        if !text.is_empty() {
            deltas(LiveDelta::Text(text.clone()));
        }
        outcome.text = text;
        outcome.usage = extract_usage(&value);
        outcome.raw = redact_json(&value);
    }
    Ok(outcome)
}

fn looks_like_sse(buffer: &str) -> Option<bool> {
    let trimmed = buffer.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("data:") || trimmed.starts_with(':') || trimmed.starts_with("event:") {
        return Some(true);
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return Some(false);
    }
    if trimmed.len() >= 8 {
        return Some(false);
    }
    None
}

fn apply_sse(
    chunk: &str,
    parser: &mut SseParser,
    deltas: &(dyn Fn(LiveDelta) + Send + Sync),
    outcome: &mut StreamOutcome,
    completed_normally: &mut bool,
) -> Result<(), AppError> {
    for data in parser.push(chunk) {
        if data.trim() == "[DONE]" {
            *completed_normally = true;
            return Ok(());
        }
        let value: Value = match serde_json::from_str(&data) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
            .is_some()
        {
            *completed_normally = true;
        }
        if let Some(message) = value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            outcome.interrupted = Some(crate::redact::mask_embedded(message));
            outcome.raw = redact_json(&value);
            return Ok(());
        }
        if let Some(usage) = extract_usage(&value) {
            outcome.usage = Some(usage);
        }
        let delta = value
            .pointer("/choices/0/delta")
            .cloned()
            .unwrap_or(Value::Null);
        if let Some(text) = delta.get("content").and_then(Value::as_str) {
            if !text.is_empty() {
                outcome.text.push_str(text);
                deltas(LiveDelta::Text(text.to_string()));
            }
        }
        let reasoning = delta
            .get("reasoning_content")
            .or_else(|| delta.get("reasoning"))
            .and_then(Value::as_str);
        if let Some(text) = reasoning {
            if !text.is_empty() {
                outcome.reasoning.push_str(text);
                deltas(LiveDelta::Reasoning(text.to_string()));
            }
        }
        outcome.raw = redact_json(&value);
    }
    Ok(())
}

pub(crate) fn data_url_parts(path: &str) -> Result<(&'static str, Vec<u8>), AppError> {
    if let Some(rest) = path.strip_prefix("data:") {
        if let Some((header, encoded)) = rest.split_once(',') {
            let mime = if header.starts_with("image/png") {
                "image/png"
            } else if header.starts_with("image/jpeg") || header.starts_with("image/jpg") {
                "image/jpeg"
            } else if header.starts_with("image/webp") {
                "image/webp"
            } else if header.starts_with("image/gif") {
                "image/gif"
            } else {
                return Err(AppError::validation("图像只支持 png、jpg、webp、gif"));
            };
            let encoded = encoded.trim();
            if encoded.len() > media::MAX_IMAGE_BYTES.div_ceil(3) * 4 {
                return Err(AppError::validation("单个图像超过 15MB"));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| AppError::validation("图像 data URL 无法解码"))?;
            if bytes.len() > media::MAX_IMAGE_BYTES {
                return Err(AppError::validation("单个图像超过 15MB"));
            }
            return Ok((mime, bytes));
        }
    }
    let file = Path::new(path);
    if !file.is_file() {
        return Err(AppError::validation(format!("找不到图像: {path}")));
    }
    let bytes =
        http_util::read_file_bounded_sync(file, media::MAX_IMAGE_BYTES).map_err(
            |err| match err {
                http_util::BoundedFileReadError::TooLarge => {
                    AppError::validation("单个图像超过 15MB")
                }
                http_util::BoundedFileReadError::Io(err) => {
                    AppError::validation(format!("无法读取图像: {err}"))
                }
            },
        )?;
    let mime = match file
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return Err(AppError::validation("图像只支持 png、jpg、webp、gif")),
    };
    Ok((mime, bytes))
}

fn data_url(path: &str) -> Result<String, AppError> {
    let (mime, bytes) = data_url_parts(path)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{mime};base64,{encoded}"))
}

pub(crate) fn request_headers(ctx: &AdapterContext) -> BTreeMap<String, String> {
    let mut headers = ctx.provider.headers.clone();
    if let Some(secret) = &ctx.api_key {
        headers.insert(
            "Authorization".to_string(),
            format!("Bearer {}", secret.expose()),
        );
    }
    headers
}

pub(crate) fn raw_http(
    method: &str,
    url: &str,
    headers: &BTreeMap<String, String>,
    body: &Value,
) -> Value {
    let mut display = redact_header_map(headers);
    display
        .entry("Content-Type".to_string())
        .or_insert_with(|| "application/json".to_string());
    json!({
        "method": method,
        "url": redact_url(url),
        "headers": display,
        "body": redact_json(body),
    })
}

fn read_models(body: &Value) -> Result<Vec<RemoteModel>, AppError> {
    let data = body
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::new(PROVIDER, "响应里没有模型列表"))?;
    let mut models: Vec<RemoteModel> = data
        .iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.trim();
            if id.is_empty() {
                return None;
            }
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or(id);
            Some(RemoteModel {
                id: id.to_string(),
                name: name.to_string(),
            })
        })
        .take(2000)
        .collect();
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
    Ok(models)
}

fn extract_tool_calls(body: &Value) -> Vec<ToolCall> {
    let Some(calls) = body
        .pointer("/choices/0/message/tool_calls")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    calls
        .iter()
        .enumerate()
        .filter_map(|(index, call)| {
            let function = call.get("function")?;
            let name = function.get("name").and_then(Value::as_str)?.trim();
            if name.is_empty() {
                return None;
            }
            let arguments = match function.get("arguments") {
                Some(Value::String(text)) => text.clone(),
                Some(Value::Null) | None => "{}".to_string(),
                Some(other) => other.to_string(),
            };
            let id = call
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty() && item.chars().count() <= 80)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("call_{index}"));
            Some(ToolCall {
                id,
                name: name.to_string(),
                arguments,
            })
        })
        .collect()
}

fn extract_text(body: &Value) -> Result<String, AppError> {
    let choice = body
        .pointer("/choices/0")
        .ok_or_else(|| AppError::new(PROVIDER, "响应里没有 choices"))?;
    if let Some(refusal) = choice.pointer("/message/refusal").and_then(Value::as_str) {
        if !refusal.is_empty() {
            return Err(AppError::new(
                PROVIDER,
                format!("模型拒绝了请求：{refusal}"),
            ));
        }
    }
    match choice.pointer("/message/content").unwrap_or(&Value::Null) {
        Value::String(text) => Ok(text.clone()),
        Value::Array(parts) => {
            let mut text = String::new();
            for part in parts {
                if let Some(value) = part.get("text").and_then(Value::as_str) {
                    text.push_str(value);
                } else if let Some(value) = part.as_str() {
                    text.push_str(value);
                }
            }
            Ok(text)
        }
        Value::Null => Ok(String::new()),
        _ => Err(AppError::new(PROVIDER, "无法读取响应文本")),
    }
}

fn extract_reasoning(body: &Value) -> String {
    let choice = match body.pointer("/choices/0") {
        Some(c) => c,
        None => return String::new(),
    };
    let reasoning_val = choice
        .pointer("/message/reasoning_content")
        .or_else(|| choice.pointer("/message/reasoning"))
        .or_else(|| choice.pointer("/message/thought"));
    match reasoning_val {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => {
            let mut text = String::new();
            for part in parts {
                if let Some(val) = part.get("text").and_then(Value::as_str) {
                    text.push_str(val);
                } else if let Some(val) = part.as_str() {
                    text.push_str(val);
                }
            }
            text
        }
        _ => String::new(),
    }
}

fn extract_usage(body: &Value) -> Option<Usage> {
    let usage = body.get("usage")?;
    let reasoning_tokens = usage
        .pointer("/completion_tokens_details/reasoning_tokens")
        .or_else(|| usage.get("reasoning_tokens"))
        .and_then(Value::as_u64);
    Some(Usage {
        input_tokens: usage.get("prompt_tokens").and_then(Value::as_u64),
        output_tokens: usage.get("completion_tokens").and_then(Value::as_u64),
        total_tokens: usage.get("total_tokens").and_then(Value::as_u64),
        reasoning_tokens,
    })
}

async fn get_json(ctx: &AdapterContext, path: &str) -> Result<(u16, Value), AppError> {
    let url = http_util::join_url(&ctx.provider.base_url, path)?;
    let headers = request_headers(ctx);
    let mut builder = ctx.http.get(url);
    for (key, value) in &headers {
        builder = builder.header(key, value);
    }
    let response = http_util::send(
        builder,
        Duration::from_millis(ctx.provider.timeout_ms),
        &ctx.provider.name,
        None,
    )
    .await?;
    http_util::read_response(response, &ctx.provider.name, None).await
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::VALIDATION;

    #[test]
    fn builds_canonical_openai_body() {
        let mut parameters = Map::new();
        parameters.insert("temperature".to_string(), json!(0.2));
        parameters.insert("response_format".to_string(), json!("json_object"));
        let body = build_chat_body(
            "demo-model",
            vec![
                json!({ "role": "system", "content": "be brief" }),
                json!({ "role": "user", "content": "ping" }),
            ],
            &parameters,
            Some(&json!({ "custom_flag": true, "stream": true })),
            false,
        )
        .unwrap();
        assert_eq!(body["model"], "demo-model");
        assert_eq!(body["temperature"], 0.2);
        assert_eq!(body["custom_flag"], true);
        assert_eq!(body["stream"], false);
        assert_eq!(body["response_format"]["type"], "json_object");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "be brief");
        assert_eq!(body["messages"][1]["content"], "ping");

        let mut text_format = Map::new();
        text_format.insert("response_format".to_string(), json!("text"));
        let plain = build_chat_body(
            "demo-model",
            vec![json!({ "role": "user", "content": "ping" })],
            &text_format,
            None,
            false,
        )
        .unwrap();
        assert!(plain.get("response_format").is_none());

        let error = build_chat_body(
            "demo-model",
            vec![json!({ "role": "user", "content": "ping" })],
            &Map::new(),
            Some(&json!({ "model": "other" })),
            false,
        )
        .unwrap_err();
        assert_eq!(error.code, VALIDATION);
    }

    #[test]
    fn encodes_local_image_without_putting_bytes_in_the_path() {
        let path = std::env::temp_dir().join(format!("aiwb-{}.png", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"png-bytes").unwrap();
        let url = data_url(path.to_str().unwrap()).unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        let _ = std::fs::remove_file(path);
        assert!(data_url("note.txt").is_err());

        // Also test decoding data URL directly
        let (mime, bytes) = data_url_parts("data:image/png;base64,cG5nLWJ5dGVz").unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(bytes, b"png-bytes");
    }
}
