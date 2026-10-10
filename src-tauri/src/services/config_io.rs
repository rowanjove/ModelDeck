use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::credentials::SecretStore;
use crate::db;
use crate::domain::{now_iso, ModelDefinition, Preset, ProviderType};
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedProvider {
    pub id: String,
    pub name: String,
    pub provider_type: ProviderType,
    pub base_url: String,
    pub headers: BTreeMap<String, String>,
    pub timeout_ms: u64,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedConfig {
    pub version: String,
    pub exported_at: String,
    pub providers: Vec<ExportedProvider>,
    pub models: Vec<ModelDefinition>,
    pub presets: Vec<Preset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub providers_imported: usize,
    pub models_imported: usize,
    pub presets_imported: usize,
}

pub async fn export_all_config(pool: &SqlitePool) -> Result<ExportedConfig, AppError> {
    let providers_records = db::list_providers(pool).await?;
    let mut models = db::list_models(pool).await?;
    for model in &mut models {
        sanitize_model(model);
    }

    let mut presets = Vec::new();
    let preset_rows = sqlx::query(
        "SELECT id, name, model_id, parameters_json, system_prompt, user_template, created_at, updated_at FROM presets ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .map_err(AppError::db)?;

    for row in preset_rows {
        if let Ok(preset) = crate::services::presets::preset_from_row(&row) {
            presets.push(preset);
        }
    }
    for preset in &mut presets {
        for (key, value) in &mut preset.parameters {
            if crate::redact::is_sensitive_key(key) {
                *value = serde_json::Value::String("{{apiKey}}".to_string());
            } else {
                sanitize_json(value);
            }
        }
        if let Some(system_prompt) = preset.system_prompt.as_mut() {
            *system_prompt = crate::redact::mask_embedded(system_prompt);
        }
        if let Some(user_template) = preset.user_template.as_mut() {
            *user_template = crate::redact::mask_embedded(user_template);
        }
    }

    // Sanitize providers: NEVER export API keys / credentials!
    let exported_providers = providers_records
        .into_iter()
        .map(|p| {
            let mut sanitized_headers = BTreeMap::new();
            for (k, v) in p.headers {
                if crate::redact::is_sensitive_key(&k) {
                    sanitized_headers.insert(k.clone(), sanitize_sensitive_header(&k, &v));
                } else {
                    sanitized_headers.insert(k, v);
                }
            }
            ExportedProvider {
                id: p.id,
                name: p.name,
                provider_type: p.provider_type,
                base_url: p.base_url,
                headers: sanitized_headers,
                timeout_ms: p.timeout_ms,
                enabled: p.enabled,
            }
        })
        .collect();

    Ok(ExportedConfig {
        version: "1.0".to_string(),
        exported_at: now_iso(),
        providers: exported_providers,
        models,
        presets,
    })
}

pub async fn import_config(
    pool: &SqlitePool,
    secrets: &dyn SecretStore,
    config: ExportedConfig,
) -> Result<ImportResult, AppError> {
    if config.version != "1.0" {
        return Err(AppError::validation(format!(
            "不支持的配置导出版本: {}",
            config.version
        )));
    }
    if config.providers.len() > 100 {
        return Err(AppError::validation("单次导入 Provider 数量不能超过 100"));
    }
    if config.models.len() > 1000 {
        return Err(AppError::validation("单次导入 Model 数量不能超过 1000"));
    }
    if config.presets.len() > 1000 {
        return Err(AppError::validation("单次导入预设数量不能超过 1000"));
    }

    let now = now_iso();
    let mut providers_count = 0;
    let mut models_count = 0;
    let mut presets_count = 0;
    let mut credentials_to_delete = HashSet::new();

    let mut tx = pool.begin().await.map_err(AppError::db)?;

    for p in config.providers {
        let name = p.name.trim();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(AppError::validation(
                "Provider 名称必须在 1 到 80 个字符之间",
            ));
        }
        let clean_url = crate::services::providers::clean_base_url(&p.base_url)?;
        let clean_headers = crate::services::providers::clean_headers(p.headers)?;
        if !(1_000..=600_000).contains(&p.timeout_ms) {
            return Err(AppError::validation("超时时间需要在 1 到 600 秒之间"));
        }

        let existing: Option<(Option<String>, String)> =
            sqlx::query_as("SELECT credential_id, base_url FROM providers WHERE id = ?")
                .bind(&p.id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(AppError::db)?;

        let credential_id = match existing {
            Some((cred, old_url)) => {
                if old_url == clean_url {
                    cred
                } else {
                    if let Some(credential_id) = cred.as_ref() {
                        credentials_to_delete.insert(credential_id.clone());
                    }
                    None // Base URL 变更，断开旧 API Key 绑定，防止旧凭据泄露至新地址
                }
            }
            None => None,
        };

        let headers_json = serde_json::to_string(&clean_headers)
            .map_err(|e| AppError::validation(format!("Header 序列化错误: {e}")))?;

        sqlx::query(
            "INSERT INTO providers
             (id, name, type, base_url, credential_id, headers_json, timeout_ms, enabled, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               type = excluded.type,
               base_url = excluded.base_url,
               credential_id = excluded.credential_id,
               headers_json = excluded.headers_json,
               timeout_ms = excluded.timeout_ms,
               enabled = excluded.enabled,
               updated_at = excluded.updated_at",
        )
        .bind(&p.id)
        .bind(name)
        .bind(p.provider_type.as_str())
        .bind(&clean_url)
        .bind(&credential_id)
        .bind(&headers_json)
        .bind(p.timeout_ms as i64)
        .bind(i64::from(p.enabled))
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(AppError::db)?;

        providers_count += 1;
    }

    for mut m in config.models {
        if m.name.trim().is_empty() || m.name.chars().count() > 120 {
            return Err(AppError::validation(
                "模型显示名称需要在 1 到 120 个字符之间",
            ));
        }
        if m.model_id.trim().is_empty() || m.model_id.chars().count() > 200 {
            return Err(AppError::validation("Model ID 需要在 1 到 200 个字符之间"));
        }
        if m.provider_id.trim().is_empty() {
            return Err(AppError::validation("模型缺少 Provider ID 关联"));
        }
        let provider_exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM providers WHERE id = ? LIMIT 1")
                .bind(&m.provider_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(AppError::db)?;
        if provider_exists.is_none() {
            return Err(AppError::validation(format!(
                "模型引用了不存在的 Provider: {}",
                m.provider_id
            )));
        }
        crate::schema::validate_and_migrate_schema_version(m.schema_version, &m.name)?;
        crate::schema::validate_definition(
            &mut m.parameters,
            &mut m.constraints,
            &mut m.transport,
            &m.capabilities,
        )?;

        let cap_json = serde_json::to_string(&m.capabilities)
            .map_err(|e| AppError::validation(format!("能力参数序列化错误: {e}")))?;
        let params_json = serde_json::to_string(&m.parameters)
            .map_err(|e| AppError::validation(format!("模型参数序列化错误: {e}")))?;
        let constraints_json = serde_json::to_string(&m.constraints)
            .map_err(|e| AppError::validation(format!("约束参数序列化错误: {e}")))?;
        let transport_json = m
            .transport
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| AppError::validation(format!("传输配置序列化错误: {e}")))?;

        sqlx::query(
            "INSERT INTO models
             (id, provider_id, name, model_id, modality, capabilities_json, parameters_json, constraints_json, transport_json, enabled, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               provider_id = excluded.provider_id,
               name = excluded.name,
               model_id = excluded.model_id,
               modality = excluded.modality,
               capabilities_json = excluded.capabilities_json,
               parameters_json = excluded.parameters_json,
               constraints_json = excluded.constraints_json,
               transport_json = excluded.transport_json,
               enabled = excluded.enabled,
               updated_at = excluded.updated_at",
        )
        .bind(&m.id)
        .bind(&m.provider_id)
        .bind(&m.name)
        .bind(&m.model_id)
        .bind(m.modality.as_str())
        .bind(&cap_json)
        .bind(&params_json)
        .bind(&constraints_json)
        .bind(&transport_json)
        .bind(i64::from(m.enabled))
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(AppError::db)?;

        models_count += 1;
    }

    for pr in config.presets {
        let preset_name = pr.name.trim();
        if preset_name.is_empty() || preset_name.chars().count() > 80 {
            return Err(AppError::validation("预设名称需要 1 到 80 个字符"));
        }
        if pr.model_id.trim().is_empty() {
            return Err(AppError::validation("预设缺少模型引用"));
        }
        let model_exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM models WHERE id = ? LIMIT 1")
                .bind(&pr.model_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(AppError::db)?;
        if model_exists.is_none() {
            return Err(AppError::validation(format!(
                "预设引用了不存在的模型: {}",
                pr.model_id
            )));
        }
        crate::schema::validate_and_migrate_schema_version(pr.schema_version, &pr.name)?;
        let params_json = serde_json::to_string(&pr.parameters)
            .map_err(|e| AppError::validation(format!("预设参数序列化错误: {}", e)))?;

        sqlx::query(
            "INSERT INTO presets
             (id, name, model_id, parameters_json, system_prompt, user_template, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               model_id = excluded.model_id,
               parameters_json = excluded.parameters_json,
               system_prompt = excluded.system_prompt,
               user_template = excluded.user_template,
               updated_at = excluded.updated_at",
        )
        .bind(&pr.id)
        .bind(preset_name)
        .bind(&pr.model_id)
        .bind(&params_json)
        .bind(&pr.system_prompt)
        .bind(&pr.user_template)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(AppError::db)?;

        presets_count += 1;
    }

    tx.commit().await.map_err(AppError::db)?;

    // The database commit and OS keyring cannot share a transaction. Delete
    // detached credentials only after commit and only when no provider still
    // references them; cleanup failures are logged without misreporting the
    // already-committed import as rolled back.
    for credential_id in credentials_to_delete {
        let referenced = sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM providers WHERE credential_id = ?)",
        )
        .bind(&credential_id)
        .fetch_one(pool)
        .await;
        match referenced {
            Ok(0) => {
                if let Err(err) = secrets.delete(&credential_id) {
                    crate::logging::global_logger().warn(
                        "config",
                        None,
                        None,
                        None,
                        None,
                        "orphan_credential_cleanup_failed",
                        &err.message,
                    );
                }
            }
            Ok(_) => {}
            Err(err) => crate::logging::global_logger().warn(
                "config",
                None,
                None,
                None,
                None,
                "orphan_credential_check_failed",
                &err.to_string(),
            ),
        }
    }

    Ok(ImportResult {
        providers_imported: providers_count,
        models_imported: models_count,
        presets_imported: presets_count,
    })
}

fn sanitize_model(model: &mut ModelDefinition) {
    let Some(transport) = model.transport.as_mut() else {
        return;
    };
    if let Some(workflow) = transport.workflow.as_mut() {
        sanitize_json(workflow);
    }
    if let Some(http) = transport.http.as_mut() {
        for header in &mut http.headers {
            if crate::redact::is_sensitive_key(&header.name) {
                header.value = sanitize_sensitive_header(&header.name, &header.value);
            } else {
                header.value = crate::redact::mask_embedded(&header.value);
            }
        }
        if let Some(body) = http.body.as_mut() {
            sanitize_json(body);
        }
        if let Some(poll) = http.poll.as_mut() {
            if let Some(body) = poll.body.as_mut() {
                sanitize_json(body);
            }
        }
    }
}

fn sanitize_sensitive_header(name: &str, value: &str) -> String {
    const PLACEHOLDER: &str = "{{apiKey}}";
    let trimmed = value.trim();
    let is_auth_header = matches!(
        name.trim().to_ascii_lowercase().replace('-', "_").as_str(),
        "authorization" | "proxy_authorization"
    );
    if trimmed.eq_ignore_ascii_case(PLACEHOLDER)
        || trimmed.eq_ignore_ascii_case(&format!("Bearer {PLACEHOLDER}"))
    {
        return trimmed.to_string();
    }
    if is_auth_header {
        if let Some((scheme, secret)) = trimmed.split_once(char::is_whitespace) {
            if !scheme.is_empty() && !secret.trim().is_empty() {
                return format!("{scheme} {PLACEHOLDER}");
            }
        }
    }
    PLACEHOLDER.to_string()
}

fn sanitize_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                if crate::redact::is_sensitive_key(key) {
                    let sanitized = match value.as_str() {
                        Some(raw) if key.eq_ignore_ascii_case("authorization") => {
                            sanitize_sensitive_header(key, raw)
                        }
                        _ => "{{apiKey}}".to_string(),
                    };
                    *value = serde_json::Value::String(sanitized);
                } else {
                    sanitize_json(value);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                sanitize_json(item);
            }
        }
        serde_json::Value::String(text) => {
            *text = crate::redact::mask_embedded(text);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_sensitive_header;

    #[test]
    fn sensitive_authorization_export_preserves_scheme_and_placeholder() {
        assert_eq!(
            sanitize_sensitive_header("Authorization", "Bearer {{apiKey}}"),
            "Bearer {{apiKey}}"
        );
        assert_eq!(
            sanitize_sensitive_header("Authorization", "Bearer live-secret"),
            "Bearer {{apiKey}}"
        );
        assert_eq!(
            sanitize_sensitive_header("X-API-Key", "live-secret"),
            "{{apiKey}}"
        );
        assert_eq!(
            sanitize_sensitive_header("Authorization", "{{apiKey}}"),
            "{{apiKey}}"
        );
    }

    #[tokio::test]
    async fn export_config_never_contains_actual_secrets() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::db::migrate(&pool).await.unwrap();

        let mut headers = std::collections::BTreeMap::new();
        headers.insert(
            "Authorization".to_string(),
            "Bearer secret-live-token".to_string(),
        );
        headers.insert("X-API-Key".to_string(), "top-secret-key".to_string());
        let provider = crate::domain::ProviderRecord {
            id: "p1".to_string(),
            name: "Provider 1".to_string(),
            provider_type: crate::domain::ProviderType::Openai,
            base_url: "https://api.openai.com/v1".to_string(),
            credential_id: Some("cred-1".to_string()),
            headers,
            timeout_ms: 30000,
            enabled: true,
            created_at: crate::domain::now_iso(),
            updated_at: crate::domain::now_iso(),
        };
        crate::db::insert_provider(&pool, &provider).await.unwrap();

        let exported = super::export_all_config(&pool).await.unwrap();
        let exp_p = exported
            .providers
            .into_iter()
            .find(|p| p.id == "p1")
            .unwrap();
        assert_eq!(
            exp_p.headers.get("Authorization").unwrap(),
            "Bearer {{apiKey}}"
        );
        assert_eq!(exp_p.headers.get("X-API-Key").unwrap(), "{{apiKey}}");
    }

    #[tokio::test]
    async fn import_config_disconnects_and_cleans_credential_when_url_changes() {
        use crate::credentials::SecretStore;

        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::db::migrate(&pool).await.unwrap();

        let secrets = crate::credentials::MemorySecretStore::default();
        secrets.set("cred-url-test", "sk-secret-abc").unwrap();

        let provider = crate::domain::ProviderRecord {
            id: "prov-url".to_string(),
            name: "Old Provider".to_string(),
            provider_type: crate::domain::ProviderType::Openai,
            base_url: "https://api.old-url.com/v1".to_string(),
            credential_id: Some("cred-url-test".to_string()),
            headers: std::collections::BTreeMap::new(),
            timeout_ms: 30000,
            enabled: true,
            created_at: crate::domain::now_iso(),
            updated_at: crate::domain::now_iso(),
        };
        crate::db::insert_provider(&pool, &provider).await.unwrap();

        // 1. 导入相同 URL：保留凭据绑定
        let config_same_url = super::ExportedConfig {
            version: "1.0".to_string(),
            exported_at: crate::domain::now_iso(),
            providers: vec![super::ExportedProvider {
                id: "prov-url".to_string(),
                name: "Updated Name".to_string(),
                provider_type: crate::domain::ProviderType::Openai,
                base_url: "https://api.old-url.com/v1".to_string(),
                headers: std::collections::BTreeMap::new(),
                timeout_ms: 30000,
                enabled: true,
            }],
            models: vec![],
            presets: vec![],
        };
        super::import_config(&pool, &secrets, config_same_url)
            .await
            .unwrap();
        let p = crate::db::get_provider(&pool, "prov-url").await.unwrap();
        assert_eq!(p.credential_id.as_deref(), Some("cred-url-test"));
        assert!(secrets.get("cred-url-test").unwrap().is_some());

        // 2. 导入不同 URL：解除旧凭据绑定并从 store 中删除孤儿凭据，防泄露至新地址
        let config_new_url = super::ExportedConfig {
            version: "1.0".to_string(),
            exported_at: crate::domain::now_iso(),
            providers: vec![super::ExportedProvider {
                id: "prov-url".to_string(),
                name: "Updated Name".to_string(),
                provider_type: crate::domain::ProviderType::Openai,
                base_url: "https://api.new-url.com/v1".to_string(),
                headers: std::collections::BTreeMap::new(),
                timeout_ms: 30000,
                enabled: true,
            }],
            models: vec![],
            presets: vec![],
        };
        super::import_config(&pool, &secrets, config_new_url)
            .await
            .unwrap();
        let p_updated = crate::db::get_provider(&pool, "prov-url").await.unwrap();
        assert_eq!(p_updated.credential_id, None);
        assert!(secrets.get("cred-url-test").unwrap().is_none());
    }
}
