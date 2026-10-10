use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};

use crate::domain::{
    now_iso, Modality, ModelDefinition, ParameterDefinition, ProviderRecord, ProviderType,
    TransportDefinition,
};
use crate::error::AppError;

pub async fn open(path: &Path) -> Result<SqlitePool, AppError> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(std::time::Duration::from_secs(10));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .map_err(AppError::db)?;
    Ok(pool)
}

pub async fn migrate(pool: &SqlitePool) -> Result<(), AppError> {
    let mut tx = pool.begin().await.map_err(AppError::db)?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        )",
    )
    .execute(&mut *tx)
    .await
    .map_err(AppError::db)?;
    for (version, sql) in [
        (1_i64, include_str!("../migrations/001_init.sql")),
        (2_i64, include_str!("../migrations/002_chat.sql")),
        (3_i64, include_str!("../migrations/003_history.sql")),
        (4_i64, include_str!("../migrations/004_voices.sql")),
        (5_i64, include_str!("../migrations/005_presets.sql")),
        (6_i64, include_str!("../migrations/006_mcp_servers.sql")),
        (7_i64, include_str!("../migrations/007_agent_memory.sql")),
        (8_i64, include_str!("../migrations/008_compaction.sql")),
        (9_i64, include_str!("../migrations/009_model_pricing.sql")),
        (10_i64, include_str!("../migrations/010_settings.sql")),
        (11_i64, include_str!("../migrations/011_prompt_presets.sql")),
        (
            12_i64,
            include_str!("../migrations/012_voice_library_v2.sql"),
        ),
        (
            13_i64,
            include_str!("../migrations/013_tool_permissions.sql"),
        ),
        (14_i64, include_str!("../migrations/014_pipelines.sql")),
        (
            15_i64,
            include_str!("../migrations/015_developer_presets.sql"),
        ),
        (
            16_i64,
            include_str!("../migrations/016_hardening_indexes.sql"),
        ),
        (
            17_i64,
            include_str!("../migrations/017_foreign_keys_and_cascade.sql"),
        ),
        (18_i64, include_str!("../migrations/018_fts5.sql")),
        (
            19_i64,
            include_str!("../migrations/019_deleted_generation_tasks.sql"),
        ),
        (
            20_i64,
            include_str!("../migrations/020_task_output_and_tombstones.sql"),
        ),
        (
            21_i64,
            include_str!("../migrations/021_task_recovery_fields.sql"),
        ),
        (
            22_i64,
            include_str!("../migrations/022_task_consistency_fields.sql"),
        ),
        (
            23_i64,
            include_str!("../migrations/023_pricing_multimodal_fields.sql"),
        ),
    ] {
        let applied: Option<i64> =
            sqlx::query_scalar("SELECT version FROM schema_migrations WHERE version = ?")
                .bind(version)
                .fetch_optional(&mut *tx)
                .await
                .map_err(AppError::db)?;
        if applied.is_some() {
            continue;
        }
        sqlx::raw_sql(sql)
            .execute(&mut *tx)
            .await
            .map_err(AppError::db)?;
        sqlx::query("INSERT INTO schema_migrations (version, applied_at) VALUES (?, ?)")
            .bind(version)
            .bind(now_iso())
            .execute(&mut *tx)
            .await
            .map_err(AppError::db)?;
    }
    tx.commit().await.map_err(AppError::db)?;
    Ok(())
}

