use std::time::Instant;

use reqwest::{Client, Proxy};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::domain::now_iso;
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySettings {
    pub mode: String, // "system" | "none" | "http" | "socks5"
    pub http_proxy: Option<String>,
    pub socks5_proxy: Option<String>,
    pub updated_at: String,
}

fn validate_proxy_url(
    field: &str,
    value: Option<&str>,
    schemes: &[&str],
) -> Result<Option<String>, AppError> {
    let Some(raw) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let url = reqwest::Url::parse(raw)
        .map_err(|err| AppError::validation(format!("无效 {field} 代理地址: {err}")))?;
    if !schemes
        .iter()
        .any(|scheme| url.scheme().eq_ignore_ascii_case(scheme))
    {
        return Err(AppError::validation(format!(
            "{field} 代理地址必须使用 {} 协议",
            schemes.join("/"),
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::validation(format!(
            "{field} 代理地址不能包含用户名或密码"
        )));
    }
    if url.host_str().is_none() {
        return Err(AppError::validation(format!("{field} 代理地址缺少主机")));
    }
    Ok(Some(raw.to_string()))
}

pub fn same_origin(left: &reqwest::Url, right: &reqwest::Url) -> bool {
    left.scheme().eq_ignore_ascii_case(right.scheme())
        && left.host() == right.host()
        && left.port_or_known_default() == right.port_or_known_default()
}

/// Follow redirects only while they stay on the origin that issued the request.
///
/// Provider requests carry credentials in custom headers such as `x-api-key` and
/// `x-goog-api-key`. Reqwest's default redirect handling does not reliably strip
/// those headers on a cross-origin redirect, so stopping such redirects is the
/// safe default for the shared client.
pub fn same_origin_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt
            .previous()
            .last()
            .is_some_and(|previous| same_origin(previous, attempt.url()))
        {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            mode: "system".to_string(),
            http_proxy: None,
            socks5_proxy: None,
            updated_at: now_iso(),
        }
    }
}

pub async fn get_proxy_settings(pool: &SqlitePool) -> Result<ProxySettings, AppError> {
    let row = sqlx::query("SELECT value_json FROM app_settings WHERE key = 'network_proxy'")
        .fetch_optional(pool)
        .await
        .map_err(AppError::db)?;

    if let Some(r) = row {
        let val_str: String = sqlx::Row::get(&r, 0);
        let mut settings: ProxySettings = serde_json::from_str(&val_str)
            .map_err(|e| AppError::storage(format!("解析网络代理设置失败: {}", e)))?;
        settings.http_proxy =
            validate_proxy_url("HTTP", settings.http_proxy.as_deref(), &["http", "https"])?;
        settings.socks5_proxy = validate_proxy_url(
            "SOCKS5",
            settings.socks5_proxy.as_deref(),
            &["socks5", "socks5h"],
        )?;
        Ok(settings)
    } else {
        Ok(ProxySettings::default())
    }
}

pub async fn set_proxy_settings(
    pool: &SqlitePool,
    mut settings: ProxySettings,
) -> Result<ProxySettings, AppError> {
    let mode = settings.mode.to_lowercase();
    if mode != "system" && mode != "none" && mode != "http" && mode != "socks5" {
        return Err(AppError::validation(
            "代理模式必须为 system, none, http 或 socks5",
        ));
    }
    settings.mode = mode;
    settings.http_proxy =
        validate_proxy_url("HTTP", settings.http_proxy.as_deref(), &["http", "https"])?;
    settings.socks5_proxy = validate_proxy_url(
        "SOCKS5",
        settings.socks5_proxy.as_deref(),
        &["socks5", "socks5h"],
    )?;
    settings.updated_at = now_iso();

    let val_json = serde_json::to_string(&settings)
        .map_err(|e| AppError::validation(format!("序列化代理设置失败: {}", e)))?;

    sqlx::query(
        "INSERT INTO app_settings (key, value_json, updated_at)
         VALUES ('network_proxy', ?, ?)
         ON CONFLICT(key) DO UPDATE SET
           value_json = excluded.value_json,
           updated_at = excluded.updated_at",
    )
    .bind(&val_json)
    .bind(&settings.updated_at)
    .execute(pool)
    .await
    .map_err(AppError::db)?;

    Ok(settings)
}

