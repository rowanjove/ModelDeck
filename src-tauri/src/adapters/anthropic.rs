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
    TextPayload, TextResult, TextTurn, ToolCall, Usage,
};
use crate::domain::ModelDefinition;
use crate::error::AppError;
use crate::http_util;
use crate::redact::shrink_json;
use crate::sse::{SseParser, Utf8ChunkDecoder};

pub struct AnthropicAdapter;

pub const ANTHROPIC: AnthropicAdapter = AnthropicAdapter;

#[async_trait]
impl ProviderAdapter for AnthropicAdapter {
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
            return Err(AppError::unsupported("Anthropic 适配器目前只执行文本生成"));
        };
        let model = model_of(ctx)?;
        let started = Instant::now();
        let body = build_body(model, &payload, false)?;
        let url = http_util::join_url(&ctx.provider.base_url, "v1/messages")?;
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
            return Err(AppError::unsupported("Anthropic 适配器目前只执行文本生成"));
        };
        let model = model_of(ctx)?;
        let started = Instant::now();
        let body = build_body(model, &payload, true)?;
        let url = http_util::join_url(&ctx.provider.base_url, "v1/messages")?;
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

fn headers_of(ctx: &AdapterContext) -> BTreeMap<String, String> {
    let mut headers = ctx.provider.headers.clone();
    if let Some(secret) = &ctx.api_key {
        headers.insert("x-api-key".to_string(), secret.expose().to_string());
    }
    headers
        .entry("anthropic-version".to_string())
        .or_insert_with(|| "2023-06-01".to_string());
    headers
}

async fn list_models(ctx: &AdapterContext) -> Result<Vec<RemoteModel>, AppError> {
    let _ = exchange::require_key(ctx)?;
    let path = ctx.models_path.as_deref().unwrap_or("v1/models");
    let url = http_util::join_url(&ctx.provider.base_url, path)?;
    let headers = headers_of(ctx);
    let (_raw, response) = exchange::send(ctx, None, "GET", &url, &headers, None).await?;
    let (_status, body) = http_util::read_response(response, &ctx.provider.name, None).await?;
    let items = body
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut models = Vec::new();
    for item in items {
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            continue;
        };
        let name = item
            .get("display_name")
            .and_then(Value::as_str)
            .unwrap_or(id);
        models.push(RemoteModel {
            id: id.to_string(),
            name: name.to_string(),
        });
    }
    Ok(models)
}

pub fn build_body(
    model: &ModelDefinition,
    payload: &TextPayload,
    stream: bool,
) -> Result<Value, AppError> {
    let mut body = Map::new();
    body.insert("model".to_string(), json!(model.model_id));
    body.insert("messages".to_string(), json!(messages_of(payload)?));
    if let Some(system) = payload
        .system_prompt
        .as_ref()
        .map(|text| text.trim())
        .filter(|text| !text.is_empty())
    {
        body.insert("system".to_string(), json!(system));
    }
    let mut max_tokens = 4096_u64;
    for (key, value) in &payload.parameters {
        if matches!(
            key.as_str(),
            "model" | "messages" | "system" | "stream" | "tools"
        ) {
            return Err(AppError::validation(format!("参数键 {key} 由协议保留")));
        }
        if key == "max_tokens" {
            max_tokens = value.as_u64().unwrap_or(max_tokens);
            continue;
        }
        body.insert(key.clone(), value.clone());
    }
    body.insert("max_tokens".to_string(), json!(max_tokens));
    if !payload.tools.is_empty() {
        let tools = payload
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.parameters,
                })
            })
            .collect::<Vec<_>>();
        body.insert("tools".to_string(), json!(tools));
    }
    if let Some(extra) = &payload.extra {
        let extra = extra
            .as_object()
            .ok_or_else(|| AppError::validation("Extra JSON 必须是对象"))?;
        for (key, value) in extra {
            if matches!(
                key.as_str(),
                "model" | "messages" | "system" | "stream" | "tools"
            ) {
                return Err(AppError::validation(format!(
                    "Extra JSON 不能覆盖协议字段 {key}"
                )));
            }
            body.insert(key.clone(), value.clone());
        }
    }
    if stream {
        body.insert("stream".to_string(), json!(true));
    }
    Ok(Value::Object(body))
}

