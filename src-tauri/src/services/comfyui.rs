use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::credentials::SecretStore;
use crate::db;
use crate::domain::ProviderType;
use crate::error::AppError;
use crate::http_util::join_url;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComfyQueueItem {
    pub prompt_id: String,
    pub number: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComfyQueueStatus {
    pub running: Vec<ComfyQueueItem>,
    pub pending: Vec<ComfyQueueItem>,
    pub total: usize,
}

fn apply_comfy_headers(
    mut builder: reqwest::RequestBuilder,
    provider: &crate::domain::ProviderRecord,
    api_key: Option<&str>,
) -> reqwest::RequestBuilder {
    for (k, v) in &provider.headers {
        builder = builder.header(k, v);
    }
    if let Some(key) = api_key {
        let key = key.trim();
        if !key.is_empty()
            && !provider
                .headers
                .keys()
                .any(|k| k.eq_ignore_ascii_case("authorization"))
        {
            builder = builder.header("Authorization", format!("Bearer {key}"));
        }
    }
    builder
}

pub async fn get_queue(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    provider_id: &str,
) -> Result<ComfyQueueStatus, AppError> {
    let provider = db::get_provider(pool, provider_id).await?;
    if provider.provider_type != ProviderType::Comfyui {
        return Err(AppError::validation("所选 Provider 不是 ComfyUI"));
    }
    let secret = match provider.credential_id.as_deref() {
        Some(cid) => secrets.get(cid)?,
        None => None,
    };
    let api_key = secret.as_ref().map(|s| s.expose());

    let url = join_url(&provider.base_url, "queue")?;
    let req = apply_comfy_headers(http.get(url), &provider, api_key);
    let resp = req.send().await.map_err(|err| {
        AppError::network(
            format!("无法连接 ComfyUI 队列: {err}"),
            &provider.name,
            None,
        )
    })?;
    let body: Value = resp.json().await.map_err(|err| {
        AppError::provider(
            format!("解析 ComfyUI 队列响应失败: {err}"),
            200,
            &provider.name,
            None,
            None,
        )
    })?;

    let mut running = Vec::new();
    let mut pending = Vec::new();

    if let Some(list) = body.get("queue_running").and_then(Value::as_array) {
        for item in list {
            if let Some(arr) = item.as_array() {
                let number = arr.first().and_then(Value::as_u64).unwrap_or(0);
                let prompt_id = arr.get(1).and_then(Value::as_str).unwrap_or("").to_string();
                if !prompt_id.is_empty() {
                    running.push(ComfyQueueItem { prompt_id, number });
                }
            }
        }
    }

    if let Some(list) = body.get("queue_pending").and_then(Value::as_array) {
        for item in list {
            if let Some(arr) = item.as_array() {
                let number = arr.first().and_then(Value::as_u64).unwrap_or(0);
                let prompt_id = arr.get(1).and_then(Value::as_str).unwrap_or("").to_string();
                if !prompt_id.is_empty() {
                    pending.push(ComfyQueueItem { prompt_id, number });
                }
            }
        }
    }

    let total = running.len() + pending.len();
    Ok(ComfyQueueStatus {
        running,
        pending,
        total,
    })
}

pub async fn interrupt(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    provider_id: &str,
) -> Result<bool, AppError> {
    let provider = db::get_provider(pool, provider_id).await?;
    if provider.provider_type != ProviderType::Comfyui {
        return Err(AppError::validation("所选 Provider 不是 ComfyUI"));
    }
    let secret = match provider.credential_id.as_deref() {
        Some(cid) => secrets.get(cid)?,
        None => None,
    };
    let api_key = secret.as_ref().map(|s| s.expose());

    let url = join_url(&provider.base_url, "interrupt")?;
    let req = apply_comfy_headers(http.post(url), &provider, api_key);
    let resp = req.json(&json!({})).send().await.map_err(|err| {
        AppError::network(
            format!("中断 ComfyUI 任务失败: {err}"),
            &provider.name,
            None,
        )
    })?;
    Ok(resp.status().is_success())
}

pub async fn clear_queue(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    provider_id: &str,
) -> Result<bool, AppError> {
    let provider = db::get_provider(pool, provider_id).await?;
    if provider.provider_type != ProviderType::Comfyui {
        return Err(AppError::validation("所选 Provider 不是 ComfyUI"));
    }
    let secret = match provider.credential_id.as_deref() {
        Some(cid) => secrets.get(cid)?,
        None => None,
    };
    let api_key = secret.as_ref().map(|s| s.expose());

    let url = join_url(&provider.base_url, "queue")?;
    let req = apply_comfy_headers(http.post(url), &provider, api_key);
    let resp = req
        .json(&json!({ "clear": true }))
        .send()
        .await
        .map_err(|err| {
            AppError::network(
                format!("清空 ComfyUI 队列失败: {err}"),
                &provider.name,
                None,
            )
        })?;
    Ok(resp.status().is_success())
}
