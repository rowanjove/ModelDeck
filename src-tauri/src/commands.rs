use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::State;

use crate::canonical::{
    AudioResult, ConnectionReport, ImageResult, RemoteModel, TextResult, VideoResult,
};
use crate::domain::{
    ChatSendInput, ConversationUpdate, GenerateAudioInput, GenerateImageInput, GenerateTextInput,
    GenerateVideoInput, Modality, ModelDefinition, ModelInput, Preset, PresetInput, Provider,
    ProviderInput, Voice, VoiceInput, VoiceUpdate,
};
use crate::error::AppError;
use crate::services;
use crate::services::chat::{ChatNotice, ChatTurnResult, Conversation, ConversationDetail};
use crate::services::history::{HistoryDetail, HistoryPage, HistoryQuery};
use crate::services::mcp::{McpServer, McpServerInput, McpToolInfo};
use crate::state::AppState;

#[tauri::command]
pub async fn providers_list(state: State<'_, AppState>) -> Result<Vec<Provider>, AppError> {
    services::providers::list(&state.db).await
}

#[tauri::command]
pub async fn providers_create(
    state: State<'_, AppState>,
    input: ProviderInput,
) -> Result<Provider, AppError> {
    services::providers::create(&state.db, state.secrets.as_ref(), input).await
}

#[tauri::command]
pub async fn providers_update(
    state: State<'_, AppState>,
    id: String,
    input: ProviderInput,
) -> Result<Provider, AppError> {
    services::providers::update(&state.db, state.secrets.as_ref(), &id, input).await
}

#[tauri::command]
pub async fn providers_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    services::providers::delete(&state.db, state.secrets.as_ref(), &id).await
}

#[tauri::command]
pub async fn providers_test(
    state: State<'_, AppState>,
    id: String,
) -> Result<ConnectionReport, AppError> {
    let db = state.db.clone();
    let http = state.http();
    let secrets = Arc::clone(&state.secrets);
    services::providers::test(&db, &http, secrets.as_ref(), &id).await
}

#[tauri::command]
pub async fn providers_list_remote_models(
    state: State<'_, AppState>,
    id: String,
    models_path: Option<String>,
) -> Result<Vec<RemoteModel>, AppError> {
    let db = state.db.clone();
    let http = state.http();
    let secrets = Arc::clone(&state.secrets);
    services::providers::list_remote_models(&db, &http, secrets.as_ref(), &id, models_path).await
}

#[tauri::command]
pub async fn models_list(state: State<'_, AppState>) -> Result<Vec<ModelDefinition>, AppError> {
    services::models::list(&state.db).await
}

#[tauri::command]
pub async fn models_create(
    state: State<'_, AppState>,
    input: ModelInput,
) -> Result<ModelDefinition, AppError> {
    services::models::create(&state.db, input).await
}

#[tauri::command]
pub async fn models_update(
    state: State<'_, AppState>,
    id: String,
    input: ModelInput,
) -> Result<ModelDefinition, AppError> {
    services::models::update(&state.db, &id, input).await
}

#[tauri::command]
pub async fn models_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    services::models::delete(&state.db, &id).await
}

#[tauri::command]
pub async fn generate_text(
    state: State<'_, AppState>,
    input: GenerateTextInput,
) -> Result<TextResult, AppError> {
    let _permit = state.scheduler.acquire(Modality::Text, false).await?;
    let db = state.db.clone();
    let http = state.http();
    let secrets = Arc::clone(&state.secrets);
    services::generate::execute(&db, &http, secrets.as_ref(), &state.data_dir, input).await
}

#[tauri::command]
pub async fn generate_image(
    state: State<'_, AppState>,
    input: GenerateImageInput,
    task_id: Option<String>,
) -> Result<ImageResult, AppError> {
    let (task_id, flag) =
        claim_direct_task(&state, task_id, "image", Some(&input.model_definition_id)).await?;
    let is_comfyui = crate::db::is_comfyui_model(&state.db, &input.model_definition_id).await;
    let permit = state
        .scheduler
        .acquire_cancellable(Modality::Image, is_comfyui, Some(&flag))
        .await;
    let _permit = match permit {
        Ok(permit) => permit,
        Err(err) => {
            state.unregister_cancel(&task_id, &flag);
            finish_claimed_task(&state.db, &task_id, &err).await?;
            return Err(err);
        }
    };
    if flag.load(std::sync::atomic::Ordering::Relaxed) {
        let err = AppError::new("cancelled", "任务已取消");
        state.unregister_cancel(&task_id, &flag);
        finish_claimed_task(&state.db, &task_id, &err).await?;
        return Err(err);
    }
    let db = state.db.clone();
    let http = state.http();
    let secrets = Arc::clone(&state.secrets);
    let result = services::image::execute_with_cancel(
        &db,
        &http,
        secrets.as_ref(),
        &state.data_dir,
        input,
        Some(task_id.clone()),
        Some(Arc::clone(&flag)),
    )
    .await;
    state.unregister_cancel(&task_id, &flag);
    result
}