pub async fn list_providers(pool: &SqlitePool) -> Result<Vec<ProviderRecord>, AppError> {
    let rows = sqlx::query(
        "SELECT id, name, type, base_url, credential_id, headers_json, timeout_ms, enabled, created_at, updated_at
         FROM providers ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(AppError::db)?;
    rows.iter().map(provider_from_row).collect()
}

pub async fn get_provider(pool: &SqlitePool, id: &str) -> Result<ProviderRecord, AppError> {
    let row = sqlx::query(
        "SELECT id, name, type, base_url, credential_id, headers_json, timeout_ms, enabled, created_at, updated_at
         FROM providers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::db)?;
    let row = row.ok_or_else(|| AppError::not_found("找不到 Provider"))?;
    provider_from_row(&row)
}

pub async fn insert_provider(pool: &SqlitePool, provider: &ProviderRecord) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO providers
         (id, name, type, base_url, credential_id, headers_json, timeout_ms, enabled, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&provider.id)
    .bind(&provider.name)
    .bind(provider.provider_type.as_str())
    .bind(&provider.base_url)
    .bind(&provider.credential_id)
    .bind(json(&provider.headers)?)
    .bind(provider.timeout_ms as i64)
    .bind(i64::from(provider.enabled))
    .bind(&provider.created_at)
    .bind(&provider.updated_at)
    .execute(pool)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

pub async fn update_provider(pool: &SqlitePool, provider: &ProviderRecord) -> Result<(), AppError> {
    let result = sqlx::query(
        "UPDATE providers
         SET name = ?, type = ?, base_url = ?, credential_id = ?, headers_json = ?, timeout_ms = ?, enabled = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(&provider.name)
    .bind(provider.provider_type.as_str())
    .bind(&provider.base_url)
    .bind(&provider.credential_id)
    .bind(json(&provider.headers)?)
    .bind(provider.timeout_ms as i64)
    .bind(i64::from(provider.enabled))
    .bind(&provider.updated_at)
    .bind(&provider.id)
    .execute(pool)
    .await
    .map_err(AppError::db)?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("找不到 Provider"));
    }
    Ok(())
}

pub async fn delete_provider(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let result = sqlx::query("DELETE FROM providers WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(AppError::db)?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("找不到 Provider"));
    }
    Ok(())
}

pub async fn list_models(pool: &SqlitePool) -> Result<Vec<ModelDefinition>, AppError> {
    let rows = sqlx::query(
        "SELECT id, provider_id, name, model_id, modality, capabilities_json, parameters_json, constraints_json, transport_json, enabled, created_at, updated_at
         FROM models ORDER BY name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(AppError::db)?;
    rows.iter().map(model_from_row).collect()
}

pub async fn get_model(pool: &SqlitePool, id: &str) -> Result<ModelDefinition, AppError> {
    let row = sqlx::query(
        "SELECT id, provider_id, name, model_id, modality, capabilities_json, parameters_json, constraints_json, transport_json, enabled, created_at, updated_at
         FROM models WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::db)?;
    let row = row.ok_or_else(|| AppError::not_found("找不到模型"))?;
    model_from_row(&row)
}

pub async fn insert_model(pool: &SqlitePool, model: &ModelDefinition) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO models
         (id, provider_id, name, model_id, modality, capabilities_json, parameters_json, constraints_json, transport_json, enabled, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&model.id)
    .bind(&model.provider_id)
    .bind(&model.name)
    .bind(&model.model_id)
    .bind(model.modality.as_str())
    .bind(json(&model.capabilities)?)
    .bind(json(&model.parameters)?)
    .bind(json(&model.constraints)?)
    .bind(model.transport.as_ref().map(json).transpose()?)
    .bind(i64::from(model.enabled))
    .bind(&model.created_at)
    .bind(&model.updated_at)
    .execute(pool)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

pub async fn update_model(pool: &SqlitePool, model: &ModelDefinition) -> Result<(), AppError> {
    let result = sqlx::query(
        "UPDATE models
         SET provider_id = ?, name = ?, model_id = ?, modality = ?, capabilities_json = ?, parameters_json = ?, constraints_json = ?, transport_json = ?, enabled = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(&model.provider_id)
    .bind(&model.name)
    .bind(&model.model_id)
    .bind(model.modality.as_str())
    .bind(json(&model.capabilities)?)
    .bind(json(&model.parameters)?)
    .bind(json(&model.constraints)?)
    .bind(model.transport.as_ref().map(json).transpose()?)
    .bind(i64::from(model.enabled))
    .bind(&model.updated_at)
    .bind(&model.id)
    .execute(pool)
    .await
    .map_err(AppError::db)?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("找不到模型"));
    }
    Ok(())
}

pub async fn delete_model(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let result = sqlx::query("DELETE FROM models WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(AppError::db)?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("找不到模型"));
    }
    Ok(())
}