fn messages_of(payload: &TextPayload) -> Result<Vec<Value>, AppError> {
    let mut messages: Vec<Value> = Vec::new();
    for turn in &payload.turns {
        let role = match turn.role.as_str() {
            "assistant" => "assistant",
            "tool" | "user" => "user",
            other => {
                return Err(AppError::validation(format!("不支持的消息角色 {other}")));
            }
        };
        let blocks = blocks_of(turn)?;
        if blocks.is_empty() {
            continue;
        }
        if let Some(last) = messages.last_mut() {
            if last.get("role").and_then(Value::as_str) == Some(role) {
                if let Some(content) = last.get_mut("content").and_then(Value::as_array_mut) {
                    content.extend(blocks);
                    continue;
                }
            }
        }
        messages.push(json!({ "role": role, "content": blocks }));
    }
    if messages.is_empty() {
        return Err(AppError::validation("没有可发送的消息"));
    }
    Ok(messages)
}

fn blocks_of(turn: &TextTurn) -> Result<Vec<Value>, AppError> {
    let mut blocks = Vec::new();
    if turn.role == "tool" {
        let id = turn
            .tool_call_id
            .clone()
            .filter(|text| !text.is_empty())
            .ok_or_else(|| AppError::validation("工具结果缺少编号"))?;
        blocks.push(json!({
            "type": "tool_result",
            "tool_use_id": id,
            "content": turn.text,
        }));
        return Ok(blocks);
    }
    if !turn.text.trim().is_empty() {
        blocks.push(json!({ "type": "text", "text": turn.text }));
    }
    for call in &turn.tool_calls {
        let input = serde_json::from_str::<Value>(&call.arguments).unwrap_or(json!({}));
        blocks.push(json!({
            "type": "tool_use",
            "id": call.id,
            "name": call.name,
            "input": input,
        }));
    }
    for image in &turn.images {
        let (mime, bytes) = crate::adapters::data_url_parts(&image.path)?;
        blocks.push(json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": mime,
                "data": base64::engine::general_purpose::STANDARD.encode(bytes),
            }
        }));
    }
    Ok(blocks)
}

fn tool_calls_of(body: &Value) -> Vec<ToolCall> {
    body.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
        .filter_map(|block| {
            Some(ToolCall {
                id: block.get("id")?.as_str()?.to_string(),
                name: block.get("name")?.as_str()?.to_string(),
                arguments: serde_json::to_string(block.get("input").unwrap_or(&json!({}))).ok()?,
            })
        })
        .collect()
}

fn text_of(body: &Value) -> String {
    body.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("")
}