fn validate_media_read_path(path: &str, data_dir: &Path) -> Result<(), AppError> {
    let p = Path::new(path);
    let path_str = path.to_ascii_lowercase();
    if path_str.contains("/.ssh")
        || path_str.contains("\\.ssh")
        || path_str.contains("/windows/")
        || path_str.contains("\\windows\\")
        || path_str.contains("/etc/shadow")
        || path_str.contains("/etc/passwd")
    {
        return Err(AppError::validation(
            "安全策略拦截：禁止访问系统保护或凭据目录",
        ));
    }
    if let Ok(canon_file) = std::fs::canonicalize(p) {
        let canon_dd = std::fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());
        if canon_file.starts_with(&canon_dd) {
            let allowed_subdirs = ["images", "videos", "audio", "files"];
            let is_in_allowed = allowed_subdirs
                .iter()
                .any(|sub| canon_file.starts_with(canon_dd.join(sub)));
            if !is_in_allowed {
                return Err(AppError::validation(
                    "安全策略拦截：禁止直接访问应用非媒体数据文件",
                ));
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn read_image_file(
    state: State<'_, AppState>,
    path: String,
) -> Result<services::image::ImageFile, AppError> {
    validate_media_read_path(&path, &state.data_dir)?;
    services::image::read_file(&path).await
}

#[tauri::command]
pub async fn generate_video(
    state: State<'_, AppState>,
    input: GenerateVideoInput,
    task_id: Option<String>,
) -> Result<VideoResult, AppError> {
    let (task_id, flag) =
        claim_direct_task(&state, task_id, "video", Some(&input.model_definition_id)).await?;
    let is_comfyui = crate::db::is_comfyui_model(&state.db, &input.model_definition_id).await;
    let permit = state
        .scheduler
        .acquire_cancellable(Modality::Video, is_comfyui, Some(&flag))
        .await;
    let _permit = match permit {
        Ok(permit) => permit,
        Err(err) => {
            state.unregister_cancel(&task_id, &flag);
            finish_claimed_task(&state.db, &task_id, &err).await?;
            return Err(err);
        }
    };
    if flag.load(std::sync::atomic::Ordering::Relaxed) {
        let err = AppError::new("cancelled", "任务已取消");
        state.unregister_cancel(&task_id, &flag);
        finish_claimed_task(&state.db, &task_id, &err).await?;
        return Err(err);
    }
    let db = state.db.clone();
    let http = state.http();
    let secrets = Arc::clone(&state.secrets);
    let result = services::video::execute_with_cancel(
        &db,
        &http,
        secrets.as_ref(),
        &state.data_dir,
        input,
        Some(task_id.clone()),
        Some(Arc::clone(&flag)),
    )
    .await;
    state.unregister_cancel(&task_id, &flag);
    result
}

#[tauri::command]
pub async fn read_video_file(
    state: State<'_, AppState>,
    path: String,
) -> Result<services::video::VideoFile, AppError> {
    validate_media_read_path(&path, &state.data_dir)?;
    services::video::read_file(&path).await
}

#[tauri::command]
pub async fn generate_audio(
    state: State<'_, AppState>,
    input: GenerateAudioInput,
    task_id: Option<String>,
) -> Result<AudioResult, AppError> {
    let (task_id, flag) =
        claim_direct_task(&state, task_id, "audio", Some(&input.model_definition_id)).await?;
    let permit = state
        .scheduler
        .acquire_cancellable(Modality::Audio, false, Some(&flag))
        .await;
    let _permit = match permit {
        Ok(permit) => permit,
        Err(err) => {
            state.unregister_cancel(&task_id, &flag);
            finish_claimed_task(&state.db, &task_id, &err).await?;
            return Err(err);
        }
    };
    if flag.load(std::sync::atomic::Ordering::Relaxed) {
        let err = AppError::new("cancelled", "任务已取消");
        state.unregister_cancel(&task_id, &flag);
        finish_claimed_task(&state.db, &task_id, &err).await?;
        return Err(err);
    }
    let db = state.db.clone();
    let http = state.http();
    let secrets = Arc::clone(&state.secrets);
    let result = services::audio::execute_with_cancel(
        &db,
        &http,
        secrets.as_ref(),
        &state.data_dir,
        input,
        Some(task_id.clone()),
        Some(Arc::clone(&flag)),
    )
    .await;
    state.unregister_cancel(&task_id, &flag);
    result
}

async fn finish_claimed_task(
    db: &sqlx::SqlitePool,
    task_id: &str,
    err: &AppError,
) -> Result<(), AppError> {
    let status = if err.code == "cancelled" {
        "cancelled"
    } else {
        "failed"
    };
    let now = crate::domain::now_iso();
    sqlx::query(
        "UPDATE generation_tasks SET status = ?, error = ?, completed_at = ?
         WHERE id = ? AND status = 'claimed'",
    )
    .bind(status)
    .bind(&err.message)
    .bind(now)
    .bind(task_id)
    .execute(db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

async fn claim_direct_task(
    state: &AppState,
    requested_id: Option<String>,
    modality: &str,
    model_id: Option<&str>,
) -> Result<(String, Arc<std::sync::atomic::AtomicBool>), AppError> {
    let id = match requested_id {
        Some(id) if !id.trim().is_empty() => id.trim().to_string(),
        _ => uuid::Uuid::new_v4().to_string(),
    };
    crate::storage_util::validate_task_id(&id)?;
    // Install cancellation state before the database await so cancel can win
    // even while the claimed row is being inserted.
    let flag = state.register_cancel(&id)?;
    match crate::db::claim_task_id(&state.db, Some(id.clone()), modality, model_id).await {
        Ok(claimed_id) => Ok((claimed_id, flag)),
        Err(err) => {
            state.unregister_cancel(&id, &flag);
            Err(err)
        }
    }
}

#[tauri::command]
pub async fn read_audio_file(
    state: State<'_, AppState>,
    path: String,
) -> Result<services::audio::AudioFile, AppError> {
    validate_media_read_path(&path, &state.data_dir)?;
    services::audio::read_file(&path).await
}

#[tauri::command]
pub async fn voices_list(state: State<'_, AppState>) -> Result<Vec<Voice>, AppError> {
    services::voices::list(&state.db).await
}

#[tauri::command]
pub async fn voices_create(
    state: State<'_, AppState>,
    input: VoiceInput,
) -> Result<Voice, AppError> {
    services::voices::create(&state.db, &state.data_dir, input).await
}

#[tauri::command]
pub async fn voices_update(
    state: State<'_, AppState>,
    id: String,
    input: VoiceUpdate,
) -> Result<Voice, AppError> {
    services::voices::update(&state.db, &state.data_dir, &id, input).await
}

#[tauri::command]
pub async fn voices_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    services::voices::delete(&state.db, &state.data_dir, &id).await
}

#[tauri::command]
pub async fn presets_list(
    state: State<'_, AppState>,
    model_id: String,
) -> Result<Vec<Preset>, AppError> {
    services::presets::list(&state.db, &model_id).await
}

#[tauri::command]
pub async fn presets_create(
    state: State<'_, AppState>,
    input: PresetInput,
) -> Result<Preset, AppError> {
    services::presets::create(&state.db, input).await
}

#[tauri::command]
pub async fn presets_update(
    state: State<'_, AppState>,
    id: String,
    input: PresetInput,
) -> Result<Preset, AppError> {
    services::presets::update(&state.db, &id, input).await
}

#[tauri::command]
pub async fn presets_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    services::presets::delete(&state.db, &id).await
}

#[tauri::command]
pub async fn mcp_servers_list(state: State<'_, AppState>) -> Result<Vec<McpServer>, AppError> {
    services::mcp::list(&state.db).await
}

#[tauri::command]
pub async fn mcp_servers_create(
    state: State<'_, AppState>,
    input: McpServerInput,
) -> Result<McpServer, AppError> {
    services::mcp::create(&state.db, input).await
}

#[tauri::command]
pub async fn mcp_servers_update(
    state: State<'_, AppState>,
    id: String,
    input: McpServerInput,
) -> Result<McpServer, AppError> {
    services::mcp::update(&state.db, &id, input).await
}

#[tauri::command]
pub async fn mcp_servers_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    services::mcp::delete(&state.db, &id).await
}

#[tauri::command]
pub async fn mcp_servers_tools(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<McpToolInfo>, AppError> {
    services::mcp::tool_info(&state.db, &state.http(), &id).await
}

#[tauri::command]
pub async fn conversations_list(state: State<'_, AppState>) -> Result<Vec<Conversation>, AppError> {
    services::chat::list(&state.db).await
}

#[tauri::command]
pub async fn conversations_get(
    state: State<'_, AppState>,
    id: String,
) -> Result<ConversationDetail, AppError> {
    services::chat::get(&state.db, &id).await
}

#[tauri::command]
pub async fn conversations_update(
    state: State<'_, AppState>,
    id: String,
    input: ConversationUpdate,
) -> Result<Conversation, AppError> {
    services::chat::update(&state.db, &id, input).await
}

#[tauri::command]
pub async fn conversations_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    services::chat::delete(&state.db, &state.cancels, &id).await
}

#[tauri::command]
pub async fn messages_delete(
    state: State<'_, AppState>,
    conversation_id: String,
    message_id: String,
) -> Result<ConversationDetail, AppError> {
    services::chat::delete_message(&state.db, &state.cancels, &conversation_id, &message_id).await
}

#[tauri::command]
pub async fn chat_cancel(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<(), AppError> {
    services::chat::cancel(&state.cancels, &conversation_id);
    Ok(())
}

#[tauri::command]
pub async fn chat_send(
    state: State<'_, AppState>,
    input: ChatSendInput,
    on_event: Channel<ChatNotice>,
) -> Result<ChatTurnResult, AppError> {
    let db = state.db.clone();
    let http = state.http();
    let secrets = Arc::clone(&state.secrets);
    services::chat::send_with_scheduler(
        &db,
        &http,
        secrets.as_ref(),
        &state.cancels,
        &state.data_dir,
        input,
        &move |notice| on_event.send(notice).is_ok(),
        Some(&state.scheduler),
    )
    .await
}

#[tauri::command]
pub async fn history_list(
    state: State<'_, AppState>,
    query: HistoryQuery,
) -> Result<HistoryPage, AppError> {
    services::history::list(&state.db, query).await
}

#[tauri::command]
pub async fn history_get(
    state: State<'_, AppState>,
    id: String,
) -> Result<HistoryDetail, AppError> {
    services::history::get(&state.db, &id).await
}

#[tauri::command]
pub async fn history_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    let ids = [id.clone()];
    state.job_queue.cancel_tasks(&ids);
    services::history::delete_task(&state.db, &id).await?;
    state.job_queue.forget_deleted_tasks(&ids);
    Ok(())
}

#[tauri::command]
pub async fn history_delete_batch(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<(), AppError> {
    state.job_queue.cancel_tasks(&ids);
    services::history::delete_tasks_batch(&state.db, &ids).await?;
    state.job_queue.forget_deleted_tasks(&ids);
    Ok(())
}

#[tauri::command]
pub async fn history_delete_all(
    state: State<'_, AppState>,
    modality: Option<String>,
) -> Result<(), AppError> {
    let ids: Vec<String> = match modality.as_deref() {
        Some(value) if !value.is_empty() => {
            sqlx::query_scalar("SELECT id FROM generation_tasks WHERE modality = ?")
                .bind(value)
                .fetch_all(&state.db)
                .await
                .map_err(AppError::db)?
        }
        _ => sqlx::query_scalar("SELECT id FROM generation_tasks")
            .fetch_all(&state.db)
            .await
            .map_err(AppError::db)?,
    };
    state.job_queue.cancel_tasks(&ids);
    services::history::delete_tasks_batch(&state.db, &ids).await?;
    state.job_queue.forget_deleted_tasks(&ids);
    Ok(())
}

#[tauri::command]
pub async fn save_text_file(
    state: State<'_, AppState>,
    path: String,
    content: String,
) -> Result<(), AppError> {
    if content.len() > 5_000_000 {
        return Err(AppError::validation("内容过大"));
    }
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(AppError::validation("保存路径必须是绝对路径"));
    }
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        let ext_lower = ext.to_ascii_lowercase();
        if matches!(
            ext_lower.as_str(),
            "exe"
                | "bat"
                | "cmd"
                | "vbs"
                | "ps1"
                | "msi"
                | "dll"
                | "com"
                | "scr"
                | "pif"
                | "reg"
                | "sys"
        ) {
            return Err(AppError::validation(
                "不允许保存为可执行脚本或系统文件扩展名",
            ));
        }
    }
    let path_str = path.to_string_lossy().to_ascii_lowercase();
    if path_str.contains("\\windows\\")
        || path_str.contains("/windows/")
        || path_str.contains("start menu\\programs\\startup")
        || path_str.contains("start menu/programs/startup")
        || path_str.contains("/.ssh")
        || path_str.contains("\\.ssh")
        || path_str.contains("/etc/shadow")
        || path_str.contains("/etc/passwd")
    {
        return Err(AppError::validation("禁止向系统保护目录保存文件"));
    }
    let Some(parent) = path.parent() else {
        return Err(AppError::validation("保存路径无效"));
    };
    if !parent.exists() {
        return Err(AppError::validation("保存目录不存在"));
    }
    let data_dir_canon =
        std::fs::canonicalize(&state.data_dir).unwrap_or_else(|_| state.data_dir.clone());
    if let Ok(canon_parent) = std::fs::canonicalize(parent) {
        if canon_parent.starts_with(&data_dir_canon) {
            return Err(AppError::validation("禁止直接覆盖应用数据目录文件"));
        }
    }
    tokio::fs::write(path, content)
        .await
        .map_err(|err| AppError::storage(format!("无法保存文件: {err}")))?;
    Ok(())
}

// 1. Model Registry 在线更新
#[tauri::command]
pub async fn registry_get(
    state: State<'_, AppState>,
) -> Result<services::registry::RegistryCatalog, AppError> {
    Ok(services::registry::get_catalog(&state.data_dir))
}

#[tauri::command]
pub async fn registry_check_update(
    state: State<'_, AppState>,
    url: Option<String>,
) -> Result<services::registry::RegistryUpdateCheck, AppError> {
    services::registry::check_update(&state.data_dir, &state.http(), url).await
}

#[tauri::command]
pub async fn registry_apply_update(
    state: State<'_, AppState>,
    url: Option<String>,
) -> Result<services::registry::RegistryCatalog, AppError> {
    services::registry::apply_update(&state.data_dir, &state.http(), url).await
}

#[tauri::command]
pub async fn registry_import_model(
    state: State<'_, AppState>,
    template_id: String,
    provider_id: String,
    overwrite: Option<bool>,
) -> Result<ModelDefinition, AppError> {
    services::registry::import_model_from_registry(
        &state.db,
        &state.data_dir,
        &template_id,
        &provider_id,
        overwrite,
    )
    .await
}

#[tauri::command]
pub async fn registry_rollback(
    state: State<'_, AppState>,
) -> Result<services::registry::RegistryCatalog, AppError> {
    services::registry::rollback_registry(&state.data_dir)
}

// 2. Provider Plugin
#[tauri::command]
pub async fn plugins_list(
    state: State<'_, AppState>,
) -> Result<Vec<services::plugins::ProviderPlugin>, AppError> {
    services::plugins::list(&state.data_dir)
}

#[tauri::command]
pub async fn plugins_install(
    state: State<'_, AppState>,
    manifest_json: String,
) -> Result<services::plugins::ProviderPlugin, AppError> {
    services::plugins::install(&state.data_dir, &manifest_json)
}

#[tauri::command]
pub async fn plugins_uninstall(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    services::plugins::uninstall(&state.data_dir, &id)
}

#[tauri::command]
pub async fn plugins_instantiate(
    state: State<'_, AppState>,
    plugin_id: String,
    api_key: Option<String>,
) -> Result<Provider, AppError> {
    services::plugins::instantiate(
        &state.db,
        state.secrets.as_ref(),
        &state.data_dir,
        &plugin_id,
        api_key,
    )
    .await
}

#[tauri::command]
pub async fn plugins_validate(
    manifest_json: String,
) -> Result<services::plugins::PluginValidationReport, AppError> {
    Ok(services::plugins::validate_manifest(&manifest_json))
}

#[tauri::command]
pub async fn plugins_test_recipe(
    recipe: crate::domain::HttpRecipe,
    sample_response: String,
) -> Result<services::plugins::TestAdapterResult, AppError> {
    services::plugins::test_recipe(&recipe, &sample_response)
}

#[tauri::command]
pub async fn plugins_get_sdk_template() -> Result<services::plugins::SdkTemplateInfo, AppError> {
    Ok(services::plugins::get_sdk_template())
}

// 3. Agent Memory
#[tauri::command]
pub async fn agent_memories_list(
    state: State<'_, AppState>,
    conversation_id: Option<String>,
) -> Result<Vec<services::memory::AgentMemory>, AppError> {
    services::memory::list(&state.db, conversation_id.as_deref()).await
}

#[tauri::command]
pub async fn agent_memories_save(
    state: State<'_, AppState>,
    input: services::memory::AgentMemoryInput,
) -> Result<services::memory::AgentMemory, AppError> {
    services::memory::save(&state.db, input).await
}

#[tauri::command]
pub async fn agent_memories_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    services::memory::delete(&state.db, &id).await
}

