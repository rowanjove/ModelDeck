use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{json, Value};

use crate::canonical::AdapterContext;
use crate::error::AppError;
use crate::http_util;
use crate::redact::{is_sensitive_key, redact_header_map, redact_url, shrink_json};

#[allow(dead_code)]
pub fn raw_request(
    method: &str,
    url: &str,
    headers: &BTreeMap<String, String>,
    body: &Value,
) -> Value {
    raw_request_with_id(method, url, headers, body, None)
}

pub fn raw_request_with_id(
    method: &str,
    url: &str,
    headers: &BTreeMap<String, String>,
    body: &Value,
    request_id: Option<&str>,
) -> Value {
    let mut obj = serde_json::Map::new();
    if let Some(rid) = request_id {
        obj.insert("requestId".to_string(), json!(rid));
    }
    obj.insert("method".to_string(), json!(method));
    obj.insert("url".to_string(), json!(redact_url(url)));
    obj.insert("headers".to_string(), json!(redact_header_map(headers)));
    obj.insert("body".to_string(), shrink_json(body));
    Value::Object(obj)
}

pub(crate) async fn wait_for_cancel(flag: &std::sync::Arc<std::sync::atomic::AtomicBool>) {
    while !flag.load(std::sync::atomic::Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Credentials may be sent to a generated media URL only when it points to
/// the exact provider origin. Matching just the hostname would also match a
/// different scheme or port on the same host.
pub(crate) fn same_origin(base: &str, target: &str) -> bool {
    let Ok(base) = reqwest::Url::parse(base) else {
        return false;
    };
    let Ok(target) = reqwest::Url::parse(target) else {
        return false;
    };
    base.scheme() == target.scheme()
        && base.host() == target.host()
        && base.port_or_known_default() == target.port_or_known_default()
}

/// Return the headers that are safe to send to a provider-generated URL.
///
/// Providers commonly return polling/result URLs in the response. Those URLs
/// are data from the remote provider and must not be allowed to exfiltrate the
/// API key if they point at another origin. Non-sensitive headers (for example
/// Accept or Content-Type) remain useful for cross-origin signed media URLs,
/// while all credential-shaped headers are removed.
pub(crate) fn headers_for_url(
    base: &str,
    target: &str,
    headers: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    if same_origin(base, target) {
        return headers.clone();
    }
    headers
        .iter()
        .filter(|(key, _)| !is_sensitive_key(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

pub async fn send(
    ctx: &AdapterContext,
    model_name: Option<&str>,
    method: &str,
    url: &str,
    headers: &BTreeMap<String, String>,
    body: Option<&Value>,
) -> Result<(Value, reqwest::Response), AppError> {
    let payload = body.cloned().unwrap_or(Value::Null);
    let safe_headers = headers_for_url(&ctx.provider.base_url, url, headers);
    let raw = raw_request_with_id(method, url, &safe_headers, &payload, Some(&ctx.request_id));

    // 结构化日志: 记录请求发起
    if let Some(ref logger) = ctx.logs {
        logger.info(
            "exchange",
            Some(&ctx.request_id),
            ctx.task_id.as_deref(),
            Some(&ctx.provider.id),
            ctx.model.as_ref().map(|m| m.id.as_str()),
            "http_request_start",
            &format!("发送 {method} 请求至 {}", redact_url(url)),
        );
    }

    let mut attempt = 0;
    loop {
        if ctx.cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(ref logger) = ctx.logs {
                logger.warn(
                    "exchange",
                    Some(&ctx.request_id),
                    ctx.task_id.as_deref(),
                    Some(&ctx.provider.id),
                    ctx.model.as_ref().map(|m| m.id.as_str()),
                    "request_cancelled",
                    "任务已取消，终止请求",
                );
            }
            return Err(AppError::validation("任务已取消"));
        }

        // 断路器检查：若该 Provider 已熔断则直接拦截
        if let Some(ref breakers) = ctx.breakers {
            if let Err(e) = breakers.check_permission(&ctx.provider.id, &ctx.provider.name) {
                if let Some(ref logger) = ctx.logs {
                    logger.warn(
                        "exchange",
                        Some(&ctx.request_id),
                        ctx.task_id.as_deref(),
                        Some(&ctx.provider.id),
                        ctx.model.as_ref().map(|m| m.id.as_str()),
                        "circuit_breaker_blocked",
                        &e.message,
                    );
                }
                return Err(e);
            }
        }

        let mut builder = match method {
            "GET" => ctx.http.get(url),
            "POST" => match body {
                Some(value) => ctx.http.post(url).json(value),
                None => ctx.http.post(url),
            },
            _ => return Err(AppError::validation("请求方法无效")),
        };
        for (key, value) in &safe_headers {
            builder = builder.header(key, value);
        }
        // Correlation ID: 注入 X-Request-ID 请求头
        builder = builder.header("X-Request-ID", &ctx.request_id);

        let send_fut = http_util::send(
            builder,
            Duration::from_millis(ctx.provider.timeout_ms),
            &ctx.provider.name,
            model_name,
        );

        let res = tokio::select! {
            _ = wait_for_cancel(&ctx.cancelled) => {
                if let Some(ref logger) = ctx.logs {
                    logger.warn(
                        "exchange",
                        Some(&ctx.request_id),
                        ctx.task_id.as_deref(),
                        Some(&ctx.provider.id),
                        ctx.model.as_ref().map(|m| m.id.as_str()),
                        "request_cancelled",
                        "等待网络响应过程中任务被取消，立即终止",
                    );
                }
                return Err(AppError::validation("任务已取消"));
            }
            r = send_fut => r,
        };

        match res {
            Ok(response) => {
                let status = response.status().as_u16();
                // 仅对 GET 请求或 429 频率限制（服务端尚未执行）进行重试
                // 非 GET 请求（如 POST 生成）遭遇 5xx 服务端错误坚决不自动重试，避免重复生成与二次扣费
                let can_retry_status = if method == "GET" {
                    crate::resilience::is_retryable_status(status)
                } else {
                    status == 429
                };

                if can_retry_status && attempt < crate::resilience::DEFAULT_MAX_RETRIES {
                    attempt += 1;
                    let delay = crate::resilience::backoff_delay(attempt);
                    if let Some(ref logger) = ctx.logs {
                        logger.warn(
                            "exchange",
                            Some(&ctx.request_id),
                            ctx.task_id.as_deref(),
                            Some(&ctx.provider.id),
                            ctx.model.as_ref().map(|m| m.id.as_str()),
                            "http_retry",
                            &format!(
                                "接收到可重试状态码 {status}，将在 {}ms 后进行第 {attempt} 次重试",
                                delay.as_millis()
                            ),
                        );
                    }
                    tokio::select! {
                        _ = wait_for_cancel(&ctx.cancelled) => {
                            return Err(AppError::validation("任务已取消"));
                        }
                        _ = tokio::time::sleep(delay) => {}
                    }
                    continue;
                }

                if (200..300).contains(&status) {
                    if let Some(ref breakers) = ctx.breakers {
                        breakers.record_result(&ctx.provider.id, true);
                    }
                    if let Some(ref logger) = ctx.logs {
                        logger.info(
                            "exchange",
                            Some(&ctx.request_id),
                            ctx.task_id.as_deref(),
                            Some(&ctx.provider.id),
                            ctx.model.as_ref().map(|m| m.id.as_str()),
                            "http_request_success",
                            &format!("请求成功 (HTTP {status})"),
                        );
                    }
                } else if status >= 500 {
                    if let Some(ref breakers) = ctx.breakers {
                        breakers.record_result(&ctx.provider.id, false);
                    }
                    if let Some(ref logger) = ctx.logs {
                        logger.error(
                            "exchange",
                            Some(&ctx.request_id),
                            ctx.task_id.as_deref(),
                            Some(&ctx.provider.id),
                            ctx.model.as_ref().map(|m| m.id.as_str()),
                            "http_request_server_error",
                            &format!("Provider 服务端异常 (HTTP {status})"),
                        );
                    }
                }

                return Ok((raw, response));
            }
            Err(err) => {
                // 非 GET 请求（如 POST 生成）在网络断开或超时时不自动重试，因为远端可能已在执行
                let can_retry_err = method == "GET" && crate::resilience::is_retryable(&err);
                if can_retry_err && attempt < crate::resilience::DEFAULT_MAX_RETRIES {
                    attempt += 1;
                    let delay = crate::resilience::backoff_delay(attempt);
                    if let Some(ref logger) = ctx.logs {
                        logger.warn(
                            "exchange",
                            Some(&ctx.request_id),
                            ctx.task_id.as_deref(),
                            Some(&ctx.provider.id),
                            ctx.model.as_ref().map(|m| m.id.as_str()),
                            "http_retry",
                            &format!(
                                "遇到可重试网络/超时错误（{}），将在 {}ms 后进行第 {attempt} 次重试",
                                err.message,
                                delay.as_millis()
                            ),
                        );
                    }
                    tokio::select! {
                        _ = wait_for_cancel(&ctx.cancelled) => {
                            return Err(AppError::validation("任务已取消"));
                        }
                        _ = tokio::time::sleep(delay) => {}
                    }
                    continue;
                }

                if let Some(ref breakers) = ctx.breakers {
                    breakers.record_result(&ctx.provider.id, false);
                }
                if let Some(ref logger) = ctx.logs {
                    logger.error(
                        "exchange",
                        Some(&ctx.request_id),
                        ctx.task_id.as_deref(),
                        Some(&ctx.provider.id),
                        ctx.model.as_ref().map(|m| m.id.as_str()),
                        "http_request_failed",
                        &err.message,
                    );
                }
                return Err(err);
            }
        }
    }
}

pub async fn download(
    ctx: &AdapterContext,
    model_name: &str,
    url: &str,
    max_bytes: usize,
) -> Result<Vec<u8>, AppError> {
    let send_fut = http_util::send(
        ctx.http.get(url),
        Duration::from_millis(ctx.provider.timeout_ms),
        &ctx.provider.name,
        Some(model_name),
    );

    let response = tokio::select! {
        _ = wait_for_cancel(&ctx.cancelled) => {
            return Err(AppError::validation("任务已取消"));
        }
        res = send_fut => res?,
    };

    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let text = String::from_utf8_lossy(
            &http_util::read_body(response, &ctx.provider.name, Some(model_name)).await?,
        )
        .into_owned();
        return Err(http_util::error_from_status(
            status,
            &text,
            &ctx.provider.name,
            Some(model_name),
        ));
    }
    if let Some(content_length) = response.content_length() {
        if content_length > max_bytes as u64 {
            return Err(AppError::validation(format!(
                "下载内容超过 {}MB 上限",
                max_bytes / (1024 * 1024)
            )));
        }
    }
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk_res) = tokio::select! {
        _ = wait_for_cancel(&ctx.cancelled) => {
            return Err(AppError::validation("任务已取消"));
        }
        chunk = stream.next() => chunk,
    } {
        let chunk = chunk_res.map_err(|err| {
            crate::http_util::map_reqwest(err, &ctx.provider.name, Some(model_name))
        })?;
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(AppError::validation(format!(
                "下载内容超过 {}MB 上限",
                max_bytes / (1024 * 1024)
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub fn require_key(ctx: &AdapterContext) -> Result<&crate::credentials::SecretString, AppError> {
    ctx.api_key
        .as_ref()
        .ok_or_else(|| AppError::auth("缺少 API Key", &ctx.provider.name, None))
}

pub fn elapsed_ms(started: std::time::Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_credentials_from_cross_origin_provider_urls() {
        let headers = BTreeMap::from([
            ("Authorization".to_string(), "Bearer secret".to_string()),
            ("x-goog-api-key".to_string(), "secret".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
        ]);

        let safe = headers_for_url(
            "https://provider.example/api",
            "https://cdn.example/result.json",
            &headers,
        );

        assert_eq!(
            safe.get("Accept").map(String::as_str),
            Some("application/json")
        );
        assert!(!safe.contains_key("Authorization"));
        assert!(!safe.contains_key("x-goog-api-key"));
    }

    #[test]
    fn preserves_credentials_for_the_exact_provider_origin() {
        let headers = BTreeMap::from([("Authorization".to_string(), "Bearer secret".to_string())]);

        let safe = headers_for_url(
            "https://provider.example/api",
            "https://provider.example/result.json",
            &headers,
        );

        assert_eq!(safe, headers);
    }

    #[test]
    fn raw_request_redacts_sensitive_query_parameters() {
        let raw = raw_request(
            "GET",
            "https://provider.example/result?api_key=live-secret&format=json",
            &BTreeMap::new(),
            &Value::Null,
        );
        let url = raw.get("url").and_then(Value::as_str).unwrap_or_default();
        assert!(!url.contains("live-secret"));
        assert!(url.contains("format=json"));
    }
}