fn provider_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<ProviderRecord, AppError> {
    let provider_type = row.try_get::<String, _>("type").map_err(AppError::db)?;
    let headers_json = row
        .try_get::<String, _>("headers_json")
        .map_err(AppError::db)?;
    Ok(ProviderRecord {
        id: row.try_get("id").map_err(AppError::db)?,
        name: row.try_get("name").map_err(AppError::db)?,
        provider_type: ProviderType::from_db(&provider_type)
            .ok_or_else(|| AppError::storage(format!("未知的 Provider 类型: {provider_type}")))?,
        base_url: row.try_get("base_url").map_err(AppError::db)?,
        credential_id: row.try_get("credential_id").map_err(AppError::db)?,
        headers: serde_json::from_str(&headers_json)
            .map_err(|err| AppError::storage(format!("Headers 已损坏: {err}")))?,
        timeout_ms: u64::try_from(row.try_get::<i64, _>("timeout_ms").map_err(AppError::db)?)
            .unwrap_or(120_000),
        enabled: row.try_get::<i64, _>("enabled").map_err(AppError::db)? != 0,
        created_at: row.try_get("created_at").map_err(AppError::db)?,
        updated_at: row.try_get("updated_at").map_err(AppError::db)?,
    })
}

fn model_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<ModelDefinition, AppError> {
    let modality = row.try_get::<String, _>("modality").map_err(AppError::db)?;
    let transport_json = row
        .try_get::<Option<String>, _>("transport_json")
        .map_err(AppError::db)?;
    Ok(ModelDefinition {
        id: row.try_get("id").map_err(AppError::db)?,
        provider_id: row.try_get("provider_id").map_err(AppError::db)?,
        name: row.try_get("name").map_err(AppError::db)?,
        model_id: row.try_get("model_id").map_err(AppError::db)?,
        modality: Modality::from_db(&modality)
            .ok_or_else(|| AppError::storage(format!("未知模态: {modality}")))?,
        capabilities: parse_json(
            &row.try_get::<String, _>("capabilities_json")
                .map_err(AppError::db)?,
            "能力",
        )?,
        parameters: parse_json::<Vec<ParameterDefinition>>(
            &row.try_get::<String, _>("parameters_json")
                .map_err(AppError::db)?,
            "参数",
        )?,
        constraints: parse_json(
            &row.try_get::<String, _>("constraints_json")
                .map_err(AppError::db)?,
            "约束",
        )?,
        transport: transport_json
            .as_deref()
            .map(|value| parse_json::<TransportDefinition>(value, "传输"))
            .transpose()?,
        schema_version: 1,
        enabled: row.try_get::<i64, _>("enabled").map_err(AppError::db)? != 0,
        created_at: row.try_get("created_at").map_err(AppError::db)?,
        updated_at: row.try_get("updated_at").map_err(AppError::db)?,
    })
}

fn json<T: serde::Serialize>(value: &T) -> Result<String, AppError> {
    serde_json::to_string(value).map_err(|err| AppError::storage(format!("无法保存 JSON: {err}")))
}

fn parse_json<T: serde::de::DeserializeOwned>(value: &str, label: &str) -> Result<T, AppError> {
    serde_json::from_str(value).map_err(|err| AppError::storage(format!("{label}已损坏: {err}")))
}

