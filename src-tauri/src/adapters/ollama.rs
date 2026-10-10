use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::openai::{data_url_parts, request_headers};
use super::ProviderAdapter;
use crate::canonical::{
    tool_schema, AdapterContext, CanonicalRequest, CanonicalResponse, ConnectionReport, LiveDelta,
    RemoteModel, TextPayload, TextResult, TextTurn, ToolCall, Usage,
};
use crate::error::AppError;
use crate::http_util::{self, join_url};
use crate::redact::{mask_embedded, redact_json};
use crate::sse::{LineParser, Utf8ChunkDecoder};
use async_trait::async_trait;
use base64::Engine;
use futures_util::StreamExt;
use serde_json::{json, Map, Value};

pub struct OllamaAdapter;

pub const OLLAMA: OllamaAdapter = OllamaAdapter;

#[async_trait]
impl ProviderAdapter for OllamaAdapter {
    async fn test_connection(&self, ctx: &AdapterContext) -> Result<ConnectionReport, AppError> {
        let started = Instant::now();
        let url = join_url(&ctx.provider.base_url, "api/tags")?;
        let response = http_util::send(
            ctx.http.get(url),
            Duration::from_millis(ctx.provider.timeout_ms),
            &ctx.provider.name,
            None,
        )
        .await?;
        let status = response.status().as_u16();
        let body = String::from_utf8_lossy(
            &http_util::read_body(response, &ctx.provider.name, None).await?,
        )
        .into_owned();
        if !(200..300).contains(&status) {
            return Err(http_util::error_from_status(
                status,
                &body,
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
        let url = join_url(&ctx.provider.base_url, "api/tags")?;
        let response = http_util::send(
            ctx.http.get(url),
            Duration::from_millis(ctx.provider.timeout_ms),
            &ctx.provider.name,
            None,
        )
        .await?;
        let (_status, body) = http_util::read_response(response, &ctx.provider.name, None).await?;
        Ok(read_models(&body))
    }

    async fn execute(
        &self,
        ctx: &AdapterContext,
        request: CanonicalRequest,
    ) -> Result<CanonicalResponse, AppError> {
        let CanonicalRequest::Text(payload) = request else {
            return Err(AppError::unsupported("Ollama 适配器目前只执行文本生成"));
        };
        let model = ctx
            .model
            .as_ref()
            .ok_or_else(|| AppError::validation("缺少模型定义"))?;
        let started = Instant::now();
        let (_body, raw_request, response) = post_chat(ctx, &payload, false).await?;
        let (status, response_body) =
            http_util::read_response(response, &ctx.provider.name, Some(&model.name)).await?;
        let text = message_text(&response_body)
            .map_err(|err| err.context(&ctx.provider.name, Some(&model.name)))?;
        let mut result = result_from(
            ctx,
            model,
            started,
            status,
            raw_request,
            TurnText {
                text,
                reasoning: String::new(),
                usage: ollama_usage(&response_body),
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
            return Err(AppError::unsupported("Ollama 适配器目前只执行文本生成"));
        };
        let model = ctx
            .model
            .as_ref()
            .ok_or_else(|| AppError::validation("缺少模型定义"))?;
        let started = Instant::now();
        let (_body, raw_request, response) = post_chat(ctx, &payload, true).await?;
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
        let mut parser = LineParser::default();
        let mut decoder = Utf8ChunkDecoder::default();
        let mut text = String::new();
        let mut reasoning = String::new();
        let mut usage = None;
        let mut raw = Value::Null;
        let mut interrupted = None;
        let mut completed_normally = false;
        while let Some(chunk) = tokio::select! {
            _ = super::exchange::wait_for_cancel(&ctx.cancelled) => None,
            chunk = stream.next() => chunk,
        } {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(err) => {
                    let error = http_util::map_reqwest(err, &ctx.provider.name, Some(&model.name));
                    if text.is_empty() && reasoning.is_empty() {
                        return Err(error);
                    }
                    interrupted = Some(error.message);
                    break;
                }
            };
            let decoded = decoder.decode(&chunk);
            for line in parser.push(&decoded) {
                apply_line(
                    &line,
                    deltas,
                    &mut text,
                    &mut reasoning,
                    &mut usage,
                    &mut raw,
                    &mut completed_normally,
                );
            }
        }
        if !ctx.cancelled.load(Ordering::Relaxed) {
            let trailing = decoder.finish();
            for line in parser.push(&trailing) {
                apply_line(
                    &line,
                    deltas,
                    &mut text,
                    &mut reasoning,
                    &mut usage,
                    &mut raw,
                    &mut completed_normally,
                );
            }
        }
        let stopped = ctx.cancelled.load(Ordering::Relaxed);
        if !stopped {
            if let Some(line) = parser.finish() {
                apply_line(
                    &line,
                    deltas,
                    &mut text,
                    &mut reasoning,
                    &mut usage,
                    &mut raw,
                    &mut completed_normally,
                );
            }
        }
        if let Some(message) = raw.get("error").and_then(Value::as_str) {
            let message = mask_embedded(message);
            if text.is_empty() && reasoning.is_empty() {
                return Err(AppError::provider(
                    message,
                    status,
                    &ctx.provider.name,
                    Some(&model.name),
                    Some(raw),
                ));
            }
            interrupted = Some(message);
        }
        if interrupted.is_none() && !stopped && !completed_normally {
            interrupted = Some("流式响应异常中断 (未收到 done=true)".to_string());
        }
        Ok(result_from(
            ctx,
            model,
            started,
            status,
            raw_request,
            TurnText {
                text,
                reasoning,
                usage,
                raw,
                stopped,
                interrupted,
            },
        ))
    }
}

fn apply_line(
    line: &str,
    deltas: &(dyn Fn(LiveDelta) + Send + Sync),
    text: &mut String,
    reasoning: &mut String,
    usage: &mut Option<Usage>,
    raw: &mut Value,
    completed_normally: &mut bool,
) {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return;
    };
    if let Some(message) = value
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        *raw = redact_json(&value);
        let _ = message;
        return;
    }
    if let Some(piece) = value.pointer("/message/content").and_then(Value::as_str) {
        if !piece.is_empty() {
            text.push_str(piece);
            deltas(LiveDelta::Text(piece.to_string()));
        }
    }
    if let Some(piece) = value.pointer("/message/thinking").and_then(Value::as_str) {
        if !piece.is_empty() {
            reasoning.push_str(piece);
            deltas(LiveDelta::Reasoning(piece.to_string()));
        }
    }
    if value.get("done").and_then(Value::as_bool) == Some(true) {
        *completed_normally = true;
        *usage = ollama_usage(&value);
    }
    *raw = redact_json(&value);
}

pub fn build_ollama_body(
    model_id: &str,
    messages: Vec<Value>,
    parameters: &Map<String, Value>,
    extra: Option<&Value>,
    stream: bool,
) -> Result<Value, AppError> {
    let mut options = Map::new();
    let mut format = None;
    copy_options(parameters, &mut options, &mut format)?;
    if let Some(extra) = extra {
        let extra = extra
            .as_object()
            .ok_or_else(|| AppError::validation("Extra JSON 必须是对象"))?;
        copy_options(extra, &mut options, &mut format)?;
    }
    let mut body = json!({
        "model": model_id,
        "messages": messages,
        "stream": stream,
    });
    let object = body
        .as_object_mut()
        .ok_or_else(|| AppError::storage("无法构造请求"))?;
    if !options.is_empty() {
        object.insert("options".to_string(), Value::Object(options));
    }
    if let Some(format) = format {
        object.insert("format".to_string(), Value::String(format));
    }
    object.insert("stream".to_string(), Value::Bool(stream));
    Ok(body)
}

fn copy_options(
    source: &Map<String, Value>,
    options: &mut Map<String, Value>,
    format: &mut Option<String>,
) -> Result<(), AppError> {
    for (key, value) in source {
        if key == "model" || key == "messages" || key == "stream" {
            if key != "stream" {
                return Err(AppError::validation(format!("参数键 {key} 由协议保留")));
            }
            continue;
        }
        if key == "max_tokens" {
            options.insert("num_predict".to_string(), value.clone());
            continue;
        }
        if key == "response_format" {
            if let Some(kind) = value.as_str() {
                if kind == "json_object" {
                    *format = Some("json".to_string());
                }
            }
            continue;
        }
        options.insert(key.clone(), value.clone());
    }
    Ok(())
}

fn ollama_messages(payload: &TextPayload) -> Result<Vec<Value>, AppError> {
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
        messages.push(ollama_turn(turn)?);
    }
    Ok(messages)
}