pub fn build_client_with_proxy(settings: &ProxySettings) -> Result<Client, AppError> {
    let http_proxy =
        validate_proxy_url("HTTP", settings.http_proxy.as_deref(), &["http", "https"])?;
    let socks5_proxy = validate_proxy_url(
        "SOCKS5",
        settings.socks5_proxy.as_deref(),
        &["socks5", "socks5h"],
    )?;
    let mut builder = Client::builder()
        .user_agent("ai-workbench/0.1")
        .redirect(same_origin_redirect_policy())
        .connect_timeout(std::time::Duration::from_secs(10))
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .tcp_keepalive(std::time::Duration::from_secs(60));

    match settings.mode.as_str() {
        "none" => {
            builder = builder.no_proxy();
        }
        "http" => {
            if let Some(url) = http_proxy {
                if !url.is_empty() {
                    let p = Proxy::all(&url)
                        .map_err(|e| AppError::validation(format!("无效 HTTP 代理地址: {}", e)))?;
                    builder = builder.proxy(p);
                }
            }
        }
        "socks5" => {
            if let Some(url) = socks5_proxy {
                if !url.is_empty() {
                    let p = Proxy::all(&url).map_err(|e| {
                        AppError::validation(format!("无效 SOCKS5 代理地址: {}", e))
                    })?;
                    builder = builder.proxy(p);
                }
            }
        }
        _ => {
            // "system" - reqwest uses system proxy by default
        }
    }

    builder
        .build()
        .map_err(|e| AppError::network(e.to_string(), "proxy", None))
}

pub async fn test_proxy_connection(settings: &ProxySettings) -> Result<u64, AppError> {
    let client = build_client_with_proxy(settings)?;
    let start = Instant::now();

    let resp = client
        .get("https://httpbin.org/ip")
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| AppError::network(format!("代理连接测试失败: {}", e), "proxy", None))?;

    if resp.status().is_success() {
        Ok(start.elapsed().as_millis() as u64)
    } else {
        Err(AppError::network(
            format!("代理连接测试返回 HTTP 状态码: {}", resp.status().as_u16()),
            "proxy",
            None,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_origin_comparison_rejects_cross_origin_and_userinfo_is_rejected() {
        let origin = reqwest::Url::parse("https://api.example.test/v1").unwrap();
        let same = reqwest::Url::parse("https://api.example.test/v2").unwrap();
        let different_host = reqwest::Url::parse("https://cdn.example.test/v2").unwrap();
        let different_scheme = reqwest::Url::parse("http://api.example.test/v2").unwrap();
        assert!(same_origin(&origin, &same));
        assert!(!same_origin(&origin, &different_host));
        assert!(!same_origin(&origin, &different_scheme));

        let err = validate_proxy_url(
            "HTTP",
            Some("https://user:password@proxy.example.test:8443"),
            &["http", "https"],
        )
        .unwrap_err();
        assert!(err.to_string().contains("用户名或密码"));
    }

    #[test]
    fn proxy_scheme_must_match_the_selected_proxy_kind() {
        assert!(
            validate_proxy_url("HTTP", Some("socks5://127.0.0.1:1080"), &["http", "https"])
                .is_err()
        );
        assert!(validate_proxy_url(
            "SOCKS5",
            Some("http://127.0.0.1:8080"),
            &["socks5", "socks5h"]
        )
        .is_err());
        assert!(validate_proxy_url(
            "SOCKS5",
            Some("socks5h://127.0.0.1:1080"),
            &["socks5", "socks5h"]
        )
        .is_ok());
    }

    #[tokio::test]
    async fn cross_origin_redirect_is_stopped_before_provider_headers_can_leak() {
        use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

        let source = MockServer::start().await;
        let target = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/stolen", target.uri())),
            )
            .mount(&source)
            .await;

        let client = Client::builder()
            .redirect(same_origin_redirect_policy())
            .build()
            .unwrap();
        let response = client
            .get(format!("{}/start", source.uri()))
            .header("x-api-key", "provider-secret")
            .send()
            .await;
        assert_eq!(response.unwrap().status(), reqwest::StatusCode::FOUND);
        let target_requests = tokio::time::timeout(
            std::time::Duration::from_millis(250),
            target.received_requests(),
        )
        .await;
        assert!(target_requests.is_err() || target_requests.unwrap().unwrap().is_empty());
    }
}