#[derive(Debug, Clone)]
pub struct ConversationRow {
    pub id: String,
    pub title: String,
    pub mode: String,
    pub model_id: String,
    pub system_prompt: String,
    pub context_strategy: String,
    pub context_limit: u32,
    pub summary: Option<String>,
    pub summary_until: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct MessageRow {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content_json: String,
    pub token_count: i64,
    pub raw_request_json: Option<String>,
    pub raw_response_json: Option<String>,
    pub is_pinned: bool,
    pub created_at: String,
}

pub async fn list_conversations(pool: &SqlitePool) -> Result<Vec<ConversationRow>, AppError> {
    let rows = sqlx::query(
        "SELECT id, title, mode, model_id, system_prompt, context_strategy, context_limit, summary, summary_until, created_at, updated_at
         FROM conversations ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(AppError::db)?;
    rows.iter().map(conversation_from_row).collect()
}

pub async fn get_conversation(pool: &SqlitePool, id: &str) -> Result<ConversationRow, AppError> {
    let row = sqlx::query(
        "SELECT id, title, mode, model_id, system_prompt, context_strategy, context_limit, summary, summary_until, created_at, updated_at
         FROM conversations WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::db)?;
    let row = row.ok_or_else(|| AppError::not_found("找不到对话"))?;
    conversation_from_row(&row)
}

pub async fn insert_conversation(pool: &SqlitePool, row: &ConversationRow) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO conversations
         (id, title, mode, model_id, system_prompt, context_strategy, context_limit, summary, summary_until, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.title)
    .bind(&row.mode)
    .bind(&row.model_id)
    .bind(&row.system_prompt)
    .bind(&row.context_strategy)
    .bind(i64::from(row.context_limit))
    .bind(&row.summary)
    .bind(&row.summary_until)
    .bind(&row.created_at)
    .bind(&row.updated_at)
    .execute(pool)
    .await
    .map_err(AppError::db)?;

    let _ = sync_fts_entry(pool, &row.id, "conversation", &row.title, &row.title).await;
    Ok(())
}

pub async fn update_conversation(pool: &SqlitePool, row: &ConversationRow) -> Result<(), AppError> {
    update_conversation_conn(pool, row).await
}

pub async fn update_conversation_conn<'c, E>(
    executor: E,
    row: &ConversationRow,
) -> Result<(), AppError>
where
    E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
{
    let result = sqlx::query(
        "UPDATE conversations
         SET title = ?, mode = ?, model_id = ?, system_prompt = ?, context_strategy = ?, context_limit = ?, summary = ?, summary_until = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(&row.title)
    .bind(&row.mode)
    .bind(&row.model_id)
    .bind(&row.system_prompt)
    .bind(&row.context_strategy)
    .bind(i64::from(row.context_limit))
    .bind(&row.summary)
    .bind(&row.summary_until)
    .bind(&row.updated_at)
    .bind(&row.id)
    .execute(executor)
    .await
    .map_err(AppError::db)?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("找不到对话"));
    }
    Ok(())
}

pub async fn delete_conversation(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let mut tx = pool.begin().await.map_err(AppError::db)?;
    delete_conversation_conn(&mut *tx, id).await?;
    tx.commit().await.map_err(AppError::db)?;
    Ok(())
}

pub async fn delete_conversation_conn<'c, E>(executor: E, id: &str) -> Result<(), AppError>
where
    E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
{
    let result = sqlx::query("DELETE FROM conversations WHERE id = ?")
        .bind(id)
        .execute(executor)
        .await
        .map_err(AppError::db)?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("找不到对话"));
    }
    Ok(())
}

pub async fn list_messages(
    pool: &SqlitePool,
    conversation_id: &str,
) -> Result<Vec<MessageRow>, AppError> {
    let rows = sqlx::query(
        "SELECT id, conversation_id, role, content_json, token_count, NULL AS raw_request_json, NULL AS raw_response_json, is_pinned, created_at
         FROM messages WHERE conversation_id = ? ORDER BY created_at ASC, id ASC",
    )
    .bind(conversation_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::db)?;
    rows.iter().map(message_from_row).collect()
}

pub async fn get_message(pool: &SqlitePool, id: &str) -> Result<MessageRow, AppError> {
    let row = sqlx::query(
        "SELECT id, conversation_id, role, content_json, token_count, raw_request_json, raw_response_json, is_pinned, created_at
         FROM messages WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::db)?;
    let row = row.ok_or_else(|| AppError::not_found("找不到消息"))?;
    message_from_row(&row)
}

pub async fn insert_message(pool: &SqlitePool, row: &MessageRow) -> Result<(), AppError> {
    insert_message_conn(pool, row).await?;
    let _ = sync_fts_entry(pool, &row.id, "message", &row.role, &row.content_json).await;
    Ok(())
}