fn ollama_turn(turn: &TextTurn) -> Result<Value, AppError> {
    if turn.role == "tool" {
        let name = turn
            .name
            .as_deref()
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .ok_or_else(|| AppError::validation("工具结果缺少名称"))?;
        return Ok(json!({
            "role": "tool",
            "tool_name": name,
            "content": turn.text,
        }));
    }
    if turn.role != "user" && turn.role != "assistant" {
        return Err(AppError::validation("消息角色无效"));
    }
    if turn.role == "assistant" && !turn.images.is_empty() {
        return Err(AppError::validation("助手消息不能包含图像"));
    }
    if turn.role == "assistant" && !turn.tool_calls.is_empty() {
        return Ok(json!({
            "role": "assistant",
            "content": turn.text,
            "tool_calls": ollama_calls(&turn.tool_calls),
        }));
    }
    if turn.images.is_empty() {
        return Ok(json!({ "role": turn.role, "content": turn.text }));
    }
    if turn.images.len() > 8 {
        return Err(AppError::validation("一次最多附加 8 张图像"));
    }
    let mut images = Vec::new();
    for image in &turn.images {
        let (_mime, bytes) = data_url_parts(&image.path)?;
        images.push(base64::engine::general_purpose::STANDARD.encode(bytes));
    }
    Ok(json!({ "role": turn.role, "content": turn.text, "images": images }))
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
    let mut body = build_ollama_body(
        &model.model_id,
        ollama_messages(payload)?,
        &payload.parameters,
        payload.extra.as_ref(),
        stream,
    )?;
    if !payload.tools.is_empty() {
        if let Some(object) = body.as_object_mut() {
            object.insert("tools".to_string(), tool_schema(&payload.tools));
        }
    }
    let url = join_url(&ctx.provider.base_url, "api/chat")?;
    let headers = request_headers(ctx);
    let (raw_request, response) =
        super::exchange::send(ctx, Some(&model.name), "POST", &url, &headers, Some(&body)).await?;
    Ok((body, raw_request, response))
}