// 4. Context Compaction & Pinning
#[tauri::command]
pub async fn messages_toggle_pin(state: State<'_, AppState>, id: String) -> Result<bool, AppError> {
    use sqlx::Row;
    let row = sqlx::query("SELECT is_pinned FROM messages WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?;
    let Some(row) = row else {
        return Err(AppError::not_found("找不到消息"));
    };
    let current: i64 = row.try_get("is_pinned").unwrap_or(0);
    let next = if current == 0 { 1 } else { 0 };
    sqlx::query("UPDATE messages SET is_pinned = ? WHERE id = ?")
        .bind(next)
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    Ok(next == 1)
}

#[tauri::command]
pub async fn conversations_compact(
    state: State<'_, AppState>,
    id: String,
) -> Result<ConversationDetail, AppError> {
    let detail = services::chat::get(&state.db, &id).await?;
    if detail.messages.len() <= 2 {
        return Ok(detail);
    }
    // 保留最新的 2 条消息作为活跃上下文，其余未 pin 的历史消息压缩为摘要
    let keep_count = 2;
    let split_idx = detail.messages.len().saturating_sub(keep_count);
    let older_unpinned: Vec<_> = detail.messages[..split_idx]
        .iter()
        .filter(|m| !m.is_pinned)
        .collect();

    if older_unpinned.is_empty() {
        return Ok(detail);
    }

    let summary_until = older_unpinned
        .last()
        .map(|m| m.id.clone())
        .unwrap_or_default();
    let mut summary_lines = Vec::new();
    for msg in &older_unpinned {
        summary_lines.push(format!("{}: {}", msg.role, msg.text));
    }
    let summary_text = summary_lines.join("\n");
    let compressed: String = summary_text.chars().take(1000).collect();
    let now = crate::domain::now_iso();
    sqlx::query(
        "UPDATE conversations
         SET summary = ?, summary_until = ?, context_strategy = 'summary', updated_at = ?
         WHERE id = ?",
    )
    .bind(format!("【压缩对话历史】\n{compressed}"))
    .bind(&summary_until)
    .bind(&now)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    services::chat::get(&state.db, &id).await
}

// 5. 模型成本统计
#[tauri::command]
pub async fn analytics_get_summary(
    state: State<'_, AppState>,
    time_range: Option<String>,
) -> Result<services::analytics::AnalyticsSummary, AppError> {
    services::analytics::get_summary_filtered(&state.db, time_range).await
}

#[tauri::command]
pub async fn analytics_set_pricing(
    state: State<'_, AppState>,
    model_id: String,
    input_per_m: f64,
    output_per_m: f64,
) -> Result<(), AppError> {
    services::analytics::set_pricing(&state.db, &model_id, input_per_m, output_per_m).await
}

#[tauri::command]
pub async fn analytics_list_pricing(
    state: State<'_, AppState>,
) -> Result<Vec<services::analytics::ModelPricing>, AppError> {
    services::analytics::list_pricing(&state.db).await
}

// 6. 多模型 Compare
#[tauri::command]
pub async fn compare_text(
    state: State<'_, AppState>,
    input: services::compare::CompareInput,
) -> Result<services::compare::CompareResult, AppError> {
    services::compare::execute(
        &state.db,
        &state.http(),
        state.secrets.as_ref(),
        &state.data_dir,
        input,
    )
    .await
}

// 7. Batch Generation
#[tauri::command]
pub async fn batch_generate(
    state: State<'_, AppState>,
    input: services::batch::BatchGenerateInput,
) -> Result<services::batch::BatchGenerateResult, AppError> {
    services::batch::execute(
        &state.db,
        &state.http(),
        state.secrets.as_ref(),
        &state.data_dir,
        input,
        None,
        Some(Arc::clone(&state.scheduler)),
    )
    .await
}

// 8. 本地服务发现 (Local Service Discovery)
#[tauri::command]
pub async fn discovery_scan_local(
    state: State<'_, AppState>,
) -> Result<Vec<services::discovery::DiscoveredService>, AppError> {
    services::discovery::discover_local_services(&state.db, &state.http()).await
}

#[tauri::command]
pub async fn discovery_add_service(
    state: State<'_, AppState>,
    service_id: String,
) -> Result<Provider, AppError> {
    services::discovery::add_discovered_service(&state.db, &service_id).await
}

// 9. 全局配置导出与导入 (Config Import / Export)
#[tauri::command]
pub async fn config_export_all(
    state: State<'_, AppState>,
) -> Result<services::config_io::ExportedConfig, AppError> {
    services::config_io::export_all_config(&state.db).await
}

#[tauri::command]
pub async fn config_import_all(
    state: State<'_, AppState>,
    config: services::config_io::ExportedConfig,
) -> Result<services::config_io::ImportResult, AppError> {
    services::config_io::import_config(&state.db, state.secrets.as_ref(), config).await
}

// 10. 网络代理设置 (Network Proxy)
#[tauri::command]
pub async fn network_get_proxy(
    state: State<'_, AppState>,
) -> Result<services::network::ProxySettings, AppError> {
    services::network::get_proxy_settings(&state.db).await
}

#[tauri::command]
pub async fn network_set_proxy(
    state: State<'_, AppState>,
    settings: services::network::ProxySettings,
) -> Result<services::network::ProxySettings, AppError> {
    let new_client = services::network::build_client_with_proxy(&settings)?;
    let saved = services::network::set_proxy_settings(&state.db, settings).await?;
    state.update_http(new_client.clone());
    state.job_queue.update_http(new_client);
    Ok(saved)
}

#[tauri::command]
pub async fn network_test_proxy(
    settings: services::network::ProxySettings,
) -> Result<u64, AppError> {
    services::network::test_proxy_connection(&settings).await
}

// 11. 任务取消与生命周期管理 (Tasks)
#[tauri::command]
pub async fn tasks_cancel(state: State<'_, AppState>, id: String) -> Result<bool, AppError> {
    let triggered_mem = state.trigger_cancel(&id);
    let cancelled_db = services::history::mark_task_cancelled(&state.db, &id).await?;
    if cancelled_db {
        state.job_queue.mark_cancelled(&id).await;
    }
    Ok(triggered_mem || cancelled_db)
}

#[tauri::command]
pub async fn tasks_submit_image(
    state: State<'_, AppState>,
    input: GenerateImageInput,
    task_id: Option<String>,
) -> Result<String, AppError> {
    state.job_queue.submit_image(input, task_id).await
}

#[tauri::command]
pub async fn tasks_submit_video(
    state: State<'_, AppState>,
    input: GenerateVideoInput,
    task_id: Option<String>,
) -> Result<String, AppError> {
    state.job_queue.submit_video(input, task_id).await
}

#[tauri::command]
pub async fn tasks_submit_audio(
    state: State<'_, AppState>,
    input: GenerateAudioInput,
    task_id: Option<String>,
) -> Result<String, AppError> {
    state.job_queue.submit_audio(input, task_id).await
}

#[tauri::command]
pub async fn tasks_submit_batch(
    state: State<'_, AppState>,
    input: services::batch::BatchGenerateInput,
    task_id: Option<String>,
) -> Result<String, AppError> {
    state.job_queue.submit_batch(input, task_id).await
}

#[tauri::command]
pub async fn tasks_submit_pipeline(
    state: State<'_, AppState>,
    input: services::pipeline::PipelineExecutionInput,
    task_id: Option<String>,
) -> Result<String, AppError> {
    state.job_queue.submit_pipeline(input, task_id).await
}

#[tauri::command]
pub async fn tasks_get_status(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<Option<services::job_queue::TaskInfo>, AppError> {
    Ok(state.job_queue.get_task_info(&task_id))
}

#[tauri::command]
pub async fn tasks_list_active(
    state: State<'_, AppState>,
) -> Result<Vec<services::job_queue::TaskInfo>, AppError> {
    Ok(state.job_queue.list_active_tasks())
}

// 12. ComfyUI 队列管理与中断 (ComfyUI Queue & Control)
#[tauri::command]
pub async fn comfyui_get_queue(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<services::comfyui::ComfyQueueStatus, AppError> {
    services::comfyui::get_queue(
        &state.db,
        &state.http(),
        state.secrets.as_ref(),
        &provider_id,
    )
    .await
}

#[tauri::command]
pub async fn comfyui_interrupt(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<bool, AppError> {
    services::comfyui::interrupt(
        &state.db,
        &state.http(),
        state.secrets.as_ref(),
        &provider_id,
    )
    .await
}

#[tauri::command]
pub async fn comfyui_clear_queue(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<bool, AppError> {
    services::comfyui::clear_queue(
        &state.db,
        &state.http(),
        state.secrets.as_ref(),
        &provider_id,
    )
    .await
}

// 13. Tool Permissions (Safe, Confirm, Restricted)
#[tauri::command]
pub async fn tool_permissions_list(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, String>, AppError> {
    services::tool_permissions::list(&state.db).await
}

#[tauri::command]
pub async fn tool_permissions_set(
    state: State<'_, AppState>,
    tool_name: String,
    level: String,
) -> Result<(), AppError> {
    services::tool_permissions::set(&state.db, &tool_name, &level).await
}

// 14. Automation / Workflow Pipeline
#[tauri::command]
pub async fn pipelines_list(
    state: State<'_, AppState>,
) -> Result<Vec<services::pipeline::PipelineDefinition>, AppError> {
    services::pipeline::list(&state.db).await
}

#[tauri::command]
pub async fn pipelines_get(
    state: State<'_, AppState>,
    id: String,
) -> Result<services::pipeline::PipelineDefinition, AppError> {
    services::pipeline::get(&state.db, &id).await
}

#[tauri::command]
pub async fn pipelines_save(
    state: State<'_, AppState>,
    definition: services::pipeline::PipelineDefinition,
) -> Result<services::pipeline::PipelineDefinition, AppError> {
    services::pipeline::save(&state.db, definition).await
}

#[tauri::command]
pub async fn pipelines_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    services::pipeline::delete(&state.db, &id).await
}

#[tauri::command]
pub async fn pipelines_presets() -> Result<Vec<services::pipeline::PipelineDefinition>, AppError> {
    Ok(services::pipeline::presets())
}

#[tauri::command]
pub async fn pipelines_validate(
    definition: services::pipeline::PipelineDefinition,
) -> Result<(), AppError> {
    services::pipeline::validate_pipeline_definition(&definition)
}

#[tauri::command]
pub async fn pipelines_export(
    definition: services::pipeline::PipelineDefinition,
) -> Result<String, AppError> {
    services::pipeline::export_pipeline_json(&definition)
}

#[tauri::command]
pub async fn pipelines_import(
    json_str: String,
) -> Result<services::pipeline::PipelineDefinition, AppError> {
    services::pipeline::import_pipeline_json(&json_str)
}

#[tauri::command]
pub async fn pipelines_run(
    state: State<'_, AppState>,
    input: services::pipeline::PipelineExecutionInput,
    task_id: Option<String>,
) -> Result<services::pipeline::PipelineExecutionResult, AppError> {
    let task_id = state.job_queue.submit_pipeline(input, task_id).await?;
    let wait = async {
        loop {
            if let Some(task) = state.job_queue.get_task_info(&task_id) {
                match task.status.as_str() {
                    "completed" => {
                        let output = task
                            .output
                            .ok_or_else(|| AppError::storage("工作流已完成，但没有可读取的结果"))?;
                        return serde_json::from_value(output).map_err(|err| {
                            AppError::storage(format!("工作流结果格式无效: {err}"))
                        });
                    }
                    "failed" | "interrupted" => {
                        return Err(AppError::storage(
                            task.error.unwrap_or_else(|| "工作流执行失败".to_string()),
                        ));
                    }
                    "cancelled" => return Err(AppError::new("cancelled", "工作流已取消")),
                    _ => {}
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    };
    match tokio::time::timeout(std::time::Duration::from_secs(620), wait).await {
        Ok(result) => result,
        Err(_) => {
            state.job_queue.cancel(&task_id);
            if services::history::mark_task_cancelled(&state.db, &task_id).await? {
                state.job_queue.mark_cancelled(&task_id).await;
            }
            Err(AppError::new(
                "TimeoutError",
                "工作流等待结果超时，任务已取消",
            ))
        }
    }
}

// 15. Developer Mode (AI API Debugger & Postman)
#[tauri::command]
pub async fn developer_send_request(
    state: State<'_, AppState>,
    input: services::developer::DevRequestInput,
) -> Result<services::developer::DevResponseOutput, AppError> {
    services::developer::send_request(&state.http(), state.secrets.as_ref(), &state.db, input).await
}

#[tauri::command]
pub async fn developer_list_presets(
    state: State<'_, AppState>,
) -> Result<Vec<services::developer::ApiTestPreset>, AppError> {
    services::developer::list_presets(&state.db).await
}

#[tauri::command]
pub async fn developer_save_preset(
    state: State<'_, AppState>,
    preset: services::developer::ApiTestPreset,
) -> Result<services::developer::ApiTestPreset, AppError> {
    services::developer::save_preset(&state.db, preset).await
}

#[tauri::command]
pub async fn developer_delete_preset(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    services::developer::delete_preset(&state.db, &id).await
}

// 16. Unified Search & Asset Library (Phase 14)
#[tauri::command]
pub async fn search_unified(
    state: State<'_, AppState>,
    query: String,
    category: Option<String>,
    modality: Option<String>,
    limit: Option<u32>,
) -> Result<services::search::UnifiedSearchOutput, AppError> {
    services::search::search_unified(&state.db, &query, category, modality, limit).await
}

#[tauri::command]
pub async fn assets_list(
    state: State<'_, AppState>,
    asset_type: Option<String>,
    query: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<services::search::AssetLibraryItem>, AppError> {
    services::search::list_asset_library(&state.db, asset_type, query, limit, offset).await
}

#[tauri::command]
pub async fn search_rebuild_index(state: State<'_, AppState>) -> Result<(), AppError> {
    services::search::rebuild_index(&state.db).await
}

// 17. Hardening: Storage Maintenance & DB Integrity Check
#[tauri::command]
pub async fn storage_inspect(
    state: State<'_, AppState>,
) -> Result<services::storage_maintenance::StorageReport, AppError> {
    services::storage_maintenance::inspect_storage(&state.data_dir, &state.db).await
}

#[tauri::command]
pub async fn storage_clean(
    state: State<'_, AppState>,
    files: Vec<String>,
) -> Result<services::storage_maintenance::CleanReport, AppError> {
    services::storage_maintenance::clean_storage(&state.data_dir, &state.db, &files).await
}

#[tauri::command]
pub async fn db_integrity_check(state: State<'_, AppState>) -> Result<Vec<String>, AppError> {
    crate::db::integrity_check(&state.db).await
}

#[tauri::command]
pub async fn db_backup(state: State<'_, AppState>, target_path: String) -> Result<(), AppError> {
    let path = std::path::PathBuf::from(&target_path);
    if let Some(parent) = path.parent() {
        crate::storage_util::ensure_dir_writable(parent)?;
    }
    crate::db::safe_backup(&state.db, &path).await
}

#[tauri::command]
pub async fn db_verify_backup(backup_path: String) -> Result<(), AppError> {
    let path = std::path::PathBuf::from(&backup_path);
    crate::db::verify_backup_file(&path).await
}

// 18. Hardening: Diagnostics & Diagnostic Bundle Export
#[tauri::command]
pub async fn diagnostics_run(
    state: State<'_, AppState>,
) -> Result<services::diagnostics::SystemDiagnosticsReport, AppError> {
    services::diagnostics::run_diagnostics(&state.data_dir, &state.db).await
}

#[tauri::command]
pub async fn diagnostics_export_bundle(
    state: State<'_, AppState>,
) -> Result<services::diagnostics::DiagnosticBundle, AppError> {
    services::diagnostics::export_diagnostic_bundle(
        &state.data_dir,
        &state.db,
        Some(state.secrets.as_ref()),
    )
    .await
}

// 19. Hardening: Structured Logs & Correlation Query
#[tauri::command]
pub async fn logs_list(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<crate::logging::LogEntry>, AppError> {
    Ok(state.logs.recent(limit.unwrap_or(100)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_media_read_path_rules() {
        let temp_root =
            std::env::temp_dir().join(format!("md-media-test-{}", uuid::Uuid::new_v4()));
        let data_dir = temp_root.join("app_data");
        let images_dir = data_dir.join("images");
        std::fs::create_dir_all(&images_dir).unwrap();

        // 1. 合法应用内媒体子目录
        let sample_img = images_dir.join("avatar.png");
        std::fs::write(&sample_img, b"fake image").unwrap();
        assert!(validate_media_read_path(sample_img.to_str().unwrap(), &data_dir).is_ok());

        // 2. 非媒体数据文件（如数据库）必须被拦截
        let db_file = data_dir.join("modeldeck.db");
        std::fs::write(&db_file, b"sqlite header").unwrap();
        let err_db = validate_media_read_path(db_file.to_str().unwrap(), &data_dir).unwrap_err();
        assert!(err_db.message.contains("禁止直接访问应用非媒体数据文件"));

        // 3. 系统凭据目录必须被拦截
        assert!(validate_media_read_path("C:\\Users\\user\\.ssh\\id_rsa", &data_dir).is_err());
        assert!(validate_media_read_path("/home/user/.ssh/id_rsa", &data_dir).is_err());
        assert!(validate_media_read_path("C:\\Windows\\System32\\cmd.exe", &data_dir).is_err());

        // 4. 外部常规图片文件允许访问
        let outside_dir = temp_root.join("external_photos");
        std::fs::create_dir_all(&outside_dir).unwrap();
        let outside_img = outside_dir.join("photo.jpg");
        std::fs::write(&outside_img, b"fake photo").unwrap();
        assert!(validate_media_read_path(outside_img.to_str().unwrap(), &data_dir).is_ok());

        let _ = std::fs::remove_dir_all(temp_root);
    }
}
