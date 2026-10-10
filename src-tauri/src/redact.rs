use std::collections::BTreeMap;

use serde_json::Value;

pub fn is_sensitive_key(name: &str) -> bool {
    let key = name.trim().to_ascii_lowercase().replace('-', "_");
    matches!(
        key.as_str(),
        "authorization"
            | "proxy_authorization"
            | "api_key"
            | "apikey"
            | "x_api_key"
            | "api_token"
            | "access_token"
            | "token"
            | "secret"
            | "password"
            | "passwd"
            | "signature"
            | "sig"
            | "cookie"
            | "cookie2"
            | "set_cookie"
    ) || key.ends_with("_token")
        || key.ends_with("_secret")
        || key.ends_with("_password")
        || key.ends_with("_api_key")
        || key.ends_with("_credential")
        || key.ends_with("_signature")
        || key.ends_with("_sig")
}

pub fn mask_secret(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= 8 {
        return "••••".to_string();
    }
    let prefix: String = trimmed.chars().take(4).collect();
    format!("{prefix}••••")
}

pub fn mask_header_value(value: &str) -> String {
    let mut parts = value.split_whitespace();
    let scheme = parts.next();
    let token = parts.next();
    let rest = parts.next();
    if rest.is_none() {
        if let (Some(scheme), Some(token)) = (scheme, token) {
            if scheme.eq_ignore_ascii_case("bearer") {
                return format!("Bearer {}", mask_secret(token));
            }
        }
    }
    mask_secret(value)
}

pub fn redact_header_map(headers: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    headers
        .iter()
        .map(|(key, value)| {
            if is_sensitive_key(key) {
                (key.clone(), mask_header_value(value))
            } else {
                (key.clone(), value.clone())
            }
        })
        .collect()
}

pub fn redact_url(input: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(input) else {
        if let Some((base, _)) = input.split_once('?') {
            return format!("{base}?[redacted]");
        }
        return input.to_string();
    };
    if !url.username().is_empty() {
        let _ = url.set_username("");
    }
    let _ = url.set_password(None);
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(key, value)| {
            if is_sensitive_key(&key) {
                (key.into_owned(), mask_secret(&value))
            } else {
                (key.into_owned(), value.into_owned())
            }
        })
        .collect();
    if pairs.is_empty() {
        url.set_query(None);
        return url.to_string();
    }
    {
        let mut query = url.query_pairs_mut();
        query.clear();
        for (key, value) in &pairs {
            query.append_pair(key, value);
        }
    }
    url.to_string()
}

pub fn shrink_json(value: &Value) -> Value {
    redact_json(&truncate_long_strings(value, 64))
}

fn truncate_long_strings(value: &Value, limit: usize) -> Value {
    match value {
        Value::String(text) if text.chars().count() > limit => {
            let head: String = text.chars().take(16).collect();
            Value::String(format!("{head}…"))
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| truncate_long_strings(item, limit))
                .collect(),
        ),
        Value::Object(map) => {
            let mut next = serde_json::Map::new();
            for (key, item) in map {
                next.insert(key.clone(), truncate_long_strings(item, limit));
            }
            Value::Object(next)
        }
        other => other.clone(),
    }
}

pub fn redact_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut redacted = serde_json::Map::new();
            for (key, item) in map {
                if is_sensitive_key(key) {
                    redacted.insert(key.clone(), Value::String(mask_json_secret(item)));
                } else {
                    redacted.insert(key.clone(), redact_json(item));
                }
            }
            Value::Object(redacted)
        }
        Value::Array(items) => Value::Array(items.iter().map(redact_json).collect()),
        Value::String(text) => Value::String(mask_embedded(text)),
        other => other.clone(),
    }
}

fn mask_json_secret(value: &Value) -> String {
    match value {
        Value::String(text) => mask_header_value(text),
        other => mask_secret(&other.to_string()),
    }
}