fn usage_of(body: &Value) -> Option<Usage> {
    let usage = body.get("usage")?;
    let input = usage.get("input_tokens").and_then(Value::as_u64);
    let output = usage.get("output_tokens").and_then(Value::as_u64);
    let reasoning = usage.get("thinking_tokens").and_then(Value::as_u64);
    Some(Usage {
        input_tokens: input,
        output_tokens: output,
        total_tokens: match (input, output) {
            (Some(left), Some(right)) => Some(left.saturating_add(right)),
            _ => None,
        },
        reasoning_tokens: reasoning,
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
    let mut decoder = Utf8ChunkDecoder::new();
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut usage = None;
    let mut interrupted = None;
    let mut stream = response.bytes_stream();
    let mut completed_normally = false;
    while let Some(chunk) = tokio::select! {
        _ = exchange::wait_for_cancel(&ctx.cancelled) => None,
        chunk = stream.next() => chunk,
    } {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(err) => {
                let err_msg = format!("流式响应中断: {err}");
                if text.is_empty() && reasoning.is_empty() {
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
        let piece = decoder.decode(&chunk);
        if piece.is_empty() {
            continue;
        }
        for data in parser.push(&piece) {
            let Ok(value) = serde_json::from_str::<Value>(&data) else {
                continue;
            };
            match value.get("type").and_then(Value::as_str) {
                Some("content_block_delta") => {
                    let delta = &value["delta"];
                    match delta.get("type").and_then(Value::as_str) {
                        Some("text_delta") => {
                            let bit = delta.get("text").and_then(Value::as_str).unwrap_or("");
                            if !bit.is_empty() {
                                text.push_str(bit);
                                deltas(LiveDelta::Text(bit.to_string()));
                            }
                        }
                        Some("thinking_delta") => {
                            let bit = delta.get("thinking").and_then(Value::as_str).unwrap_or("");
                            if !bit.is_empty() {
                                reasoning.push_str(bit);
                                deltas(LiveDelta::Reasoning(bit.to_string()));
                            }
                        }
                        _ => {}
                    }
                }
                Some("message_delta") => {
                    if let Some(next) = usage_of(&value) {
                        usage = Some(next);
                    }
                    if value
                        .pointer("/delta/stop_reason")
                        .and_then(Value::as_str)
                        .is_some()
                    {
                        completed_normally = true;
                    }
                }
                Some("message_stop") => {
                    completed_normally = true;
                }
                Some("error") => {
                    let message = value
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("Anthropic 流式响应失败");
                    if text.is_empty() && reasoning.is_empty() {
                        return Err(AppError::provider(
                            message,
                            status,
                            &ctx.provider.name,
                            Some(&model.name),
                            Some(shrink_json(&value)),
                        ));
                    }
                    interrupted = Some(message.to_string());
                    break;
                }
                _ => {}
            }
        }
        if interrupted.is_some() {
            break;
        }
    }
    let stopped = ctx.cancelled.load(Ordering::Relaxed);
    if !stopped && interrupted.is_none() {
        let mut trailing = decoder.finish();
        trailing.push_str("\n\n");
        for data in parser.push(&trailing) {
            let Ok(value) = serde_json::from_str::<Value>(&data) else {
                continue;
            };
            match value.get("type").and_then(Value::as_str) {
                Some("content_block_delta") => {
                    let delta = &value["delta"];
                    match delta.get("type").and_then(Value::as_str) {
                        Some("text_delta") => {
                            let bit = delta.get("text").and_then(Value::as_str).unwrap_or("");
                            if !bit.is_empty() {
                                text.push_str(bit);
                                deltas(LiveDelta::Text(bit.to_string()));
                            }
                        }
                        Some("thinking_delta") => {
                            let bit = delta.get("thinking").and_then(Value::as_str).unwrap_or("");
                            if !bit.is_empty() {
                                reasoning.push_str(bit);
                                deltas(LiveDelta::Reasoning(bit.to_string()));
                            }
                        }
                        _ => {}
                    }
                }
                Some("message_delta") => {
                    if let Some(next) = usage_of(&value) {
                        usage = Some(next);
                    }
                    if value
                        .pointer("/delta/stop_reason")
                        .and_then(Value::as_str)
                        .is_some()
                    {
                        completed_normally = true;
                    }
                }
                Some("message_stop") => completed_normally = true,
                Some("error") => {
                    interrupted = value
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                }
                _ => {}
            }
        }
    }
    let interrupted = if let Some(msg) = interrupted {
        Some(msg)
    } else if !completed_normally && !stopped {
        if text.is_empty() && reasoning.is_empty() {
            return Err(AppError::network(
                "流式响应异常中断 (未收到 message_stop 或结束标志)",
                &ctx.provider.name,
                Some(&model.name),
            ));
        } else {
            Some("流式响应异常中断 (未收到 message_stop 或结束标志)".to_string())
        }
    } else {
        None
    };

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
        reasoning,
        interrupted,
        tool_calls: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Modality;

    fn model() -> ModelDefinition {
        ModelDefinition {
            id: "model-id".to_string(),
            name: "Model".to_string(),
            provider_id: "provider-id".to_string(),
            model_id: "claude-test".to_string(),
            modality: Modality::Text,
            capabilities: Value::Null,
            parameters: Vec::new(),
            constraints: Vec::new(),
            transport: None,
            schema_version: 1,
            enabled: true,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn payload(extra: Option<Value>) -> TextPayload {
        TextPayload {
            system_prompt: None,
            turns: vec![TextTurn::plain("user", "ping", Vec::new())],
            parameters: Map::new(),
            extra,
            tools: Vec::new(),
        }
    }

    #[test]
    fn includes_extra_fields_and_rejects_protocol_overwrites() {
        let body = build_body(
            &model(),
            &payload(Some(json!({ "temperature": 0.2 }))),
            false,
        )
        .unwrap();
        assert_eq!(body["temperature"], json!(0.2));

        let error =
            build_body(&model(), &payload(Some(json!({ "messages": [] }))), false).unwrap_err();
        assert_eq!(error.code, crate::error::VALIDATION);
    }
}
