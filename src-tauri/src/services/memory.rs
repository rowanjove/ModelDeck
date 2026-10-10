use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::domain::now_iso;
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMemory {
    pub id: String,
    pub conversation_id: Option<String>,
    pub scope: String,
    pub key: String,
    pub content: String,
    pub importance: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMemoryInput {
    pub id: Option<String>,
    pub conversation_id: Option<String>,
    pub scope: Option<String>,
    pub key: String,
    pub content: String,
    pub importance: Option<i64>,
}

pub async fn list(
    pool: &SqlitePool,
    conversation_id: Option<&str>,
) -> Result<Vec<AgentMemory>, AppError> {
    let rows = if let Some(cid) = conversation_id {
        sqlx::query(
            "SELECT id, conversation_id, scope, key, content, importance, created_at, updated_at
             FROM agent_memories
             WHERE scope = 'global' OR scope = 'session' OR conversation_id = ?
             ORDER BY importance DESC, updated_at DESC",
        )
        .bind(cid)
        .fetch_all(pool)
        .await
        .map_err(AppError::db)?
    } else {
        sqlx::query(
            "SELECT id, conversation_id, scope, key, content, importance, created_at, updated_at
             FROM agent_memories
             ORDER BY importance DESC, updated_at DESC",
        )
        .fetch_all(pool)
        .await
        .map_err(AppError::db)?
    };

    rows.iter().map(memory_from_row).collect()
}

pub async fn save(pool: &SqlitePool, input: AgentMemoryInput) -> Result<AgentMemory, AppError> {
    let key = input.key.trim().to_string();
    let content = input.content.trim().to_string();
    if key.is_empty() || content.is_empty() {
        return Err(AppError::validation("记忆键与内容不能为空"));
    }
    let scope = input.scope.unwrap_or_else(|| {
        if input.conversation_id.is_some() {
            "conversation".to_string()
        } else {
            "global".to_string()
        }
    });
    let importance = input.importance.unwrap_or(1).clamp(1, 10);
    let now = now_iso();

    if let Some(id) = input.id.filter(|s| !s.trim().is_empty()) {
        sqlx::query(
            "UPDATE agent_memories
             SET key = ?, content = ?, scope = ?, importance = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&key)
        .bind(&content)
        .bind(&scope)
        .bind(importance)
        .bind(&now)
        .bind(&id)
        .execute(pool)
        .await
        .map_err(AppError::db)?;

        let row = sqlx::query(
            "SELECT id, conversation_id, scope, key, content, importance, created_at, updated_at
             FROM agent_memories WHERE id = ?",
        )
        .bind(&id)
        .fetch_one(pool)
        .await
        .map_err(AppError::db)?;

        return memory_from_row(&row);
    }

    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO agent_memories
         (id, conversation_id, scope, key, content, importance, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&input.conversation_id)
    .bind(&scope)
    .bind(&key)
    .bind(&content)
    .bind(importance)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(AppError::db)?;

    Ok(AgentMemory {
        id,
        conversation_id: input.conversation_id,
        scope,
        key,
        content,
        importance,
        created_at: now.clone(),
        updated_at: now,
    })
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM agent_memories WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

pub async fn format_for_prompt(
    pool: &SqlitePool,
    conversation_id: Option<&str>,
) -> Result<String, AppError> {
    let memories = list(pool, conversation_id).await?;
    if memories.is_empty() {
        return Ok(String::new());
    }
    let mut lines = Vec::new();
    lines.push("【Agent 记忆库（长期记忆与用户偏好）】:".to_string());
    for item in memories.iter().take(20) {
        let tag = match item.scope.as_str() {
            "global" => "全局",
            "session" => "会话",
            "conversation" => "当前对话",
            _ => "记忆",
        };
        lines.push(format!("- [{tag}][{}]: {}", item.key, item.content));
    }
    Ok(lines.join("\n"))
}

fn memory_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<AgentMemory, AppError> {
    Ok(AgentMemory {
        id: row.try_get("id").map_err(AppError::db)?,
        conversation_id: row.try_get("conversation_id").map_err(AppError::db)?,
        scope: row.try_get("scope").map_err(AppError::db)?,
        key: row.try_get("key").map_err(AppError::db)?,
        content: row.try_get("content").map_err(AppError::db)?,
        importance: row.try_get("importance").map_err(AppError::db)?,
        created_at: row.try_get("created_at").map_err(AppError::db)?,
        updated_at: row.try_get("updated_at").map_err(AppError::db)?,
    })
}