pub fn mask_embedded(text: &str) -> String {
    let mut result = text.to_string();

    // 1. 截断包含的 data URL (例如 base64 图片/音频)
    if let Some(pos) = result.find("data:") {
        if let Some(semi) = result[pos..].find(";base64,") {
            let base64_start = pos + semi + 8;
            if let Some(end_offset) = result[base64_start..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ')')
            {
                let end = base64_start + end_offset;
                if end - base64_start > 32 {
                    result.replace_range(base64_start..end, "•••");
                }
            } else if result.len() - base64_start > 32 {
                result.truncate(base64_start);
                result.push_str("•••");
            }
        }
    }

    // 2. 脱敏 Bearer token (句首或句中)
    let mut search_idx = 0;
    while search_idx < result.len() {
        if let Some(pos) = find_ascii_case_insensitive(&result, search_idx, "bearer ") {
            let abs_pos = pos + 7;
            let token_part = &result[abs_pos..];
            let token_len = token_part
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == '}')
                .unwrap_or(token_part.len());
            if token_len > 4 {
                let secret = &result[abs_pos..abs_pos + token_len];
                let masked = mask_secret(secret);
                result.replace_range(abs_pos..abs_pos + token_len, &masked);
                search_idx = abs_pos + masked.len();
            } else {
                search_idx = abs_pos + token_len.max(1);
            }
        } else {
            break;
        }
    }

    // 3. 脱敏形如 sk-xxx / sk_xxx / key-xxx 的嵌入式 API 密钥
    for prefix in &["sk-", "sk_", "key-"] {
        let mut idx = 0;
        while idx < result.len() {
            if let Some(pos) = result[idx..].find(prefix) {
                let abs_pos = idx + pos;
                let token_part = &result[abs_pos..];
                let token_len = token_part
                    .find(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                    .unwrap_or(token_part.len());
                if token_len > 8 {
                    let secret = &result[abs_pos..abs_pos + token_len];
                    let masked = mask_secret(secret);
                    result.replace_range(abs_pos..abs_pos + token_len, &masked);
                    idx = abs_pos + masked.len();
                } else {
                    idx = abs_pos + token_len.max(1);
                }
            } else {
                break;
            }
        }
    }

    // 4. 脱敏以 HTTP Header 形式出现的凭据和 Cookie。
    for (header_name, is_auth_header) in [
        ("authorization:", true),
        ("proxy-authorization:", false),
        ("x-api-key:", false),
        ("x-goog-api-key:", false),
        ("api-key:", false),
        ("api_token:", false),
        ("access_token:", false),
        ("cookie:", false),
        ("cookie2:", false),
        ("set-cookie:", false),
        ("password:", false),
        ("passwd:", false),
        ("signature:", false),
    ] {
        let mut idx = 0;
        while idx < result.len() {
            if let Some(pos) = find_ascii_case_insensitive(&result, idx, header_name) {
                let value_start = pos + header_name.len();
                let remaining = &result[value_start..];
                let leading_ws = remaining
                    .chars()
                    .take_while(|ch| ch.is_whitespace() && *ch != '\r' && *ch != '\n')
                    .map(char::len_utf8)
                    .sum::<usize>();
                let secret_start = value_start + leading_ws;
                let secret_len = result[secret_start..]
                    .find(['\r', '\n', '"', '\'', '}'])
                    .unwrap_or(result.len() - secret_start);
                if secret_len > 0 {
                    let secret = &result[secret_start..secret_start + secret_len];
                    let masked = if is_auth_header {
                        mask_header_value(secret)
                    } else {
                        mask_secret(secret)
                    };
                    result.replace_range(secret_start..secret_start + secret_len, &masked);
                    idx = secret_start + masked.len();
                } else {
                    idx = secret_start + 1;
                }
            } else {
                break;
            }
        }
    }

    // 5. 脱敏嵌入式 URL 查询参数中的敏感 key/token
    for param_name in &[
        "key=",
        "api_key=",
        "apikey=",
        "x-api-key=",
        "x_api_key=",
        "x-goog-api-key=",
        "api-token=",
        "api_token=",
        "token=",
        "access_token=",
        "proxy-authorization=",
        "secret=",
        "password=",
        "passwd=",
        "signature=",
        "sig=",
        "cookie=",
        "cookie2=",
        "set-cookie=",
    ] {
        let mut idx = 0;
        while idx < result.len() {
            if let Some(pos) = find_ascii_case_insensitive(&result, idx, param_name) {
                let abs_pos = pos + param_name.len();
                let token_part = &result[abs_pos..];
                let token_len = token_part
                    .find(|c: char| {
                        c.is_whitespace()
                            || c == '&'
                            || c == '"'
                            || c == '\''
                            || c == '#'
                            || c == '}'
                    })
                    .unwrap_or(token_part.len());
                if token_len > 4 {
                    let secret = &result[abs_pos..abs_pos + token_len];
                    let masked = mask_secret(secret);
                    result.replace_range(abs_pos..abs_pos + token_len, &masked);
                    idx = abs_pos + masked.len();
                } else {
                    idx = abs_pos + token_len.max(1);
                }
            } else {
                break;
            }
        }
    }

    result
}