fn ollama_calls(calls: &[ToolCall]) -> Vec<Value> {
    calls
        .iter()
        .map(|call| {
            let arguments = serde_json::from_str::<Value>(&call.arguments)
                .unwrap_or_else(|_| Value::String(call.arguments.clone()));
            json!({
                "function": {
                    "name": call.name,
                    "arguments": arguments,
                }
            })
        })
        .collect()
}

fn extract_tool_calls(body: &Value) -> Vec<ToolCall> {
    let Some(calls) = body
        .pointer("/message/tool_calls")
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
            Some(ToolCall {
                id: format!("call_{index}"),
                name: name.to_string(),
                arguments,
            })
        })
        .collect()
}

fn message_text(body: &Value) -> Result<String, AppError> {
    match body.pointer("/message/content") {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Null) | None => Ok(String::new()),
        _ => Err(AppError::new(crate::error::PROVIDER, "无法读取响应文本")),
    }
}

fn ollama_usage(body: &Value) -> Option<Usage> {
    let input = body.get("prompt_eval_count").and_then(Value::as_u64);
    let output = body.get("eval_count").and_then(Value::as_u64);
    if input.is_none() && output.is_none() {
        return None;
    }
    let total = match (input, output) {
        (Some(input), Some(output)) => Some(input.saturating_add(output)),
        _ => None,
    };
    Some(Usage {
        input_tokens: input,
        output_tokens: output,
        total_tokens: total,
        reasoning_tokens: None,
    })
}

