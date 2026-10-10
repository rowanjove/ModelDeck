use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetLibraryItem {
    pub id: String,
    pub task_id: String,
    pub asset_type: String, // "image", "video", "audio"
    pub path: Option<String>,
    pub url: Option<String>,
    pub mime_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u32>,
    pub prompt: String,
    pub model_id: String,
    pub model_name: String,
    pub provider_name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedSearchItem {
    pub id: String,
    pub category: String, // "conversation" | "asset" | "task" | "model" | "pipeline"
    pub title: String,
    pub snippet: String,
    pub modality: Option<String>,
    pub path: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedSearchOutput {
    pub items: Vec<UnifiedSearchItem>,
    pub total: usize,
}

pub fn is_cjk_char(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}' |
        '\u{3400}'..='\u{4DBF}' |
        '\u{F900}'..='\u{FAFF}' |
        '\u{3040}'..='\u{309F}' |
        '\u{30A0}'..='\u{30FF}' |
        '\u{AC00}'..='\u{D7AF}'
    )
}

/// 将 CJK 字符逐字拆分空格，保留连续非 CJK 单词与数字
pub fn cjk_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    let mut prev_is_cjk = false;
    for ch in input.chars() {
        let is_cjk = is_cjk_char(ch);
        if is_cjk {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push(ch);
            out.push(' ');
            prev_is_cjk = true;
        } else {
            if prev_is_cjk && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push(ch);
            prev_is_cjk = false;
        }
    }
    out
}

/// 格式化用户搜索词为安全的 SQLite FTS5 MATCH 表达式
pub fn format_fts5_query(query: &str) -> String {
    let clean = query.trim();
    if clean.is_empty() {
        return String::new();
    }

    let mut terms = Vec::new();
    for word in clean.split_whitespace() {
        let has_cjk = word.chars().any(is_cjk_char);
        if has_cjk {
            let sanitized_raw: String = word
                .chars()
                .filter(|c| c.is_alphanumeric() || is_cjk_char(*c) || *c == '_' || *c == '-')
                .collect();
            let seg = cjk_segment(word);
            let cjk_tokens: Vec<&str> = seg.split_whitespace().collect();
            if cjk_tokens.len() <= 1 {
                if !sanitized_raw.is_empty() {
                    terms.push(format!("\"{}\"*", sanitized_raw));
                }
            } else if !sanitized_raw.is_empty() {
                terms.push(format!(
                    "(\"{}\"* OR \"{}\")",
                    sanitized_raw,
                    cjk_tokens.join(" ")
                ));
            } else {
                terms.push(format!("\"{}\"", cjk_tokens.join(" ")));
            }
        } else {
            let sanitized: String = word
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if !sanitized.is_empty() {
                // Quote the whole token so punctuation such as `gpt-4o` is
                // treated as a literal FTS term rather than a column or
                // boolean operator expression.
                terms.push(format!("\"{}\"*", sanitized.replace('"', "\"\"")));
            }
        }
    }

    terms.join(" AND ")
}