/// Find an ASCII token without creating a lower-cased copy whose byte offsets
/// can differ from the original UTF-8 string (for example, `İ` expands when
/// lower-cased). All offsets returned by this helper are valid UTF-8 slices.
fn find_ascii_case_insensitive(haystack: &str, start: usize, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(start.min(haystack.len()));
    }
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    if start > haystack.len() || needle.len() > haystack.len().saturating_sub(start) {
        return None;
    }
    (start..=haystack.len() - needle.len()).find(|&offset| {
        haystack[offset..offset + needle.len()]
            .iter()
            .zip(needle)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
    })
}

pub fn mask_with_known_secrets(text: &str, known_secrets: &[&str]) -> String {
    let mut result = mask_embedded(text);
    for secret in known_secrets {
        let s = secret.trim();
        if s.len() >= 6 && result.contains(s) {
            result = result.replace(s, &mask_secret(s));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn masks_bearer_headers_query_and_embedded_keys() {
        let mut headers = BTreeMap::new();
        headers.insert(
            "Authorization".to_string(),
            "Bearer sk-live-secret-value".to_string(),
        );
        headers.insert("X-Title".to_string(), "workbench".to_string());
        let redacted = redact_header_map(&headers);
        assert_eq!(
            redacted.get("Authorization").map(String::as_str),
            Some("Bearer sk-l••••")
        );
        assert_eq!(
            redacted.get("X-Title").map(String::as_str),
            Some("workbench")
        );

        let url = redact_url("https://example.test/v1/chat?api_key=sk-live-secret&model=demo");
        assert!(!url.contains("sk-live-secret"));
        assert!(url.contains("model=demo"));

        let body = redact_json(&json!({
            "authorization": "sk-live-secret-value",
            "temperature": 0.2,
            "note": "Bearer sk-live-secret-value"
        }));
        let serialized = body.to_string();
        assert!(!serialized.contains("sk-live-secret-value"));
        assert!(serialized.contains("0.2"));
    }

    #[test]
    fn masks_embedded_tokens_after_unicode_without_panicking() {
        let redacted = mask_embedded("İBearer sk-live-secret-value");
        assert!(!redacted.contains("sk-live-secret-value"));
        assert!(redacted.starts_with("İBearer"));

        let redacted = mask_embedded("前缀 İ api_key=sk-live-secret-value");
        assert!(!redacted.contains("sk-live-secret-value"));
    }

    #[test]
    fn masks_cookie_and_signed_url_credentials() {
        let mut headers = BTreeMap::new();
        headers.insert(
            "Cookie".to_string(),
            "session=long-private-cookie".to_string(),
        );
        headers.insert(
            "Set-Cookie".to_string(),
            "sid=long-private-cookie".to_string(),
        );
        let redacted_headers = redact_header_map(&headers);
        assert!(!redacted_headers
            .values()
            .any(|value| value.contains("long-private-cookie")));

        let url = redact_url(
            "https://cdn.example.test/file?X-Amz-Credential=AKIA_PRIVATE&X-Amz-Signature=abcdef0123456789&part=1",
        );
        assert!(!url.contains("AKIA_PRIVATE"));
        assert!(!url.contains("abcdef0123456789"));
        assert!(url.contains("part=1"));
    }

    #[test]
    fn masks_credentials_in_plain_debug_text() {
        let text = "Authorization: Bearer private-token\r\nCookie: session=private-cookie; refresh=private-refresh\r\nx-goog-api-key: google-secret\r\nsignature=private-signature";
        let redacted = mask_embedded(text);
        for secret in [
            "private-token",
            "private-cookie",
            "private-refresh",
            "google-secret",
            "private-signature",
        ] {
            assert!(!redacted.contains(secret), "debug text leaked {secret}");
        }
    }
}