pub async fn insert_message_conn<'c, E>(executor: E, row: &MessageRow) -> Result<(), AppError>
where
    E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
{
    sqlx::query(
        "INSERT INTO messages
         (id, conversation_id, role, content_json, token_count, raw_request_json, raw_response_json, is_pinned, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.conversation_id)
    .bind(&row.role)
    .bind(&row.content_json)
    .bind(row.token_count)
    .bind(&row.raw_request_json)
    .bind(&row.raw_response_json)
    .bind(if row.is_pinned { 1_i64 } else { 0_i64 })
    .bind(&row.created_at)
    .execute(executor)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

#[allow(dead_code)]
pub async fn update_message(pool: &SqlitePool, row: &MessageRow) -> Result<(), AppError> {
    update_message_conn(pool, row).await
}

pub async fn update_message_conn<'c, E>(executor: E, row: &MessageRow) -> Result<(), AppError>
where
    E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
{
    let result = sqlx::query(
        "UPDATE messages
         SET content_json = ?, token_count = ?, raw_request_json = ?, raw_response_json = ?, is_pinned = ?
         WHERE id = ?",
    )
    .bind(&row.content_json)
    .bind(row.token_count)
    .bind(&row.raw_request_json)
    .bind(&row.raw_response_json)
    .bind(if row.is_pinned { 1_i64 } else { 0_i64 })
    .bind(&row.id)
    .execute(executor)
    .await
    .map_err(AppError::db)?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("找不到消息"));
    }
    Ok(())
}

#[allow(dead_code)]
pub async fn delete_messages_after(
    pool: &SqlitePool,
    conversation_id: &str,
    created_at: &str,
    id: &str,
    inclusive: bool,
) -> Result<(), AppError> {
    delete_messages_after_conn(pool, conversation_id, created_at, id, inclusive).await
}

