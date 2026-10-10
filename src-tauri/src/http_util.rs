use std::time::Duration;

use futures_util::StreamExt;
use serde_json::Value;

use crate::error::{AppError, AUTH, INVALID_REQUEST, PROVIDER, RATE_LIMIT, TIMEOUT};
use crate::redact::{mask_embedded, redact_json};

/// Keep provider responses bounded before converting them into an owned
/// `String`/`Vec<u8>`. This protects JSON and error paths from unbounded body
/// allocations while leaving media downloads to their larger, explicit limit.
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub enum BoundedFileReadError {
    Io(std::io::Error),
    TooLarge,
}

pub fn read_file_bounded_sync(
    path: &std::path::Path,
    limit: usize,
) -> Result<Vec<u8>, BoundedFileReadError> {
    use std::io::Read;

    let metadata = std::fs::metadata(path).map_err(BoundedFileReadError::Io)?;
    if !metadata.is_file() {
        return Err(BoundedFileReadError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        )));
    }
    if metadata.len() > limit as u64 {
        return Err(BoundedFileReadError::TooLarge);
    }

    let file = std::fs::File::open(path).map_err(BoundedFileReadError::Io)?;
    let capacity = usize::try_from(metadata.len()).unwrap_or(limit).min(limit);
    let mut bytes = Vec::with_capacity(capacity);
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(BoundedFileReadError::Io)?;
    if bytes.len() > limit {
        return Err(BoundedFileReadError::TooLarge);
    }
    Ok(bytes)
}

/// Read at most `limit + 1` bytes so an oversized or concurrently growing
/// file cannot force an unbounded allocation before the size check.
pub async fn read_file_bounded(
    path: &std::path::Path,
    limit: usize,
) -> Result<Vec<u8>, BoundedFileReadError> {
    use tokio::io::AsyncReadExt;

    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(BoundedFileReadError::Io)?;
    if !metadata.is_file() {
        return Err(BoundedFileReadError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        )));
    }
    if metadata.len() > limit as u64 {
        return Err(BoundedFileReadError::TooLarge);
    }

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(BoundedFileReadError::Io)?;
    let capacity = usize::try_from(metadata.len()).unwrap_or(limit).min(limit);
    let mut bytes = Vec::with_capacity(capacity);
    let mut reader = (&mut file).take(limit as u64 + 1);
    reader
        .read_to_end(&mut bytes)
        .await
        .map_err(BoundedFileReadError::Io)?;
    if bytes.len() > limit {
        return Err(BoundedFileReadError::TooLarge);
    }
    Ok(bytes)
}

pub async fn read_body(
    response: reqwest::Response,
    provider: &str,
    model: Option<&str>,
) -> Result<Vec<u8>, AppError> {
    read_body_with_limit(response, MAX_RESPONSE_BYTES, provider, model).await
}

