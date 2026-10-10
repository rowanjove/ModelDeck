use serde_json::{Map, Value};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::domain::{now_iso, Preset, PresetInput};
use crate::error::AppError;

pub async fn list(pool: &SqlitePool, model_id: &str) -> Result<Vec<Preset>, AppError> {
    let model_id = model_id.trim();
    if model_id.is_empty() {
        return Err(AppError::validation("请选择模型"));
    }
    let rows = sqlx::query(
        "SELECT id, name, model_id, parameters_json, system_prompt, user_template, created_at, updated_at
         FROM presets WHERE model_id = ? ORDER BY name, id",
    )
    .bind(model_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::db)?;
    rows.iter().map(preset_from_row).collect()
}

pub async fn create(pool: &SqlitePool, input: PresetInput) -> Result<Preset, AppError> {
    let name = clean_name(&input.name)?;
    let model_id = input.model_id.trim();
    if model_id.is_empty() {
        return Err(AppError::validation("请选择模型"));
    }
    crate::db::get_model(pool, model_id).await?;
    let parameters = clean_parameters(&input.parameters)?;
    let system_prompt = input.system_prompt.filter(|s| !s.trim().is_empty());
    let user_template = input.user_template.filter(|s| !s.trim().is_empty());
    let id = Uuid::new_v4().to_string();
    let now = now_iso();
    sqlx::query(
        "INSERT INTO presets (id, name, model_id, parameters_json, system_prompt, user_template, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(model_id)
    .bind(json_text(&parameters)?)
    .bind(&system_prompt)
    .bind(&user_template)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(AppError::db)?;
    Ok(Preset {
        id,
        name,
        model_id: model_id.to_string(),
        system_prompt,
        user_template,
        parameters,
        created_at: now.clone(),
        updated_at: now,
        schema_version: 1,
    })
}

pub async fn update(pool: &SqlitePool, id: &str, input: PresetInput) -> Result<Preset, AppError> {
    let current = get(pool, id).await?;
    let name = clean_name(&input.name)?;
    let model_id = input.model_id.trim();
    if model_id.is_empty() {
        return Err(AppError::validation("请选择模型"));
    }
    if model_id != current.model_id {
        crate::db::get_model(pool, model_id).await?;
    }
    let parameters = clean_parameters(&input.parameters)?;
    let system_prompt = input.system_prompt.filter(|s| !s.trim().is_empty());
    let user_template = input.user_template.filter(|s| !s.trim().is_empty());
    let now = now_iso();
    sqlx::query(
        "UPDATE presets SET name = ?, model_id = ?, parameters_json = ?, system_prompt = ?, user_template = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&name)
    .bind(model_id)
    .bind(json_text(&parameters)?)
    .bind(&system_prompt)
    .bind(&user_template)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(AppError::db)?;
    Ok(Preset {
        id: id.to_string(),
        name,
        model_id: model_id.to_string(),
        system_prompt,
        user_template,
        parameters,
        created_at: current.created_at,
        updated_at: now,
        schema_version: 1,
    })
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let result = sqlx::query("DELETE FROM presets WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(AppError::db)?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("找不到预设"));
    }
    Ok(())
}

pub async fn delete_for_model(pool: &SqlitePool, model_id: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM presets WHERE model_id = ?")
        .bind(model_id)
        .execute(pool)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

async fn get(pool: &SqlitePool, id: &str) -> Result<Preset, AppError> {
    let row = sqlx::query(
        "SELECT id, name, model_id, parameters_json, system_prompt, user_template, created_at, updated_at FROM presets WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::db)?
    .ok_or_else(|| AppError::not_found("找不到预设"))?;
    preset_from_row(&row)
}

pub(crate) fn preset_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Preset, AppError> {
    use sqlx::Row;
    let parameters = serde_json::from_str(
        row.try_get::<String, _>("parameters_json")
            .map_err(AppError::db)?
            .as_str(),
    )
    .unwrap_or_else(|_| Map::new());
    Ok(Preset {
        id: row.try_get("id").map_err(AppError::db)?,
        name: row.try_get("name").map_err(AppError::db)?,
        model_id: row.try_get("model_id").map_err(AppError::db)?,
        system_prompt: row.try_get("system_prompt").ok(),
        user_template: row.try_get("user_template").ok(),
        parameters,
        created_at: row.try_get("created_at").map_err(AppError::db)?,
        updated_at: row.try_get("updated_at").map_err(AppError::db)?,
        schema_version: 1,
    })
}

fn clean_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::validation("预设名称需要 1 到 80 个字符"));
    }
    Ok(name.to_string())
}

fn clean_parameters(parameters: &Map<String, Value>) -> Result<Map<String, Value>, AppError> {
    let text = serde_json::to_string(parameters).unwrap_or_default();
    if text.len() > 100_000 {
        return Err(AppError::validation("预设参数过大"));
    }
    Ok(parameters.clone())
}

fn json_text(value: &Map<String, Value>) -> Result<String, AppError> {
    serde_json::to_string(value).map_err(|err| AppError::storage(format!("无法保存预设: {err}")))
}
