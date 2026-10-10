use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::time::Instant;

use async_trait::async_trait;
use base64::Engine;
use futures_util::StreamExt;
use serde_json::{json, Map, Value};

use super::exchange::{self, elapsed_ms};
use super::ProviderAdapter;
use crate::canonical::{
    AdapterContext, CanonicalRequest, CanonicalResponse, ConnectionReport, LiveDelta, RemoteModel,
    TextPayload, TextResult, TextTurn, Usage,
};
use crate::domain::ModelDefinition;
use crate::error::AppError;
use crate::http_util;
use crate::redact::shrink_json;
use crate::sse::{SseParser, Utf8ChunkDecoder};

pub struct GeminiAdapter;

pub const GEMINI: GeminiAdapter = GeminiAdapter;

#[async_trait]
impl ProviderAdapter for GeminiAdapter {
    async fn test_connection(&self, ctx: &AdapterContext) -> Result<ConnectionReport, AppError> {
        let started = Instant::now();
        let _ = list_models(ctx).await?;
        Ok(ConnectionReport {
            latency_ms: elapsed_ms(started),
            message: "已连接".to_string(),
        })
    }

    async fn list_models(&self, ctx: &AdapterContext) -> Result<Vec<RemoteModel>, AppError> {
        list_models(ctx).await
    }

    async fn execute(
        &self,
        ctx: &AdapterContext,
        request: CanonicalRequest,
    ) -> Result<CanonicalResponse, AppError> {
        let CanonicalRequest::Text(payload) = request else {
            return Err(AppError::unsupported("Gemini 适配器目前只执行文本生成"));
        };
        let model = model_of(ctx)?;
        let started = Instant::now();
        let body = build_body(&payload)?;
        let url = http_util::join_url(
            &ctx.provider.base_url,
            &format!(
                "v1beta/models/{}:generateContent",
                model_id(&model.model_id)
            ),
        )?;
        let headers = headers_of(ctx);
        let (raw, response) =
            exchange::send(ctx, Some(&model.name), "POST", &url, &headers, Some(&body)).await?;
        let (status, value) =
            http_util::read_response(response, &ctx.provider.name, Some(&model.name)).await?;
        let mut result = text_result(ctx, model, started, status, raw, &value, String::new());
        result.tool_calls = tool_calls_of(&value);
        Ok(CanonicalResponse::Text(result))
    }

    async fn stream(
        &self,
        ctx: &AdapterContext,
        request: CanonicalRequest,
        deltas: &(dyn Fn(LiveDelta) + Send + Sync),
    ) -> Result<TextResult, AppError> {
        let CanonicalRequest::Text(payload) = request else {
            return Err(AppError::unsupported("Gemini 适配器目前只执行文本生成"));
        };
        let model = model_of(ctx)?;
        let started = Instant::now();
        let body = build_body(&payload)?;
        let url = http_util::join_url(
            &ctx.provider.base_url,
            &format!(
                "v1beta/models/{}:streamGenerateContent?alt=sse",
                model_id(&model.model_id)
            ),
        )?;
        let headers = headers_of(ctx);
        let (raw, response) =
            exchange::send(ctx, Some(&model.name), "POST", &url, &headers, Some(&body)).await?;
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
        read_stream(ctx, model, response, deltas, started, status, raw).await
    }
}

fn model_of(ctx: &AdapterContext) -> Result<&ModelDefinition, AppError> {
    ctx.model
        .as_ref()
        .ok_or_else(|| AppError::validation("缺少模型定义"))
}

fn model_id(value: &str) -> String {
    value.trim().trim_start_matches("models/").to_string()
}

fn headers_of(ctx: &AdapterContext) -> BTreeMap<String, String> {
    let mut headers = ctx.provider.headers.clone();
    if let Some(secret) = &ctx.api_key {
        headers.insert("x-goog-api-key".to_string(), secret.expose().to_string());
    }
    headers
}

async fn list_models(ctx: &AdapterContext) -> Result<Vec<RemoteModel>, AppError> {
    let _ = exchange::require_key(ctx)?;
    let path = ctx.models_path.as_deref().unwrap_or("v1beta/models");
    let url = http_util::join_url(&ctx.provider.base_url, path)?;
    let headers = headers_of(ctx);
    let (_raw, response) = exchange::send(ctx, None, "GET", &url, &headers, None).await?;
    let (_status, body) = http_util::read_response(response, &ctx.provider.name, None).await?;
    let items = body
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut models = Vec::new();
    for item in items {
        let Some(name) = item.get("name").and_then(Value::as_str) else {
            continue;
        };
        let id = model_id(name);
        let name_label = item
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or(&id)
            .to_string();
        models.push(RemoteModel {
            id,
            name: name_label,
        });
    }
    Ok(models)
}

