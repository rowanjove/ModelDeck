use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolPermissionLevel {
    Safe,
    Confirm,
    Restricted,
}

impl ToolPermissionLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Confirm => "confirm",
            Self::Restricted => "restricted",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "safe" => Self::Safe,
            "restricted" => Self::Restricted,
            _ => Self::Confirm,
        }
    }
}

pub async fn list(pool: &SqlitePool) -> Result<HashMap<String, String>, AppError> {
    let mut map = default_permissions();
    let rows = sqlx::query("SELECT tool_name, level FROM tool_permissions")
        .fetch_all(pool)
        .await
        .map_err(AppError::db)?;

    for row in rows {
        let name: String = row.try_get("tool_name").map_err(AppError::db)?;
        let level: String = row.try_get("level").map_err(AppError::db)?;
        map.insert(name, level);
    }
    Ok(map)
}

pub async fn set(pool: &SqlitePool, tool_name: &str, level: &str) -> Result<(), AppError> {
    let clean_tool = tool_name.trim();
    if clean_tool.is_empty() {
        return Err(AppError::validation("工具名称不能为空"));
    }
    let parsed_level = ToolPermissionLevel::parse(level);
    sqlx::query(
        "INSERT INTO tool_permissions (tool_name, level)
         VALUES (?, ?)
         ON CONFLICT(tool_name) DO UPDATE SET level = excluded.level",
    )
    .bind(clean_tool)
    .bind(parsed_level.as_str())
    .execute(pool)
    .await
    .map_err(AppError::db)?;

    Ok(())
}

pub async fn get_level(
    pool: &SqlitePool,
    tool_name: &str,
) -> Result<ToolPermissionLevel, AppError> {
    let row = sqlx::query("SELECT level FROM tool_permissions WHERE tool_name = ?")
        .bind(tool_name)
        .fetch_optional(pool)
        .await
        .map_err(AppError::db)?;

    if let Some(row) = row {
        if let Ok(lvl) = row.try_get::<String, _>("level") {
            return Ok(ToolPermissionLevel::parse(&lvl));
        }
    }

    Ok(default_level_for(tool_name))
}

pub fn default_level_for(tool_name: &str) -> ToolPermissionLevel {
    match tool_name {
        "current_time" | "read_file" | "list_directory" | "memory" => ToolPermissionLevel::Safe,
        "write_file" | "delete_file" | "run_command" | "http_request" => {
            ToolPermissionLevel::Confirm
        }
        _ => ToolPermissionLevel::Confirm,
    }
}

pub fn default_permissions() -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("current_time".to_string(), "safe".to_string());
    map.insert("read_file".to_string(), "safe".to_string());
    map.insert("list_directory".to_string(), "safe".to_string());
    map.insert("memory".to_string(), "safe".to_string());
    map.insert("write_file".to_string(), "confirm".to_string());
    map.insert("delete_file".to_string(), "confirm".to_string());
    map.insert("run_command".to_string(), "confirm".to_string());
    map.insert("http_request".to_string(), "confirm".to_string());
    map
}