fn read_models(body: &Value) -> Vec<RemoteModel> {
    let mut models: Vec<RemoteModel> = body
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|model| {
            let name = model.get("name")?.as_str()?.trim();
            if name.is_empty() {
                None
            } else {
                Some(RemoteModel {
                    id: name.to_string(),
                    name: name.to_string(),
                })
            }
        })
        .take(2000)
        .collect();
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
    models
}

struct TurnText {
    text: String,
    reasoning: String,
    usage: Option<Usage>,
    raw: Value,
    stopped: bool,
    interrupted: Option<String>,
}

fn result_from(
    ctx: &AdapterContext,
    model: &crate::domain::ModelDefinition,
    started: Instant,
    status: u16,
    raw_request: Value,
    turn: TurnText,
) -> TextResult {
    TextResult {
        text: turn.text,
        usage: turn.usage,
        latency_ms: elapsed_ms(started),
        provider_id: ctx.provider.id.clone(),
        provider_name: ctx.provider.name.clone(),
        model_definition_id: model.id.clone(),
        model_name: model.name.clone(),
        model_id: model.model_id.clone(),
        raw_request,
        raw_response: turn.raw,
        status,
        stopped: turn.stopped,
        reasoning: turn.reasoning,
        interrupted: turn.interrupted,
        tool_calls: Vec::new(),
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_openai_parameter_names_into_ollama_options() {
        let mut parameters = Map::new();
        parameters.insert("temperature".to_string(), json!(0.2));
        parameters.insert("max_tokens".to_string(), json!(32));
        parameters.insert("response_format".to_string(), json!("json_object"));
        let body = build_ollama_body(
            "llama",
            vec![json!({ "role": "user", "content": "ping" })],
            &parameters,
            Some(&json!({ "num_ctx": 1024, "stream": true })),
            true,
        )
        .unwrap();
        assert_eq!(body["stream"], true);
        assert_eq!(body["format"], "json");
        assert_eq!(body["options"]["temperature"], 0.2);
        assert_eq!(body["options"]["num_predict"], 32);
        assert_eq!(body["options"]["num_ctx"], 1024);
        assert!(body["options"].get("stream").is_none());
    }

    #[test]
    fn decodes_utf8_split_across_ndjson_chunks_and_requires_done() {
        let mut decoder = Utf8ChunkDecoder::default();
        let mut parser = LineParser::default();
        let mut text = String::new();
        let mut reasoning = String::new();
        let mut usage = None;
        let mut raw = Value::Null;
        let mut completed = false;
        let deltas = |_delta: LiveDelta| {};
        let body = r#"{"message":{"content":"你好"},"done":true}
"#;
        for chunk in body.as_bytes().chunks(2) {
            let decoded = decoder.decode(chunk);
            for line in parser.push(&decoded) {
                apply_line(
                    &line,
                    &deltas,
                    &mut text,
                    &mut reasoning,
                    &mut usage,
                    &mut raw,
                    &mut completed,
                );
            }
        }
        let trailing = decoder.finish();
        for line in parser.push(&trailing) {
            apply_line(
                &line,
                &deltas,
                &mut text,
                &mut reasoning,
                &mut usage,
                &mut raw,
                &mut completed,
            );
        }
        if let Some(line) = parser.finish() {
            apply_line(
                &line,
                &deltas,
                &mut text,
                &mut reasoning,
                &mut usage,
                &mut raw,
                &mut completed,
            );
        }
        assert_eq!(text, "你好");
        assert!(completed);
    }
}