pub async fn delete_messages_after_conn<'c, E>(
    executor: E,
    conversation_id: &str,
    created_at: &str,
    id: &str,
    inclusive: bool,
) -> Result<(), AppError>
where
    E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
{
    let comparison = if inclusive { ">=" } else { ">" };
    let sql = format!(
        "DELETE FROM messages
         WHERE conversation_id = ? AND (created_at > ? OR (created_at = ? AND id {comparison} ?))"
    );
    sqlx::query(&sql)
        .bind(conversation_id)
        .bind(created_at)
        .bind(created_at)
        .bind(id)
        .execute(executor)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

#[allow(dead_code)]
pub async fn list_messages_after(
    pool: &SqlitePool,
    conversation_id: &str,
    created_at: &str,
    id: &str,
    inclusive: bool,
) -> Result<Vec<MessageRow>, AppError> {
    let comparison = if inclusive { ">=" } else { ">" };
    let sql = format!(
        "SELECT id, conversation_id, role, content_json, token_count, raw_request_json, raw_response_json, is_pinned, created_at
         FROM messages
         WHERE conversation_id = ? AND (created_at > ? OR (created_at = ? AND id {comparison} ?))
         ORDER BY created_at ASC, id ASC"
    );
    let rows = sqlx::query(&sql)
        .bind(conversation_id)
        .bind(created_at)
        .bind(created_at)
        .bind(id)
        .fetch_all(pool)
        .await
        .map_err(AppError::db)?;
    let mut out = Vec::new();
    for row in &rows {
        out.push(message_from_row(row)?);
    }
    Ok(out)
}

fn conversation_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<ConversationRow, AppError> {
    let limit = row
        .try_get::<i64, _>("context_limit")
        .map_err(AppError::db)?;
    Ok(ConversationRow {
        id: row.try_get("id").map_err(AppError::db)?,
        title: row.try_get("title").map_err(AppError::db)?,
        mode: row.try_get("mode").map_err(AppError::db)?,
        model_id: row.try_get("model_id").map_err(AppError::db)?,
        system_prompt: row.try_get("system_prompt").map_err(AppError::db)?,
        context_strategy: row.try_get("context_strategy").map_err(AppError::db)?,
        context_limit: u32::try_from(limit).unwrap_or(20),
        summary: row.try_get("summary").map_err(AppError::db)?,
        summary_until: row.try_get("summary_until").map_err(AppError::db)?,
        created_at: row.try_get("created_at").map_err(AppError::db)?,
        updated_at: row.try_get("updated_at").map_err(AppError::db)?,
    })
}

fn message_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<MessageRow, AppError> {
    let pinned: i64 = row.try_get("is_pinned").unwrap_or(0);
    Ok(MessageRow {
        id: row.try_get("id").map_err(AppError::db)?,
        conversation_id: row.try_get("conversation_id").map_err(AppError::db)?,
        role: row.try_get("role").map_err(AppError::db)?,
        content_json: row.try_get("content_json").map_err(AppError::db)?,
        token_count: row.try_get("token_count").map_err(AppError::db)?,
        raw_request_json: row.try_get("raw_request_json").map_err(AppError::db)?,
        raw_response_json: row.try_get("raw_response_json").map_err(AppError::db)?,
        is_pinned: pinned != 0,
        created_at: row.try_get("created_at").map_err(AppError::db)?,
    })
}

pub async fn integrity_check(pool: &SqlitePool) -> Result<Vec<String>, AppError> {
    let rows = sqlx::query("PRAGMA integrity_check")
        .fetch_all(pool)
        .await
        .map_err(AppError::db)?;
    let mut results = Vec::new();
    for row in rows {
        let val: String = row.try_get(0).map_err(AppError::db)?;
        results.push(val);
    }
    Ok(results)
}

pub async fn safe_backup(pool: &SqlitePool, target_path: &Path) -> Result<(), AppError> {
    if let Some(parent) = target_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::storage(format!("无法创建备份目标目录: {e}")))?;
    }
    if target_path.exists() {
        tokio::fs::remove_file(target_path)
            .await
            .map_err(|e| AppError::storage(format!("无法清理已存在的旧备份文件: {e}")))?;
    }

    // 1. 先执行 TRUNCATE checkpoint，将 WAL 日志页面合并入主库
    let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(pool)
        .await;

    // 2. 通过 VACUUM INTO 制作合并后的只读一致快照
    let target_str = target_path.to_string_lossy().replace('\'', "''");
    let vacuum_sql = format!("VACUUM INTO '{target_str}'");
    sqlx::raw_sql(&vacuum_sql)
        .execute(pool)
        .await
        .map_err(|e| AppError::storage(format!("安全数据库快照备份失败: {e}")))?;

    // 3. 对新生成的备份文件执行完整性校验
    verify_backup_file(target_path).await?;
    Ok(())
}

pub async fn verify_backup_file(backup_path: &Path) -> Result<(), AppError> {
    if !backup_path.exists() {
        return Err(AppError::not_found("备份文件不存在"));
    }
    let pool = open(backup_path).await?;
    let checks = integrity_check(&pool).await?;
    if checks.len() != 1 || checks[0] != "ok" {
        return Err(AppError::storage(format!(
            "备份文件完整性检验失败: {:?}",
            checks
        )));
    }
    let fk_rows = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&pool)
        .await
        .map_err(AppError::db)?;
    if !fk_rows.is_empty() {
        return Err(AppError::storage("备份文件存在外键约束违规"));
    }
    Ok(())
}