pub async fn read_body_with_limit(
    response: reqwest::Response,
    limit: usize,
    provider: &str,
    model: Option<&str>,
) -> Result<Vec<u8>, AppError> {
    if let Some(content_length) = response.content_length() {
        if content_length > limit as u64 {
            return Err(AppError::provider(
                format!("Provider 响应过大，超过 {}MB 上限", limit / (1024 * 1024)),
                200,
                provider,
                model,
                None,
            ));
        }
    }
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| map_reqwest(err, provider, model))?;
        if body.len() + chunk.len() > limit {
            return Err(AppError::provider(
                format!("Provider 响应过大，超过 {}MB 上限", limit / (1024 * 1024)),
                200,
                provider,
                model,
                None,
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub fn join_url(base: &str, path: &str) -> Result<String, AppError> {
    let base = base.trim().trim_end_matches('/');
    let path = path.trim().trim_start_matches('/');
    if base.is_empty() || path.is_empty() {
        return Err(AppError::validation("请求地址不完整"));
    }
    let url = format!("{base}/{path}");
    reqwest::Url::parse(&url).map_err(|_| AppError::validation("请求地址不合法"))?;
    Ok(url)
}

pub async fn send(
    builder: reqwest::RequestBuilder,
    timeout: Duration,
    provider: &str,
    model: Option<&str>,
) -> Result<reqwest::Response, AppError> {
    builder
        .timeout(timeout)
        .send()
        .await
        .map_err(|err| map_reqwest(err, provider, model))
}

pub async fn read_response(
    response: reqwest::Response,
    provider: &str,
    model: Option<&str>,
) -> Result<(u16, Value), AppError> {
    read_response_with_limit(response, MAX_RESPONSE_BYTES, provider, model).await
}

pub async fn read_response_with_limit(
    response: reqwest::Response,
    limit: usize,
    provider: &str,
    model: Option<&str>,
) -> Result<(u16, Value), AppError> {
    let status = response.status().as_u16();
    let body = read_body_with_limit(response, limit, provider, model).await?;
    let text = String::from_utf8_lossy(&body);
    if !(200..300).contains(&status) {
        return Err(error_from_status(status, &text, provider, model));
    }
    if text.trim_start().starts_with("data:") {
        return Err(AppError::provider(
            "服务器返回了流式响应，当前生成使用非流式接口",
            status,
            provider,
            model,
            None,
        ));
    }
    let value = serde_json::from_str(&text).map_err(|_| {
        AppError::provider(
            "响应不是 JSON",
            status,
            provider,
            model,
            Some(Value::String(text.chars().take(2000).collect())),
        )
    })?;
    Ok((status, value))
}

pub fn error_from_status(status: u16, body: &str, provider: &str, model: Option<&str>) -> AppError {
    let message = mask_embedded(&extract_message(body));
    let raw = raw_from_body(body);
    let (code, fallback) = match status {
        401 | 403 => (AUTH, "认证失败"),
        408 => (TIMEOUT, "请求超时"),
        429 => (RATE_LIMIT, "请求频率超过限制"),
        400 | 404 | 409 | 422 => (INVALID_REQUEST, "请求无效"),
        _ => (PROVIDER, "Provider 返回错误"),
    };
    let message = if message.trim().is_empty() {
        fallback.to_string()
    } else {
        message
    };
    AppError::status(code, message, status, provider, model, raw)
}

fn extract_message(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
            return message.chars().take(400).collect();
        }
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            return message.chars().take(400).collect();
        }
        if let Some(message) = value.get("error").and_then(Value::as_str) {
            return message.chars().take(400).collect();
        }
    }
    body.trim().chars().take(300).collect()
}

fn raw_from_body(body: &str) -> Option<Value> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    let value = serde_json::from_str::<Value>(trimmed)
        .unwrap_or_else(|_| Value::String(trimmed.chars().take(2000).collect()));
    Some(redact_json(&value))
}

pub(crate) fn map_reqwest(err: reqwest::Error, provider: &str, model: Option<&str>) -> AppError {
    if err.is_timeout() {
        AppError::timeout(provider, model)
    } else if err.is_connect() {
        AppError::network(format!("无法连接 {provider}"), provider, model)
    } else {
        // reqwest's Display implementation may include the complete request
        // URL. Keep it out of both the user-facing error and exchange logs;
        // provider URLs can contain API keys or signed query parameters.
        AppError::network("网络请求失败", provider, model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bounded_file_read_rejects_oversized_files_before_loading_them() {
        let path = std::env::temp_dir().join(format!(
            "ai-workbench-bounded-read-{}.bin",
            uuid::Uuid::new_v4()
        ));
        tokio::fs::write(&path, b"12345").await.unwrap();

        let result = read_file_bounded(&path, 4).await;
        tokio::fs::remove_file(&path).await.unwrap();

        assert!(matches!(result, Err(BoundedFileReadError::TooLarge)));
    }

    #[test]
    fn bounded_sync_file_read_rejects_oversized_files() {
        let path = std::env::temp_dir().join(format!(
            "ai-workbench-bounded-sync-read-{}.bin",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&path, b"12345").unwrap();

        let result = read_file_bounded_sync(&path, 4);
        std::fs::remove_file(&path).unwrap();

        assert!(matches!(result, Err(BoundedFileReadError::TooLarge)));
    }
}
