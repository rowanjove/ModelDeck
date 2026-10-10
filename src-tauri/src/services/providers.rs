use std::collections::BTreeMap;

use reqwest::Client;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::adapters::adapter_for;
use crate::canonical::{AdapterContext, ConnectionReport, RemoteModel};
use crate::credentials::{SecretStore, SecretString};
use crate::db;
use crate::domain::{now_iso, Provider, ProviderInput, ProviderRecord, ProviderType};
use crate::error::AppError;
use crate::redact::is_sensitive_key;
use crate::schema::validate_rel_path;

struct PreparedProvider {
    name: String,
    provider_type: ProviderType,
    base_url: String,
    api_key: Option<String>,
    clear_api_key: bool,
    headers: BTreeMap<String, String>,
    timeout_ms: u64,
    enabled: bool,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<Provider>, AppError> {
    let providers = db::list_providers(pool).await?;
    Ok(providers.iter().map(ProviderRecord::to_dto).collect())
}

pub async fn create(
    pool: &SqlitePool,
    secrets: &dyn SecretStore,
    input: ProviderInput,
) -> Result<Provider, AppError> {
    let prepared = prepare(input)?;
    let id = Uuid::new_v4().to_string();
    let now = now_iso();
    let mut credential_id = None;
    if let Some(api_key) = &prepared.api_key {
        secrets.set(&id, api_key)?;
        credential_id = Some(id.clone());
    }
    let record = ProviderRecord {
        id: id.clone(),
        name: prepared.name,
        provider_type: prepared.provider_type,
        base_url: prepared.base_url,
        credential_id,
        headers: prepared.headers,
        timeout_ms: prepared.timeout_ms,
        enabled: prepared.enabled,
        created_at: now.clone(),
        updated_at: now,
    };
    if let Err(err) = db::insert_provider(pool, &record).await {
        if record.credential_id.is_some() {
            let _ = secrets.delete(&id);
        }
        return Err(err);
    }
    Ok(record.to_dto())
}

pub async fn update(
    pool: &SqlitePool,
    secrets: &dyn SecretStore,
    id: &str,
    input: ProviderInput,
) -> Result<Provider, AppError> {
    let existing = db::get_provider(pool, id).await?;
    let prepared = prepare(input)?;
    if prepared.clear_api_key && prepared.api_key.is_some() {
        return Err(AppError::validation("不能同时移除并设置 API Key"));
    }
    let previous_secret = existing
        .credential_id
        .as_deref()
        .map(|credential_id| secrets.get(credential_id))
        .transpose()?;
    let mut credential_id = existing.credential_id.clone();
    if let Some(api_key) = &prepared.api_key {
        secrets.set(&existing.id, api_key)?;
        credential_id = Some(existing.id.clone());
    }
    let record = ProviderRecord {
        id: existing.id.clone(),
        name: prepared.name,
        provider_type: prepared.provider_type,
        base_url: prepared.base_url,
        credential_id: if prepared.clear_api_key {
            None
        } else {
            credential_id
        },
        headers: prepared.headers,
        timeout_ms: prepared.timeout_ms,
        enabled: prepared.enabled,
        created_at: existing.created_at,
        updated_at: now_iso(),
    };
    if prepared.clear_api_key {
        if let Some(credential_id) = existing.credential_id.as_deref() {
            secrets.delete(credential_id)?;
        }
    }
    if let Err(err) = db::update_provider(pool, &record).await {
        // The provider id is also the keyring account. Restore the old value
        // when a database write fails after replacing it.
        if prepared.api_key.is_some() || prepared.clear_api_key {
            if let (Some(credential_id), Some(previous)) = (
                existing.credential_id.as_deref(),
                previous_secret.as_ref().and_then(|s| s.as_ref()),
            ) {
                let _ = secrets.set(credential_id, previous.expose());
            } else if prepared.clear_api_key {
                let _ = secrets.delete(&existing.id);
            } else if let Some(previous) = previous_secret.as_ref().and_then(|s| s.as_ref()) {
                let _ = secrets.set(&existing.id, previous.expose());
            } else {
                let _ = secrets.delete(&existing.id);
            }
        }
        return Err(err);
    }
    Ok(record.to_dto())
}

pub async fn delete(
    pool: &SqlitePool,
    secrets: &dyn SecretStore,
    id: &str,
) -> Result<(), AppError> {
    let existing = db::get_provider(pool, id).await?;
    let previous_secret = existing
        .credential_id
        .as_deref()
        .map(|credential_id| secrets.get(credential_id))
        .transpose()?;
    if let Some(credential_id) = existing.credential_id.as_deref() {
        secrets.delete(credential_id)?;
    }
    if let Err(err) = db::delete_provider(pool, id).await {
        if let (Some(credential_id), Some(previous)) = (
            existing.credential_id.as_deref(),
            previous_secret.as_ref().and_then(|s| s.as_ref()),
        ) {
            let _ = secrets.set(credential_id, previous.expose());
        }
        return Err(err);
    }
    Ok(())
}

pub async fn test(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    id: &str,
) -> Result<ConnectionReport, AppError> {
    let provider = db::get_provider(pool, id).await?;
    let api_key = load_secret(secrets, provider.credential_id.as_deref())?;
    let kind = provider.provider_type;
    let ctx = AdapterContext::new(http.clone(), provider, None, api_key, None, None);
    adapter_for(kind)
        .test_connection(&ctx)
        .await
        .map_err(|err| err.annotate(&ctx.provider.name, ""))
}

pub async fn list_remote_models(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    id: &str,
    models_path: Option<String>,
) -> Result<Vec<RemoteModel>, AppError> {
    let models_path = clean_models_path(models_path)?;
    let provider = db::get_provider(pool, id).await?;
    let api_key = load_secret(secrets, provider.credential_id.as_deref())?;
    let kind = provider.provider_type;
    let ctx = AdapterContext::new(http.clone(), provider, None, api_key, None, models_path);
    adapter_for(kind)
        .list_models(&ctx)
        .await
        .map_err(|err| err.annotate(&ctx.provider.name, ""))
}

pub fn load_secret(
    secrets: &dyn SecretStore,
    credential_id: Option<&str>,
) -> Result<Option<SecretString>, AppError> {
    match credential_id {
        Some(id) => secrets.get(id),
        None => Ok(None),
    }
}

fn prepare(input: ProviderInput) -> Result<PreparedProvider, AppError> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::validation("Provider 名称需要 1 到 80 个字符"));
    }
    if !(1_000..=600_000).contains(&input.timeout_ms) {
        return Err(AppError::validation("超时时间需要在 1 到 600 秒之间"));
    }
    Ok(PreparedProvider {
        name: name.to_string(),
        provider_type: input.provider_type,
        base_url: clean_base_url(&input.base_url)?,
        api_key: clean_api_key(input.api_key)?,
        clear_api_key: input.clear_api_key,
        headers: clean_headers(input.headers)?,
        timeout_ms: input.timeout_ms,
        enabled: input.enabled,
    })
}

