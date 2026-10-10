use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::credentials::SecretStore;
use crate::db;
use crate::domain::now_iso;
use crate::error::AppError;
use crate::redact::{is_sensitive_key, mask_embedded, redact_header_map, redact_json, redact_url};
use crate::services::network;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTestPreset {
    pub id: String,
    pub name: String,
    pub method: String,
    pub path: String,
    pub provider_id: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevRequestInput {
    pub method: String,
    pub url: String,
    pub provider_id: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SseEventItem {
    pub index: usize,
    pub event_type: String,
    pub data: String,
    pub offset_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevResponseOutput {
    pub status: u16,
    pub status_text: String,
    pub headers: BTreeMap<String, String>,
    pub body: String,
    pub latency_ms: u64,
    pub size_bytes: usize,
    pub curl_command: String,
    pub is_sse: bool,
    pub sse_events: Vec<SseEventItem>,
}

pub async fn send_request(
    http: &Client,
    secrets: &dyn SecretStore,
    pool: &SqlitePool,
    input: DevRequestInput,
) -> Result<DevResponseOutput, AppError> {
    let mut final_url = input.url.trim().to_string();
    let mut headers = input.headers.clone();

    // 1. If provider_id is provided, resolve base_url and credentials
    let mut auth_token: Option<String> = None;
    let mut provider_type = None;
    if let Some(ref pid) = input.provider_id {
        if !pid.trim().is_empty() {
            let provider = db::get_provider(pool, pid).await?;
            let base_url = reqwest::Url::parse(provider.base_url.trim())
                .map_err(|err| AppError::validation(format!("Provider Base URL 无效: {err}")))?;
            provider_type = Some(provider.provider_type);
            // If url is relative path, join with provider.base_url
            if final_url.starts_with('/') {
                let base = provider.base_url.trim_end_matches('/');
                final_url = format!("{}{}", base, final_url);
            }
            let request_url = reqwest::Url::parse(&final_url)
                .map_err(|err| AppError::validation(format!("请求 URL 无效: {err}")))?;
            // A provider credential may only be used with that provider's origin.
            // Otherwise an arbitrary absolute URL could exfiltrate the key.
            if !network::same_origin(&base_url, &request_url) {
                return Err(AppError::validation(
                    "带 Provider 凭据的请求必须使用 Provider 的同源地址",
                ));
            }
            // Inherit provider default headers
            for (k, v) in provider.headers {
                headers.entry(k).or_insert(v);
            }
            // Fetch credentials only after the target origin has been checked.
            if let Some(ref cred_id) = provider.credential_id {
                if let Some(secret) = secrets.get(cred_id)? {
                    auth_token = Some(secret.expose().to_string());
                }
            }
        }
    }

    // Inject only the authentication header expected by this provider.
    if let Some(token) = auth_token.as_ref() {
        let (header_name, header_value) = match provider_type {
            Some(crate::domain::ProviderType::Anthropic) => ("x-api-key", token.clone()),
            Some(crate::domain::ProviderType::Gemini) => ("x-goog-api-key", token.clone()),
            Some(crate::domain::ProviderType::Fal) => ("Authorization", format!("Key {token}")),
            Some(crate::domain::ProviderType::Replicate)
            | Some(crate::domain::ProviderType::Openai)
            | Some(crate::domain::ProviderType::OpenaiCompatible) => {
                ("Authorization", format!("Bearer {token}"))
            }
            _ => ("", String::new()),
        };
        if !header_name.is_empty()
            && !headers
                .keys()
                .any(|key| key.eq_ignore_ascii_case(header_name))
        {
            headers.insert(header_name.to_string(), header_value);
        }
    }

    // Ensure default Content-Type for POST/PUT if body is present
    if (input.method == "POST" || input.method == "PUT")
        && input.body.is_some()
        && !headers.contains_key("Content-Type")
        && !headers.contains_key("content-type")
    {
        headers.insert("Content-Type".to_string(), "application/json".to_string());
    }

    // 2. Generate safe cURL string
    let curl_command = generate_curl(&input.method, &final_url, &headers, input.body.as_deref());

    // 3. Build request
    let method = Method::from_bytes(input.method.as_bytes())
        .map_err(|e| AppError::validation(format!("不支持的 HTTP 方法: {e}")))?;

    let mut req_builder = http.request(method, &final_url);
    for (k, v) in &headers {
        req_builder = req_builder.header(k, v);
    }
    if let Some(ref body_str) = input.body {
        req_builder = req_builder.body(body_str.clone());
    }

    let timeout = Duration::from_millis(input.timeout_ms.unwrap_or(60_000));
    req_builder = req_builder.timeout(timeout);

    // 4. Send request and measure latency
    let start_time = Instant::now();
    let resp = req_builder
        .send()
        .await
        .map_err(|err| crate::http_util::map_reqwest(err, "Developer", None))?;

    let latency_ms = start_time.elapsed().as_millis() as u64;
    let status = resp.status().as_u16();
    let status_text = resp
        .status()
        .canonical_reason()
        .unwrap_or("Unknown")
        .to_string();

    let mut resp_headers = BTreeMap::new();
    for (k, v) in resp.headers() {
        if let Ok(v_str) = v.to_str() {
            let value = if k.as_str().eq_ignore_ascii_case("location") {
                redact_url(v_str)
            } else {
                v_str.to_string()
            };
            resp_headers.insert(k.as_str().to_string(), value);
        }
    }
    resp_headers = redact_header_map(&resp_headers);

    let content_type = resp_headers
        .get("content-type")
        .or_else(|| resp_headers.get("Content-Type"))
        .cloned()
        .unwrap_or_default()
        .to_lowercase();

    let (raw_bytes, size_bytes, truncated) = read_response_preview(resp).await?;
    let body_text = if truncated {
        format!(
            "{}\n\n[Warning: 响应体超过 5MB，已截断显示前 5MB 内容；实际大小至少 {} bytes]",
            sanitize_debug_text(&String::from_utf8_lossy(&raw_bytes)),
            size_bytes
        )
    } else {
        sanitize_debug_text(&String::from_utf8_lossy(&raw_bytes))
    };

    let is_sse = content_type.contains("text/event-stream")
        || body_text.lines().any(|l| l.trim().starts_with("data:"));

    // 5. If SSE, parse chunks into items
    let mut sse_events = Vec::new();
    if is_sse {
        let mut cur_event = String::new();
        let mut idx = 1;
        for line in body_text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("event:") {
                cur_event = trimmed
                    .strip_prefix("event:")
                    .unwrap_or("")
                    .trim()
                    .to_string();
            } else if trimmed.starts_with("data:") {
                let data_part = trimmed
                    .strip_prefix("data:")
                    .unwrap_or("")
                    .trim()
                    .to_string();
                sse_events.push(SseEventItem {
                    index: idx,
                    event_type: if cur_event.is_empty() {
                        "message".to_string()
                    } else {
                        cur_event.clone()
                    },
                    data: data_part,
                    offset_ms: latency_ms,
                });
                idx += 1;
                cur_event.clear();
            }
        }
    }

    Ok(DevResponseOutput {
        status,
        status_text,
        headers: resp_headers,
        body: body_text,
        latency_ms,
        size_bytes,
        curl_command,
        is_sse,
        sse_events,
    })
}

const MAX_RESPONSE_PREVIEW_BYTES: usize = 5 * 1024 * 1024;

async fn read_response_preview(
    response: reqwest::Response,
) -> Result<(Vec<u8>, usize, bool), AppError> {
    let declared_size = response
        .content_length()
        .map(|size| usize::try_from(size).unwrap_or(usize::MAX));
    let mut stream = response.bytes_stream();
    let mut preview = Vec::with_capacity(
        declared_size
            .unwrap_or(MAX_RESPONSE_PREVIEW_BYTES)
            .min(MAX_RESPONSE_PREVIEW_BYTES),
    );
    let mut total = 0u64;
    let mut truncated = false;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| crate::http_util::map_reqwest(err, "Developer", None))?;
        total = total.saturating_add(chunk.len() as u64);
        if preview.len() < MAX_RESPONSE_PREVIEW_BYTES {
            let remaining = MAX_RESPONSE_PREVIEW_BYTES - preview.len();
            preview.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        }
        if total > MAX_RESPONSE_PREVIEW_BYTES as u64
            || (declared_size.is_some_and(|size| size > MAX_RESPONSE_PREVIEW_BYTES)
                && total >= MAX_RESPONSE_PREVIEW_BYTES as u64)
        {
            truncated = true;
            break;
        }
    }

    let observed_size = usize::try_from(total).unwrap_or(usize::MAX);
    let size = declared_size
        .filter(|declared| *declared >= observed_size)
        .unwrap_or(observed_size);
    Ok((preview, size, truncated))
}