fn build_body(payload: &TextPayload) -> Result<Value, AppError> {
    let mut body = Map::new();
    body.insert("contents".to_string(), json!(contents_of(payload)?));
    if let Some(system) = payload
        .system_prompt
        .as_ref()
        .map(|text| text.trim())
        .filter(|text| !text.is_empty())
    {
        body.insert(
            "systemInstruction".to_string(),
            json!({ "parts": [{ "text": system }] }),
        );
    }
    let mut config = Map::new();
    for (key, value) in &payload.parameters {
        if matches!(
            key.as_str(),
            "model" | "contents" | "systemInstruction" | "tools"
        ) {
            return Err(AppError::validation(format!("参数键 {key} 由协议保留")));
        }
        let name = if key == "max_tokens" {
            "maxOutputTokens"
        } else {
            key.as_str()
        };
        config.insert(name.to_string(), value.clone());
    }
    if !config.is_empty() {
        body.insert("generationConfig".to_string(), Value::Object(config));
    }
    if !payload.tools.is_empty() {
        let declarations = payload
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters,
                })
            })
            .collect::<Vec<_>>();
        body.insert(
            "tools".to_string(),
            json!([{ "functionDeclarations": declarations }]),
        );
    }
    if let Some(extra) = &payload.extra {
        let extra = extra
            .as_object()
            .ok_or_else(|| AppError::validation("Extra JSON 必须是对象"))?;
        for (key, value) in extra {
            if matches!(
                key.as_str(),
                "model" | "contents" | "systemInstruction" | "generationConfig" | "tools"
            ) {
                return Err(AppError::validation(format!(
                    "Extra JSON 不能覆盖协议字段 {key}"
                )));
            }
            body.insert(key.clone(), value.clone());
        }
    }
    Ok(Value::Object(body))
}

fn contents_of(payload: &TextPayload) -> Result<Vec<Value>, AppError> {
    let mut contents: Vec<Value> = Vec::new();
    for turn in &payload.turns {
        let role = if turn.role == "assistant" {
            "model"
        } else {
            "user"
        };
        let parts = parts_of(turn)?;
        if parts.is_empty() {
            continue;
        }
        if let Some(last) = contents.last_mut() {
            if last.get("role").and_then(Value::as_str) == Some(role) {
                if let Some(stored) = last.get_mut("parts").and_then(Value::as_array_mut) {
                    stored.extend(parts);
                    continue;
                }
            }
        }
        contents.push(json!({ "role": role, "parts": parts }));
    }
    if contents.is_empty() {
        return Err(AppError::validation("没有可发送的消息"));
    }
    Ok(contents)
}

fn parts_of(turn: &TextTurn) -> Result<Vec<Value>, AppError> {
    let mut parts = Vec::new();
    if turn.role == "tool" {
        let name = turn
            .name
            .clone()
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| "tool".to_string());
        parts.push(json!({
            "functionResponse": {
                "name": name,
                "response": { "result": turn.text }
            }
        }));
        return Ok(parts);
    }
    if !turn.text.trim().is_empty() {
        parts.push(json!({ "text": turn.text }));
    }
    for call in &turn.tool_calls {
        let args = serde_json::from_str::<Value>(&call.arguments).unwrap_or(json!({}));
        parts.push(json!({
            "functionCall": { "name": call.name, "args": args }
        }));
    }
    for image in &turn.images {
        let (mime, bytes) = crate::adapters::data_url_parts(&image.path)?;
        parts.push(json!({
            "inline_data": {
                "mime_type": mime,
                "data": base64::engine::general_purpose::STANDARD.encode(bytes),
            }
        }));
    }
    Ok(parts)
}