pub async fn list_asset_library(
    pool: &SqlitePool,
    asset_type: Option<String>,
    query: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<AssetLibraryItem>, AppError> {
    let limit = limit.unwrap_or(50).clamp(1, 200);
    let offset = offset.unwrap_or(0);

    let mut clauses = Vec::new();
    let mut binds: Vec<String> = Vec::new();

    if let Some(t) = asset_type.filter(|s| !s.trim().is_empty() && s != "all") {
        clauses.push("a.type = ?");
        binds.push(t.trim().to_string());
    }

    if let Some(q) = query.filter(|s| !s.trim().is_empty()) {
        let fts_q = format_fts5_query(q.trim());
        if !fts_q.is_empty() {
            clauses.push("(t.id IN (SELECT entity_id FROM fts_entries WHERE entity_type = 'task' AND (search_text MATCH ? OR body MATCH ?)) OR t.input_json LIKE ? OR p.name LIKE ? OR m.name LIKE ? OR a.path LIKE ?)");
            binds.push(fts_q.clone());
            binds.push(fts_q);
            let pattern = format!("%{}%", q.trim());
            binds.push(pattern.clone());
            binds.push(pattern.clone());
            binds.push(pattern.clone());
            binds.push(pattern);
        } else {
            clauses
                .push("(t.input_json LIKE ? OR p.name LIKE ? OR m.name LIKE ? OR a.path LIKE ?)");
            let pattern = format!("%{}%", q.trim());
            binds.push(pattern.clone());
            binds.push(pattern.clone());
            binds.push(pattern.clone());
            binds.push(pattern);
        }
    }

    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };

    let sql = format!(
        "SELECT a.id, a.task_id, a.type, a.path, a.url, a.mime_type, a.width, a.height, a.duration_ms, a.created_at,
                t.input_json, t.model_id,
                p.name AS provider_name, m.name AS model_name
         FROM assets a
         JOIN generation_tasks t ON t.id = a.task_id
         LEFT JOIN providers p ON p.id = t.provider_id
         LEFT JOIN models m ON m.id = t.model_id
         {where_sql}
         ORDER BY a.created_at DESC
         LIMIT ? OFFSET ?"
    );

    let mut q = sqlx::query(&sql);
    for b in binds {
        q = q.bind(b);
    }
    q = q.bind(i64::from(limit)).bind(i64::from(offset));

    let rows = q.fetch_all(pool).await.map_err(AppError::db)?;
    let mut items = Vec::with_capacity(rows.len());

    for row in rows {
        let input_json_str: String = row.try_get("input_json").unwrap_or_default();
        let prompt = serde_json::from_str::<serde_json::Value>(&input_json_str)
            .ok()
            .and_then(|v| {
                v.get("prompt")
                    .and_then(|p| p.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();

        let width = row
            .try_get::<Option<i64>, _>("width")
            .ok()
            .flatten()
            .map(|w| w as u32);
        let height = row
            .try_get::<Option<i64>, _>("height")
            .ok()
            .flatten()
            .map(|h| h as u32);
        let duration_ms = row
            .try_get::<Option<i64>, _>("duration_ms")
            .ok()
            .flatten()
            .map(|d| d as u32);

        items.push(AssetLibraryItem {
            id: row.try_get("id").map_err(AppError::db)?,
            task_id: row.try_get("task_id").map_err(AppError::db)?,
            asset_type: row.try_get("type").map_err(AppError::db)?,
            path: row.try_get("path").ok(),
            url: row.try_get("url").ok(),
            mime_type: row.try_get("mime_type").ok(),
            width,
            height,
            duration_ms,
            prompt,
            model_id: row.try_get("model_id").unwrap_or_default(),
            model_name: row
                .try_get::<Option<String>, _>("model_name")
                .unwrap_or_default()
                .unwrap_or_else(|| "默认模型".to_string()),
            provider_name: row
                .try_get::<Option<String>, _>("provider_name")
                .unwrap_or_default()
                .unwrap_or_else(|| "本地服务".to_string()),
            created_at: row.try_get("created_at").map_err(AppError::db)?,
        });
    }

    Ok(items)
}

pub async fn search_unified(
    pool: &SqlitePool,
    query_str: &str,
    category: Option<String>,
    modality: Option<String>,
    limit: Option<u32>,
) -> Result<UnifiedSearchOutput, AppError> {
    let clean_query = query_str.trim();
    if clean_query.is_empty() {
        return Ok(UnifiedSearchOutput {
            items: Vec::new(),
            total: 0,
        });
    }

    let limit = limit.unwrap_or(30).clamp(1, 100);
    let pattern = format!("%{}%", clean_query);
    let fts_query = format_fts5_query(clean_query);
    let mut items = Vec::new();

    let target_cat = category.as_deref().unwrap_or("all");

    // 1. Search Conversations
    if target_cat == "all" || target_cat == "conversation" {
        let conv_sql = if !fts_query.is_empty() {
            r#"
                SELECT c.id, c.title, c.created_at,
                       COALESCE((SELECT content_json FROM messages WHERE conversation_id = c.id AND id IN (
                           SELECT entity_id FROM fts_entries WHERE entity_type = 'message' AND (search_text MATCH ? OR body MATCH ?)
                       ) ORDER BY created_at DESC LIMIT 1), c.title) AS match_snippet
                FROM conversations c
                WHERE c.id IN (
                    SELECT entity_id FROM fts_entries WHERE entity_type = 'conversation' AND (search_text MATCH ? OR body MATCH ?)
                ) OR c.id IN (
                    SELECT conversation_id FROM messages WHERE id IN (
                        SELECT entity_id FROM fts_entries WHERE entity_type = 'message' AND (search_text MATCH ? OR body MATCH ?)
                    )
                ) OR c.title LIKE ? OR EXISTS (
                    SELECT 1 FROM messages WHERE conversation_id = c.id AND content_json LIKE ?
                )
                ORDER BY c.created_at DESC
                LIMIT ?
            "#
        } else {
            r#"
                SELECT c.id, c.title, c.created_at,
                       COALESCE((SELECT content_json FROM messages WHERE conversation_id = c.id AND content_json LIKE ? ORDER BY created_at DESC LIMIT 1), c.title) AS match_snippet
                FROM conversations c
                WHERE c.title LIKE ? OR EXISTS (
                    SELECT 1 FROM messages WHERE conversation_id = c.id AND content_json LIKE ?
                )
                ORDER BY c.created_at DESC
                LIMIT ?
            "#
        };

        let mut q = sqlx::query(conv_sql);
        if !fts_query.is_empty() {
            q = q
                .bind(&fts_query)
                .bind(&fts_query)
                .bind(&fts_query)
                .bind(&fts_query)
                .bind(&fts_query)
                .bind(&fts_query)
                .bind(&pattern)
                .bind(&pattern)
                .bind(i64::from(limit));
        } else {
            q = q
                .bind(&pattern)
                .bind(&pattern)
                .bind(&pattern)
                .bind(i64::from(limit));
        }

        let rows = q.fetch_all(pool).await.map_err(AppError::db)?;
        for r in rows {
            let id: String = r.try_get("id").unwrap_or_default();
            let title: String = r.try_get("title").unwrap_or_else(|_| "对话".to_string());
            let snippet: String = r.try_get("match_snippet").unwrap_or_default();
            let created_at: String = r.try_get("created_at").unwrap_or_default();

            items.push(UnifiedSearchItem {
                id,
                category: "conversation".to_string(),
                title,
                snippet: if snippet.chars().count() > 120 {
                    format!("{}...", snippet.chars().take(120).collect::<String>())
                } else {
                    snippet
                },
                modality: Some("text".to_string()),
                path: None,
                metadata: None,
                created_at,
            });
        }
    }

    // 2. Search Media Assets (Images, Videos, Audios)
    if target_cat == "all" || target_cat == "asset" {
        let asset_sql = if !fts_query.is_empty() {
            r#"
                SELECT a.id, a.type, a.path, a.mime_type, a.created_at, t.input_json, m.name AS model_name
                FROM assets a
                JOIN generation_tasks t ON t.id = a.task_id
                LEFT JOIN models m ON m.id = t.model_id
                WHERE t.id IN (
                    SELECT entity_id FROM fts_entries WHERE entity_type = 'task' AND (search_text MATCH ? OR body MATCH ?)
                ) OR a.id IN (
                    SELECT entity_id FROM fts_entries WHERE entity_type = 'asset' AND (search_text MATCH ? OR body MATCH ?)
                ) OR t.input_json LIKE ? OR a.path LIKE ? OR m.name LIKE ?
                ORDER BY a.created_at DESC
                LIMIT ?
            "#
        } else {
            r#"
                SELECT a.id, a.type, a.path, a.mime_type, a.created_at, t.input_json, m.name AS model_name
                FROM assets a
                JOIN generation_tasks t ON t.id = a.task_id
                LEFT JOIN models m ON m.id = t.model_id
                WHERE t.input_json LIKE ? OR a.path LIKE ? OR m.name LIKE ?
                ORDER BY a.created_at DESC
                LIMIT ?
            "#
        };

        let mut q = sqlx::query(asset_sql);
        if !fts_query.is_empty() {
            q = q
                .bind(&fts_query)
                .bind(&fts_query)
                .bind(&fts_query)
                .bind(&fts_query)
                .bind(&pattern)
                .bind(&pattern)
                .bind(&pattern)
                .bind(i64::from(limit));
        } else {
            q = q
                .bind(&pattern)
                .bind(&pattern)
                .bind(&pattern)
                .bind(i64::from(limit));
        }

        let rows = q.fetch_all(pool).await.map_err(AppError::db)?;
        for r in rows {
            let id: String = r.try_get("id").unwrap_or_default();
            let a_type: String = r.try_get("type").unwrap_or_else(|_| "media".to_string());
            let path: Option<String> = r.try_get("path").ok();
            let created_at: String = r.try_get("created_at").unwrap_or_default();
            let input_json_str: String = r.try_get("input_json").unwrap_or_default();
            let prompt = serde_json::from_str::<serde_json::Value>(&input_json_str)
                .ok()
                .and_then(|v| {
                    v.get("prompt")
                        .and_then(|p| p.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| "媒体资产".to_string());
            let model_name: Option<String> = r.try_get("model_name").ok();

            items.push(UnifiedSearchItem {
                id,
                category: "asset".to_string(),
                title: format!("[{}] {}", a_type.to_uppercase(), prompt),
                snippet: format!(
                    "模型: {} | 路径: {}",
                    model_name.unwrap_or_default(),
                    path.as_deref().unwrap_or("-")
                ),
                modality: Some(a_type),
                path,
                metadata: None,
                created_at,
            });
        }
    }

    // 3. Search Models
    if target_cat == "all" || target_cat == "model" {
        let model_sql = r#"
            SELECT m.id, m.name, m.model_id, m.modality, m.created_at, p.name AS provider_name
            FROM models m
            LEFT JOIN providers p ON p.id = m.provider_id
            WHERE m.name LIKE ? OR m.model_id LIKE ? OR p.name LIKE ?
            ORDER BY m.created_at DESC
            LIMIT ?
        "#;

        let rows = sqlx::query(model_sql)
            .bind(&pattern)
            .bind(&pattern)
            .bind(&pattern)
            .bind(i64::from(limit))
            .fetch_all(pool)
            .await
            .map_err(AppError::db)?;
        for r in rows {
            let id: String = r.try_get("id").unwrap_or_default();
            let name: String = r.try_get("name").unwrap_or_default();
            let model_id: String = r.try_get("model_id").unwrap_or_default();
            let mod_str: String = r.try_get("modality").unwrap_or_default();
            let created_at: String = r.try_get("created_at").unwrap_or_default();
            let provider_name: Option<String> = r.try_get("provider_name").ok();

            items.push(UnifiedSearchItem {
                id,
                category: "model".to_string(),
                title: name,
                snippet: format!(
                    "厂商: {} | 模型 ID: {}",
                    provider_name.unwrap_or_default(),
                    model_id
                ),
                modality: Some(mod_str),
                path: None,
                metadata: None,
                created_at,
            });
        }
    }

    // 4. Search Pipelines / Workflows
    if target_cat == "all" || target_cat == "pipeline" {
        let pipe_sql = if !fts_query.is_empty() {
            r#"
                SELECT id, name, description, created_at
                FROM pipelines
                WHERE id IN (
                    SELECT entity_id FROM fts_entries WHERE entity_type = 'pipeline' AND (search_text MATCH ? OR body MATCH ?)
                ) OR name LIKE ? OR description LIKE ?
                ORDER BY created_at DESC
                LIMIT ?
            "#
        } else {
            r#"
                SELECT id, name, description, created_at
                FROM pipelines
                WHERE name LIKE ? OR description LIKE ?
                ORDER BY created_at DESC
                LIMIT ?
            "#
        };

        let mut q = sqlx::query(pipe_sql);
        if !fts_query.is_empty() {
            q = q
                .bind(&fts_query)
                .bind(&fts_query)
                .bind(&pattern)
                .bind(&pattern)
                .bind(i64::from(limit));
        } else {
            q = q.bind(&pattern).bind(&pattern).bind(i64::from(limit));
        }

        let rows = q.fetch_all(pool).await.map_err(AppError::db)?;
        for r in rows {
            let id: String = r.try_get("id").unwrap_or_default();
            let name: String = r.try_get("name").unwrap_or_default();
            let desc: String = r.try_get("description").unwrap_or_default();
            let created_at: String = r.try_get("created_at").unwrap_or_default();

            items.push(UnifiedSearchItem {
                id,
                category: "pipeline".to_string(),
                title: format!("[工作流] {}", name),
                snippet: desc,
                modality: Some("workflow".to_string()),
                path: None,
                metadata: None,
                created_at,
            });
        }
    }

    // Optional modality filter
    if let Some(m) = modality.filter(|s| !s.trim().is_empty() && s != "all") {
        items.retain(|i| i.modality.as_deref() == Some(m.as_str()));
    }

    let total = items.len();
    Ok(UnifiedSearchOutput { items, total })
}

/// 重建 SQLite FTS5 全文索引，支持老数据迁移、灾难恢复与 CJK 词切分重建
pub async fn rebuild_index(pool: &SqlitePool) -> Result<(), AppError> {
    let mut tx = pool.begin().await.map_err(AppError::db)?;

    sqlx::query("DELETE FROM fts_entries")
        .execute(&mut *tx)
        .await
        .map_err(AppError::db)?;

    // 1. Conversations
    let convs = sqlx::query("SELECT id, title FROM conversations")
        .fetch_all(&mut *tx)
        .await
        .map_err(AppError::db)?;
    for r in convs {
        let id: String = r.try_get("id").unwrap_or_default();
        let title: String = r.try_get("title").unwrap_or_default();
        let search_text = cjk_segment(&title);
        sqlx::query("INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text) VALUES (?, 'conversation', ?, ?, ?)")
            .bind(&id)
            .bind(&title)
            .bind(&title)
            .bind(&search_text)
            .execute(&mut *tx)
            .await
            .map_err(AppError::db)?;
    }

    // 2. Messages
    let msgs = sqlx::query("SELECT id, role, content_json FROM messages")
        .fetch_all(&mut *tx)
        .await
        .map_err(AppError::db)?;
    for r in msgs {
        let id: String = r.try_get("id").unwrap_or_default();
        let role: String = r.try_get("role").unwrap_or_default();
        let body: String = r.try_get("content_json").unwrap_or_default();
        let search_text = cjk_segment(&body);
        sqlx::query("INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text) VALUES (?, 'message', ?, ?, ?)")
            .bind(&id)
            .bind(&role)
            .bind(&body)
            .bind(&search_text)
            .execute(&mut *tx)
            .await
            .map_err(AppError::db)?;
    }

    // 3. Generation Tasks
    let tasks = sqlx::query("SELECT id, modality, input_json, error FROM generation_tasks")
        .fetch_all(&mut *tx)
        .await
        .map_err(AppError::db)?;
    for r in tasks {
        let id: String = r.try_get("id").unwrap_or_default();
        let modality: String = r.try_get("modality").unwrap_or_default();
        let body: String = r.try_get("input_json").unwrap_or_default();
        let err: String = r
            .try_get::<Option<String>, _>("error")
            .ok()
            .flatten()
            .unwrap_or_default();
        let search_text = format!("{} {}", cjk_segment(&body), cjk_segment(&err));
        sqlx::query("INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text) VALUES (?, 'task', ?, ?, ?)")
            .bind(&id)
            .bind(&modality)
            .bind(&body)
            .bind(&search_text)
            .execute(&mut *tx)
            .await
            .map_err(AppError::db)?;
    }

    // 4. Agent Memories
    let memories = sqlx::query("SELECT id, key, content FROM agent_memories")
        .fetch_all(&mut *tx)
        .await
        .map_err(AppError::db)?;
    for r in memories {
        let id: String = r.try_get("id").unwrap_or_default();
        let key: String = r.try_get("key").unwrap_or_default();
        let content: String = r.try_get("content").unwrap_or_default();
        let search_text = format!("{} {}", cjk_segment(&key), cjk_segment(&content));
        sqlx::query("INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text) VALUES (?, 'memory', ?, ?, ?)")
            .bind(&id)
            .bind(&key)
            .bind(&content)
            .bind(&search_text)
            .execute(&mut *tx)
            .await
            .map_err(AppError::db)?;
    }

    // 5. Pipelines
    let pipelines = sqlx::query("SELECT id, name, description FROM pipelines")
        .fetch_all(&mut *tx)
        .await
        .map_err(AppError::db)?;
    for r in pipelines {
        let id: String = r.try_get("id").unwrap_or_default();
        let name: String = r.try_get("name").unwrap_or_default();
        let desc: String = r.try_get("description").unwrap_or_default();
        let search_text = format!("{} {}", cjk_segment(&name), cjk_segment(&desc));
        sqlx::query("INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text) VALUES (?, 'pipeline', ?, ?, ?)")
            .bind(&id)
            .bind(&name)
            .bind(&desc)
            .bind(&search_text)
            .execute(&mut *tx)
            .await
            .map_err(AppError::db)?;
    }

    // 6. Assets
    let assets = sqlx::query("SELECT id, type, path FROM assets")
        .fetch_all(&mut *tx)
        .await
        .map_err(AppError::db)?;
    for r in assets {
        let id: String = r.try_get("id").unwrap_or_default();
        let a_type: String = r.try_get("type").unwrap_or_default();
        let path: String = r
            .try_get::<Option<String>, _>("path")
            .ok()
            .flatten()
            .unwrap_or_default();
        let search_text = cjk_segment(&path);
        sqlx::query("INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text) VALUES (?, 'asset', ?, ?, ?)")
            .bind(&id)
            .bind(&a_type)
            .bind(&path)
            .bind(&search_text)
            .execute(&mut *tx)
            .await
            .map_err(AppError::db)?;
    }

    tx.commit().await.map_err(AppError::db)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cjk_segment_and_fts5() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE VIRTUAL TABLE fts_test_cjk USING fts5(
                entity_id UNINDEXED,
                entity_type UNINDEXED,
                title,
                body,
                search_text,
                tokenize = 'unicode61'
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        let title = "量子计算前沿探讨";
        let body = "请解释超导量子比特与拓扑量子计算的区别，以及在 Cyberpunk 风格中的应用";
        let search_text = format!("{} {}", cjk_segment(title), cjk_segment(body));

        sqlx::query(
            "INSERT INTO fts_test_cjk(entity_id, entity_type, title, body, search_text)
             VALUES ('conv-1', 'conversation', ?, ?, ?)",
        )
        .bind(title)
        .bind(body)
        .bind(&search_text)
        .execute(&pool)
        .await
        .unwrap();

        // 1. Search sub-word "量子"
        let q1 = format_fts5_query("量子");
        assert_eq!(q1, "(\"量子\"* OR \"量 子\")");
        let count1: (i64,) = sqlx::query_as(&format!(
            "SELECT count(*) FROM fts_test_cjk WHERE search_text MATCH '{}'",
            q1
        ))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count1.0, 1, "Should match '量子'");

        // 2. Search sub-word "拓扑" (in the middle of body)
        let q2 = format_fts5_query("拓扑");
        assert_eq!(q2, "(\"拓扑\"* OR \"拓 扑\")");
        let count2: (i64,) = sqlx::query_as(&format!(
            "SELECT count(*) FROM fts_test_cjk WHERE search_text MATCH '{}'",
            q2
        ))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count2.0, 1, "Should match '拓扑'");

        // 3. Search English "Cyberpunk" prefix
        let q3 = format_fts5_query("Cyber");
        assert_eq!(q3, "\"Cyber\"*");
        let count3: (i64,) = sqlx::query_as(&format!(
            "SELECT count(*) FROM fts_test_cjk WHERE search_text MATCH '{}'",
            q3
        ))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count3.0, 1, "Should match 'Cyber*'");

        // 4. Mixed query "拓扑 Cyberpunk"
        let q4 = format_fts5_query("拓扑 Cyber");
        assert_eq!(q4, "(\"拓扑\"* OR \"拓 扑\") AND \"Cyber\"*");
        let count4: (i64,) = sqlx::query_as(&format!(
            "SELECT count(*) FROM fts_test_cjk WHERE search_text MATCH '{}'",
            q4
        ))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count4.0, 1, "Should match mixed '拓扑 AND Cyber*'");

        // 5. Query that does NOT match
        let q5 = format_fts5_query("火星探测");
        let count5: (i64,) = sqlx::query_as(&format!(
            "SELECT count(*) FROM fts_test_cjk WHERE search_text MATCH '{}'",
            q5
        ))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count5.0, 0, "Should not match '火星探测'");

        let hyphen = format_fts5_query("gpt-4o");
        assert_eq!(hyphen, "\"gpt-4o\"*");
        let count_hyphen: (i64,) =
            sqlx::query_as("SELECT count(*) FROM fts_test_cjk WHERE search_text MATCH ?")
                .bind(&hyphen)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count_hyphen.0, 0);
    }

    #[tokio::test]
    async fn test_rebuild_index_and_cascade_triggers() {
        let dir = std::env::temp_dir().join(format!("ai_test_fts_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&dir);
        let db_path = dir.join("test_fts.db");
        let pool = crate::db::open(&db_path).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();

        // 1. Insert conversation and verify automatic trigger sync to FTS
        let conv_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO conversations (id, title, mode, model_id, system_prompt, context_strategy, context_limit, created_at, updated_at)
             VALUES (?, '深度强化学习与机器人控制', 'chat', 'model-1', '', 'sliding_window', 10, '2026-10-01', '2026-10-01')"
        )
        .bind(&conv_id)
        .execute(&pool)
        .await
        .unwrap();

        let count: (i64,) = sqlx::query_as("SELECT count(*) FROM fts_entries WHERE entity_id = ?")
            .bind(&conv_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            count.0, 1,
            "Trigger should automatically sync new conversation to fts_entries"
        );

        // 2. Rebuild index
        rebuild_index(&pool).await.unwrap();

        // 3. Search via unified search
        let res = search_unified(&pool, "强化学习", None, None, None)
            .await
            .unwrap();
        assert!(
            !res.items.is_empty(),
            "Unified search should find conversation by sub-word"
        );
        assert_eq!(res.items[0].id, conv_id);

        // 4. Delete conversation and verify trigger automatically deletes FTS entry
        crate::db::delete_conversation(&pool, &conv_id)
            .await
            .unwrap();
        let count_after: (i64,) =
            sqlx::query_as("SELECT count(*) FROM fts_entries WHERE entity_id = ?")
                .bind(&conv_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            count_after.0, 0,
            "Trigger should automatically delete FTS entry on conversation delete"
        );
        drop(pool);
        let _ = std::fs::remove_dir_all(dir);
    }
}