fn generate_curl(
    method: &str,
    url: &str,
    headers: &BTreeMap<String, String>,
    body: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    parts.push(format!("curl -X {}", method));
    parts.push(format!("\"{}\"", shell_quote_contents(&redact_url(url))));
    for (k, v) in redact_header_map(headers) {
        parts.push(format!(
            "-H \"{}: {}\"",
            shell_quote_contents(&k),
            shell_quote_contents(&v)
        ));
    }
    if let Some(b) = body {
        if !b.trim().is_empty() {
            let safe_body = sanitize_debug_text(b);
            parts.push(format!("-d \"{}\"", shell_quote_contents(&safe_body)));
        }
    }
    parts.join(" \\\n  ")
}

fn shell_quote_contents(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

fn sanitize_debug_text(text: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        redact_json(&value).to_string()
    } else if text
        .lines()
        .any(|line| line.trim_start().starts_with("data:"))
    {
        text.lines()
            .map(|line| {
                let Some((prefix, data)) = line.split_once("data:") else {
                    return mask_embedded(line);
                };
                match serde_json::from_str::<serde_json::Value>(data.trim()) {
                    Ok(value) => format!("{prefix}data: {}", redact_json(&value)),
                    Err(_) => mask_embedded(line),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        mask_embedded(text)
    }
}

pub async fn list_presets(pool: &SqlitePool) -> Result<Vec<ApiTestPreset>, AppError> {
    let rows = sqlx::query(
        "SELECT id, name, method, path, provider_id, headers_json, body_json, created_at, updated_at
         FROM api_test_presets ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(AppError::db)?;

    let mut list = Vec::new();
    for row in rows {
        let headers_json: String = row.try_get("headers_json").map_err(AppError::db)?;
        let headers = serde_json::from_str(&headers_json).unwrap_or_default();
        let mut preset = ApiTestPreset {
            id: row.try_get("id").map_err(AppError::db)?,
            name: row.try_get("name").map_err(AppError::db)?,
            method: row.try_get("method").map_err(AppError::db)?,
            path: row.try_get("path").map_err(AppError::db)?,
            provider_id: row.try_get("provider_id").ok(),
            headers,
            body: row.try_get("body_json").map_err(AppError::db)?,
            created_at: row.try_get("created_at").map_err(AppError::db)?,
            updated_at: row.try_get("updated_at").map_err(AppError::db)?,
        };
        sanitize_preset_secrets(&mut preset)?;
        list.push(preset);
    }

    if list.is_empty() {
        return Ok(default_presets());
    }

    Ok(list)
}

pub async fn save_preset(
    pool: &SqlitePool,
    mut preset: ApiTestPreset,
) -> Result<ApiTestPreset, AppError> {
    if preset.name.trim().is_empty() {
        return Err(AppError::validation("预设名称不能为空"));
    }

    let now = now_iso();
    let is_new = preset.id.trim().is_empty();
    if is_new {
        preset.id = Uuid::new_v4().to_string();
        preset.created_at = now.clone();
    }
    preset.updated_at = now;
    sanitize_preset_secrets(&mut preset)?;

    let headers_json = serde_json::to_string(&preset.headers)
        .map_err(|e| AppError::validation(format!("Headers 序列化失败: {e}")))?;

    if is_new {
        sqlx::query(
            "INSERT INTO api_test_presets (id, name, method, path, provider_id, headers_json, body_json, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&preset.id)
        .bind(&preset.name)
        .bind(&preset.method)
        .bind(&preset.path)
        .bind(&preset.provider_id)
        .bind(&headers_json)
        .bind(&preset.body)
        .bind(&preset.created_at)
        .bind(&preset.updated_at)
        .execute(pool)
        .await
        .map_err(AppError::db)?;
    } else {
        sqlx::query(
            "UPDATE api_test_presets
             SET name = ?, method = ?, path = ?, provider_id = ?, headers_json = ?, body_json = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&preset.name)
        .bind(&preset.method)
        .bind(&preset.path)
        .bind(&preset.provider_id)
        .bind(&headers_json)
        .bind(&preset.body)
        .bind(&preset.updated_at)
        .bind(&preset.id)
        .execute(pool)
        .await
        .map_err(AppError::db)?;
    }

    Ok(preset)
}

fn safe_secret_placeholder(value: &str) -> bool {
    let value = value.trim();
    value == "{{apiKey}}"
        || ["Bearer", "Key"]
            .iter()
            .any(|scheme| value.eq_ignore_ascii_case(&format!("{scheme} {{{{apiKey}}}}")))
}

fn redact_preset_secret(value: &str) -> String {
    let value = value.trim();
    if safe_secret_placeholder(value) {
        return value.to_string();
    }
    for scheme in ["Bearer", "Key"] {
        if value
            .get(..scheme.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
            && value[scheme.len()..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
        {
            return format!("{scheme} {{{{apiKey}}}}");
        }
    }
    "{{apiKey}}".to_string()
}

fn sanitize_json_secrets(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if is_sensitive_key(key) {
                    *child = serde_json::Value::String(match child {
                        serde_json::Value::String(text) => redact_preset_secret(text),
                        _ => "{{apiKey}}".to_string(),
                    });
                } else {
                    sanitize_json_secrets(child);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                sanitize_json_secrets(child);
            }
        }
        serde_json::Value::String(text) => *text = mask_embedded(text),
        _ => {}
    }
}

fn sanitize_preset_secrets(preset: &mut ApiTestPreset) -> Result<(), AppError> {
    for (key, value) in &mut preset.headers {
        if is_sensitive_key(key) {
            *value = redact_preset_secret(value);
        }
    }
    if preset.body.trim().is_empty() {
        return Ok(());
    }
    if let Ok(mut body) = serde_json::from_str::<serde_json::Value>(&preset.body) {
        sanitize_json_secrets(&mut body);
        preset.body = serde_json::to_string(&body)
            .map_err(|err| AppError::validation(format!("请求体序列化失败: {err}")))?;
    } else {
        preset.body = mask_embedded(&preset.body);
    }
    Ok(())
}

pub async fn delete_preset(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM api_test_presets WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

pub fn default_presets() -> Vec<ApiTestPreset> {
    vec![
        ApiTestPreset {
            id: "preset_chat_completion".to_string(),
            name: "💬 OpenAI 聊天补全 (Chat Completions)".to_string(),
            method: "POST".to_string(),
            path: "/v1/chat/completions".to_string(),
            provider_id: None,
            headers: BTreeMap::new(),
            body: serde_json::json!({
                "model": "gpt-4o",
                "messages": [
                    { "role": "system", "content": "You are a helpful assistant." },
                    { "role": "user", "content": "Hello, write a 1-sentence greeting!" }
                ],
                "temperature": 0.7,
                "stream": false
            })
            .to_string(),
            created_at: "".to_string(),
            updated_at: "".to_string(),
        },
        ApiTestPreset {
            id: "preset_anthropic_messages".to_string(),
            name: "🧠 Anthropic 消息接口 (Messages API)".to_string(),
            method: "POST".to_string(),
            path: "/v1/messages".to_string(),
            provider_id: None,
            headers: BTreeMap::from_iter([
                ("anthropic-version".to_string(), "2023-06-01".to_string()),
            ]),
            body: serde_json::json!({
                "model": "claude-3-5-sonnet-20241022",
                "max_tokens": 1024,
                "messages": [
                    { "role": "user", "content": "Explain quantum computing in one short sentence." }
                ]
            })
            .to_string(),
            created_at: "".to_string(),
            updated_at: "".to_string(),
        },
        ApiTestPreset {
            id: "preset_image_generation".to_string(),
            name: "🖼️ 图像生成 (Images Generations)".to_string(),
            method: "POST".to_string(),
            path: "/v1/images/generations".to_string(),
            provider_id: None,
            headers: BTreeMap::new(),
            body: serde_json::json!({
                "model": "dall-e-3",
                "prompt": "A serene floating garden in the clouds, digital art",
                "size": "1024x1024",
                "n": 1
            })
            .to_string(),
            created_at: "".to_string(),
            updated_at: "".to_string(),
        },
        ApiTestPreset {
            id: "preset_list_models".to_string(),
            name: "📋 查询可用模型列表 (List Models)".to_string(),
            method: "GET".to_string(),
            path: "/v1/models".to_string(),
            provider_id: None,
            headers: BTreeMap::new(),
            body: "".to_string(),
            created_at: "".to_string(),
            updated_at: "".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_sanitization_never_persists_literal_credentials() {
        let mut preset = ApiTestPreset {
            id: "preset".to_string(),
            name: "test".to_string(),
            method: "POST".to_string(),
            path: "/v1/chat/completions".to_string(),
            provider_id: None,
            headers: BTreeMap::from([
                (
                    "Authorization".to_string(),
                    "Bearer real-secret".to_string(),
                ),
                ("x-api-key".to_string(), "another-secret".to_string()),
                ("Content-Type".to_string(), "application/json".to_string()),
            ]),
            body: r#"{"api_key":"body-secret","messages":[{"role":"user","content":"hi"}]}"#
                .to_string(),
            created_at: String::new(),
            updated_at: String::new(),
        };

        sanitize_preset_secrets(&mut preset).unwrap();
        assert_eq!(preset.headers["Authorization"], "Bearer {{apiKey}}");
        assert_eq!(preset.headers["x-api-key"], "{{apiKey}}");
        assert!(!preset.body.contains("body-secret"));
        assert!(preset.body.contains("{{apiKey}}"));
        assert_eq!(preset.headers["Content-Type"], "application/json");
    }

    #[test]
    fn preset_sanitization_preserves_safe_auth_placeholders() {
        assert_eq!(
            redact_preset_secret("Bearer {{apiKey}}"),
            "Bearer {{apiKey}}"
        );
        assert_eq!(redact_preset_secret("Key {{apiKey}}"), "Key {{apiKey}}");
        assert_eq!(redact_preset_secret("{{apiKey}}"), "{{apiKey}}");
    }

    #[test]
    fn preset_sanitization_masks_credentials_in_plain_text_bodies() {
        let mut preset = ApiTestPreset {
            id: "preset".to_string(),
            name: "test".to_string(),
            method: "POST".to_string(),
            path: "/".to_string(),
            provider_id: None,
            headers: BTreeMap::new(),
            body: "Cookie: session=private-cookie\nAuthorization: Bearer private-token".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
        };

        sanitize_preset_secrets(&mut preset).unwrap();

        assert!(!preset.body.contains("private-cookie"));
        assert!(!preset.body.contains("private-token"));
    }

    #[test]
    fn debug_output_redacts_credentials_from_url_headers_and_json_body() {
        let headers = BTreeMap::from([
            (
                "Authorization".to_string(),
                "Bearer very-secret-token".to_string(),
            ),
            ("Cookie".to_string(), "session=private-cookie".to_string()),
            (
                "x-goog-api-key".to_string(),
                "google-private-key".to_string(),
            ),
        ]);
        let curl = generate_curl(
            "POST",
            "https://example.test/path?api_key=url-private-key",
            &headers,
            Some(r#"{"access_token":"body-private-token","prompt":"hello"}"#),
        );

        for secret in [
            "very-secret-token",
            "private-cookie",
            "google-private-key",
            "url-private-key",
            "body-private-token",
        ] {
            assert!(!curl.contains(secret), "curl output leaked {secret}");
        }
        assert!(curl.contains("hello"));
    }

    #[test]
    fn debug_output_redacts_json_and_sse_response_secrets() {
        let json =
            sanitize_debug_text(r#"{"access_token":"response-private-token","message":"hello"}"#);
        assert!(!json.contains("response-private-token"));
        assert!(json.contains("hello"));

        let sse = sanitize_debug_text(
            "event: result\ndata: {\"api_key\":\"sse-private-key\",\"text\":\"done\"}\n\n",
        );
        assert!(!sse.contains("sse-private-key"));
        assert!(sse.contains("done"));
    }

    #[test]
    fn debug_redaction_handles_unicode_auth_values_without_panicking() {
        let mut preset = ApiTestPreset {
            id: "preset".to_string(),
            name: "test".to_string(),
            method: "GET".to_string(),
            path: "/".to_string(),
            provider_id: None,
            headers: BTreeMap::from([(
                "Authorization".to_string(),
                "密钥 Bearer secret-value".to_string(),
            )]),
            body: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        sanitize_preset_secrets(&mut preset).unwrap();
        assert_eq!(preset.headers["Authorization"], "{{apiKey}}");
    }

    #[tokio::test]
    async fn response_preview_stops_at_the_configured_limit() {
        use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = vec![b'x'; MAX_RESPONSE_PREVIEW_BYTES + 16 * 1024];
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .mount(&server)
            .await;

        let response = Client::new().get(server.uri()).send().await.unwrap();
        let (preview, size, truncated) = read_response_preview(response).await.unwrap();

        assert_eq!(preview.len(), MAX_RESPONSE_PREVIEW_BYTES);
        assert!(size > MAX_RESPONSE_PREVIEW_BYTES);
        assert!(truncated);
    }
}