pub(crate) fn clean_base_url(value: &str) -> Result<String, AppError> {
    let value = value.trim().trim_end_matches('/');
    let url =
        reqwest::Url::parse(value).map_err(|_| AppError::validation("Base URL 不是合法地址"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AppError::validation("Base URL 只支持 http 或 https"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::validation(
            "不要把密钥写进 Base URL，请填写 API Key",
        ));
    }
    if url.query().is_some() {
        return Err(AppError::validation("Base URL 不能包含查询参数"));
    }
    Ok(value.to_string())
}

fn clean_api_key(value: Option<String>) -> Result<Option<String>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let mut value = value.trim().to_string();
    if value.is_empty() {
        return Ok(None);
    }
    if let Some(rest) = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
    {
        value = rest.trim().to_string();
    }
    if value.is_empty()
        || value.len() > 4_000
        || value
            .chars()
            .any(|char| char == '\n' || char == '\r' || char == '\0')
    {
        return Err(AppError::validation("API Key 无效"));
    }
    Ok(Some(value))
}

pub(crate) fn clean_headers(
    headers: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, AppError> {
    if headers.len() > 20 {
        return Err(AppError::validation("自定义 Headers 最多 20 项"));
    }
    let mut cleaned = BTreeMap::new();
    for (key, value) in headers {
        let key = key.trim().to_string();
        let value = value.trim().to_string();
        if key.is_empty() {
            continue;
        }
        if key.len() > 80 || value.len() > 500 {
            return Err(AppError::validation(format!("Header「{key}」过长")));
        }
        if key.chars().any(|char| char.is_whitespace() || char == ':')
            || value
                .chars()
                .any(|char| char == '\n' || char == '\r' || char == '\0')
        {
            return Err(AppError::validation(format!("Header「{key}」包含非法字符")));
        }
        if is_sensitive_key(&key) {
            return Err(AppError::validation(
                "请把密钥填在 API Key，不要写进自定义 Headers",
            ));
        }
        cleaned.insert(key, value);
    }
    Ok(cleaned)
}

fn clean_models_path(path: Option<String>) -> Result<Option<String>, AppError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let path = path.trim().trim_start_matches('/').to_string();
    if path.is_empty() {
        return Ok(None);
    }
    validate_rel_path(&path, "modelsPath")?;
    Ok(Some(path))
}