fn parts_in(body: &Value) -> Vec<Value> {
    body.pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn text_of(body: &Value) -> String {
    parts_in(body)
        .iter()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("")
}

fn tool_calls_of(body: &Value) -> Vec<crate::canonical::ToolCall> {
    parts_in(body)
        .iter()
        .filter_map(|part| part.get("functionCall"))
        .filter_map(|call| {
            Some(crate::canonical::ToolCall {
                id: format!(
                    "gemini-{}",
                    call.get("name").and_then(Value::as_str).unwrap_or("tool")
                ),
                name: call.get("name")?.as_str()?.to_string(),
                arguments: serde_json::to_string(call.get("args").unwrap_or(&json!({}))).ok()?,
            })
        })
        .collect()
}

fn usage_of(body: &Value) -> Option<Usage> {
    let usage = body.get("usageMetadata")?;
    let reasoning_tokens = usage.get("thoughtsTokenCount").and_then(Value::as_u64);
    Some(Usage {
        input_tokens: usage.get("promptTokenCount").and_then(Value::as_u64),
        output_tokens: usage.get("candidatesTokenCount").and_then(Value::as_u64),
        total_tokens: usage.get("totalTokenCount").and_then(Value::as_u64),
        reasoning_tokens,
    })
}

fn text_result(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    started: Instant,
    status: u16,
    raw_request: Value,
    body: &Value,
    reasoning: String,
) -> TextResult {
    TextResult {
        text: text_of(body),
        usage: usage_of(body),
        latency_ms: elapsed_ms(started),
        provider_id: ctx.provider.id.clone(),
        provider_name: ctx.provider.name.clone(),
        model_definition_id: model.id.clone(),
        model_name: model.name.clone(),
        model_id: model.model_id.clone(),
        raw_request,
        raw_response: shrink_json(body),
        status,
        stopped: false,
        reasoning,
        interrupted: None,
        tool_calls: Vec::new(),
    }
}

async fn read_stream(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    response: reqwest::Response,
    deltas: &(dyn Fn(LiveDelta) + Send + Sync),
    started: Instant,
    status: u16,
    raw_request: Value,
) -> Result<TextResult, AppError> {
    let mut parser = SseParser::default();
    let mut decoder = Utf8ChunkDecoder::default();
    let mut text = String::new();
    let mut usage = None;
    let mut interrupted = None;
    let mut completed_normally = false;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = tokio::select! {
        _ = exchange::wait_for_cancel(&ctx.cancelled) => None,
        chunk = stream.next() => chunk,
    } {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(err) => {
                let err_msg = format!("流式响应中断: {err}");
                if text.is_empty() {
                    return Err(AppError::network(
                        err_msg,
                        &ctx.provider.name,
                        Some(&model.name),
                    ));
                }
                interrupted = Some(err_msg);
                break;
            }
        };
        let decoded = decoder.decode(&chunk);
        for data in parser.push(&decoded) {
            let Ok(value) = serde_json::from_str::<Value>(&data) else {
                continue;
            };
            if let Some(err_node) = value.get("error") {
                let message = err_node
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Gemini 流式异常");
                if text.is_empty() {
                    return Err(AppError::provider(
                        message,
                        status,
                        &ctx.provider.name,
                        Some(&model.name),
                        Some(crate::redact::shrink_json(&value)),
                    ));
                }
                interrupted = Some(message.to_string());
                break;
            }
            let bit = text_of(&value);
            if !bit.is_empty() {
                text.push_str(&bit);
                deltas(LiveDelta::Text(bit));
            }
            if let Some(next) = usage_of(&value) {
                usage = Some(next);
            }
            if value
                .pointer("/candidates/0/finishReason")
                .and_then(Value::as_str)
                .is_some()
            {
                completed_normally = true;
            }
        }
        if interrupted.is_some() {
            break;
        }
    }
    let stopped = ctx.cancelled.load(Ordering::Relaxed);
    if !stopped && interrupted.is_none() {
        let mut trailing = decoder.finish();
        // Flush a final SSE event even when the server closes after one
        // newline instead of sending the usual blank-line separator.
        trailing.push_str("\n\n");
        for data in parser.push(&trailing) {
            let Ok(value) = serde_json::from_str::<Value>(&data) else {
                continue;
            };
            let bit = text_of(&value);
            if !bit.is_empty() {
                text.push_str(&bit);
                deltas(LiveDelta::Text(bit));
            }
            if let Some(next) = usage_of(&value) {
                usage = Some(next);
            }
            if value
                .pointer("/candidates/0/finishReason")
                .and_then(Value::as_str)
                .is_some()
            {
                completed_normally = true;
            }
        }
    }
    if interrupted.is_none() && !stopped && !completed_normally {
        interrupted = Some("流式响应异常中断 (未收到 finishReason)".to_string());
    }
    Ok(TextResult {
        text,
        usage,
        latency_ms: elapsed_ms(started),
        provider_id: ctx.provider.id.clone(),
        provider_name: ctx.provider.name.clone(),
        model_definition_id: model.id.clone(),
        model_name: model.name.clone(),
        model_id: model.model_id.clone(),
        raw_request,
        raw_response: json!({ "stream": true }),
        status,
        stopped,
        reasoning: String::new(),
        interrupted,
        tool_calls: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn includes_extra_fields_and_rejects_protocol_overwrites() {
        let payload = TextPayload {
            system_prompt: None,
            turns: vec![TextTurn::plain("user", "ping", Vec::new())],
            parameters: Map::new(),
            extra: Some(json!({ "safetySettings": [] })),
            tools: Vec::new(),
        };
        let body = build_body(&payload).unwrap();
        assert_eq!(body["safetySettings"], json!([]));

        let mut invalid = payload;
        invalid.extra = Some(json!({ "contents": [] }));
        let error = build_body(&invalid).unwrap_err();
        assert_eq!(error.code, crate::error::VALIDATION);
    }
}