pub async fn sync_fts_entry(
    pool: &SqlitePool,
    entity_id: &str,
    entity_type: &str,
    title: &str,
    body: &str,
) -> Result<(), AppError> {
    let search_text = format!(
        "{} {}",
        crate::services::search::cjk_segment(title),
        crate::services::search::cjk_segment(body)
    );
    sqlx::query(
        "DELETE FROM fts_entries WHERE entity_id = ?;
         INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
         VALUES (?, ?, ?, ?, ?);",
    )
    .bind(entity_id)
    .bind(entity_id)
    .bind(entity_type)
    .bind(title)
    .bind(body)
    .bind(&search_text)
    .execute(pool)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

pub async fn claim_task_id(
    pool: &SqlitePool,
    raw_id: Option<String>,
    modality: &str,
    model_id: Option<&str>,
) -> Result<String, AppError> {
    let id = match raw_id {
        Some(s) if !s.trim().is_empty() => {
            let trimmed = s.trim();
            crate::storage_util::validate_task_id(trimmed)?;
            trimmed.to_string()
        }
        _ => uuid::Uuid::new_v4().to_string(),
    };

    let now = crate::domain::now_iso();
    // Keep the claimed row readable by history views before the queue has
    // persisted the full request. Batch and pipeline inputs may be replaced
    // later, but they must start from the same schema as normal tasks.
    let input_json = serde_json::json!({
        "kind": modality,
        "prompt": "",
        "providerName": "",
        "modelName": "",
        "modelApiId": ""
    })
    .to_string();
    // 使用 PRIMARY KEY 约束原子抢占认领 ID，杜绝并发竞争与重复写入
    let res = sqlx::query(
        "INSERT INTO generation_tasks
         (id, modality, provider_id, model_id, status, input_json, parameters_json, created_at)
         SELECT ?, ?, '', ?, 'claimed', ?, '{}', ?
         WHERE NOT EXISTS (
           SELECT 1 FROM deleted_generation_tasks WHERE id = ?
         )",
    )
    .bind(&id)
    .bind(modality)
    .bind(model_id.unwrap_or(""))
    .bind(&input_json)
    .bind(&now)
    .bind(&id)
    .execute(pool)
    .await;

    match res {
        Ok(result) if result.rows_affected() == 0 => Err(AppError::conflict(format!(
            "任务 ID '{id}' 已被删除，不能重复提交"
        ))),
        Ok(_) => Ok(id),
        Err(sqlx::Error::Database(ref db_err)) if db_err.is_unique_violation() => Err(
            AppError::conflict(format!("任务 ID '{id}' 已存在，不能重复提交")),
        ),
        Err(err) => Err(AppError::db(err)),
    }
}

pub async fn is_comfyui_model(pool: &SqlitePool, model_id: &str) -> bool {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT p.type 
         FROM models m 
         JOIN providers p ON p.id = m.provider_id 
         WHERE m.id = ? LIMIT 1",
    )
    .bind(model_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    matches!(row.as_ref().map(|(t,)| t.as_str()), Some("comfyui"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_safe_backup_and_integrity_check() {
        let temp_dir = std::env::temp_dir().join(format!("ai-wb-test-db-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let db_path = temp_dir.join("live.db");
        let pool = open(&db_path).await.unwrap();
        migrate(&pool).await.unwrap();

        // 写入测试数据
        let provider = ProviderRecord {
            id: "test-prov-1".to_string(),
            name: "Test Provider".to_string(),
            provider_type: ProviderType::Openai,
            base_url: "https://api.openai.com/v1".to_string(),
            credential_id: None,
            headers: std::collections::BTreeMap::new(),
            timeout_ms: 30000,
            enabled: true,
            created_at: now_iso(),
            updated_at: now_iso(),
        };
        insert_provider(&pool, &provider).await.unwrap();

        // 执行 WAL 安全快照备份
        let backup_path = temp_dir.join("backup.db");
        safe_backup(&pool, &backup_path).await.unwrap();

        // 验证备份文件通过完整性检查
        verify_backup_file(&backup_path).await.unwrap();

        // 验证备份中能读取到刚才写入的数据
        let backup_pool = open(&backup_path).await.unwrap();
        let retrieved = get_provider(&backup_pool, "test-prov-1").await.unwrap();
        assert_eq!(retrieved.name, "Test Provider");

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }

    #[tokio::test]
    async fn test_verify_corrupted_backup_rejected() {
        let temp_dir =
            std::env::temp_dir().join(format!("ai-wb-test-db-bad-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let bad_path = temp_dir.join("bad.db");
        // 写入非合法的伪造数据
        tokio::fs::write(&bad_path, b"not a valid sqlite database header at all")
            .await
            .unwrap();

        let err = verify_backup_file(&bad_path).await.unwrap_err();
        assert!(
            err.message.contains("完整性检验失败")
                || err.message.contains("数据库")
                || err.message.contains("database")
        );

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
