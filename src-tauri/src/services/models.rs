use sqlx::SqlitePool;
use uuid::Uuid;

use crate::db;
use crate::domain::{now_iso, ModelDefinition, ModelInput};
use crate::error::AppError;
use crate::schema::validate_definition;

pub async fn list(pool: &SqlitePool) -> Result<Vec<ModelDefinition>, AppError> {
    db::list_models(pool).await
}

pub async fn create(pool: &SqlitePool, input: ModelInput) -> Result<ModelDefinition, AppError> {
    let model = prepare(None, input)?;
    db::get_provider(pool, &model.provider_id).await?;
    let now = now_iso();
    let model = ModelDefinition {
        id: Uuid::new_v4().to_string(),
        created_at: now.clone(),
        updated_at: now,
        ..model
    };
    db::insert_model(pool, &model).await?;
    Ok(model)
}

pub async fn update(
    pool: &SqlitePool,
    id: &str,
    input: ModelInput,
) -> Result<ModelDefinition, AppError> {
    let existing = db::get_model(pool, id).await?;
    let model = prepare(Some(existing), input)?;
    db::get_provider(pool, &model.provider_id).await?;
    db::update_model(pool, &model).await?;
    Ok(model)
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    db::delete_model(pool, id).await?;
    crate::services::presets::delete_for_model(pool, id).await
}

fn prepare(
    existing: Option<ModelDefinition>,
    mut input: ModelInput,
) -> Result<ModelDefinition, AppError> {
    let name = input.name.trim();
    let model_id = input.model_id.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(AppError::validation("模型名称需要 1 到 120 个字符"));
    }
    if model_id.is_empty() || model_id.chars().count() > 200 {
        return Err(AppError::validation("模型 ID 需要 1 到 200 个字符"));
    }
    if input.provider_id.trim().is_empty() {
        return Err(AppError::validation("请选择 Provider"));
    }
    validate_definition(
        &mut input.parameters,
        &mut input.constraints,
        &mut input.transport,
        &input.capabilities,
    )?;
    let (id, created_at) = existing
        .map(|model| (model.id, model.created_at))
        .unwrap_or_else(|| (String::new(), String::new()));
    Ok(ModelDefinition {
        id,
        name: name.to_string(),
        provider_id: input.provider_id.trim().to_string(),
        model_id: model_id.to_string(),
        modality: input.modality,
        capabilities: input.capabilities,
        parameters: input.parameters,
        constraints: input.constraints,
        transport: input.transport,
        schema_version: 1,
        enabled: input.enabled,
        created_at,
        updated_at: now_iso(),
    })
}
