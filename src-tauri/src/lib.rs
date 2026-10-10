use tauri::Manager;

mod adapters;
mod canonical;
mod commands;
mod credentials;
mod db;
mod domain;
mod error;
mod http_util;
pub mod logging;
mod media;
mod redact;
pub mod resilience;
mod schema;
mod services;
mod sse;
mod state;
pub mod storage_util;
pub mod windows_util;

#[cfg(test)]
mod integration_tests;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    if let Err(_err) = windows_util::ensure_webview2() {
        return;
    }

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let app_state = tauri::async_runtime::block_on(state::AppState::initialize(&dir))?;
            let handle = app.handle().clone();
            let emitter: crate::services::job_queue::EventSender = std::sync::Arc::new(
                move |event_name: &str, payload: &crate::services::job_queue::TaskEvent| {
                    use tauri::Emitter;
                    let _ = handle.emit(event_name, payload);
                },
            );
            app_state.job_queue.set_emitter(emitter);
            app.manage(app_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::providers_list,
            commands::providers_create,
            commands::providers_update,
            commands::providers_delete,
            commands::providers_test,
            commands::providers_list_remote_models,
            commands::models_list,
            commands::models_create,
            commands::models_update,
            commands::models_delete,
            commands::generate_text,
            commands::generate_image,
            commands::read_image_file,
            commands::generate_video,
            commands::read_video_file,
            commands::generate_audio,
            commands::read_audio_file,
            commands::voices_list,
            commands::voices_create,
            commands::voices_update,
            commands::voices_delete,
            commands::presets_list,
            commands::presets_create,
            commands::presets_update,
            commands::presets_delete,
            commands::mcp_servers_list,
            commands::mcp_servers_create,
            commands::mcp_servers_update,
            commands::mcp_servers_delete,
            commands::mcp_servers_tools,
            commands::save_text_file,
            commands::conversations_list,
            commands::conversations_get,
            commands::conversations_update,
            commands::conversations_delete,
            commands::messages_delete,
            commands::chat_send,
            commands::chat_cancel,
            commands::history_list,
            commands::history_get,
            commands::history_delete,
            commands::history_delete_batch,
            commands::history_delete_all,
            commands::registry_get,
            commands::registry_check_update,
            commands::registry_apply_update,
            commands::registry_import_model,
            commands::registry_rollback,
            commands::plugins_list,
            commands::plugins_install,
            commands::plugins_uninstall,
            commands::plugins_instantiate,
            commands::plugins_validate,
            commands::plugins_test_recipe,
            commands::plugins_get_sdk_template,
            commands::agent_memories_list,
            commands::agent_memories_save,
            commands::agent_memories_delete,
            commands::messages_toggle_pin,
            commands::conversations_compact,
            commands::analytics_get_summary,
            commands::analytics_set_pricing,
            commands::analytics_list_pricing,
            commands::compare_text,
            commands::batch_generate,
            commands::discovery_scan_local,
            commands::discovery_add_service,
            commands::config_export_all,
            commands::config_import_all,
            commands::network_get_proxy,
            commands::network_set_proxy,
            commands::network_test_proxy,
            commands::tasks_cancel,
            commands::tasks_submit_image,
            commands::tasks_submit_video,
            commands::tasks_submit_audio,
            commands::tasks_submit_batch,
            commands::tasks_submit_pipeline,
            commands::tasks_get_status,
            commands::tasks_list_active,
            commands::comfyui_get_queue,
            commands::comfyui_interrupt,
            commands::comfyui_clear_queue,
            commands::tool_permissions_list,
            commands::tool_permissions_set,
            commands::pipelines_list,
            commands::pipelines_get,
            commands::pipelines_save,
            commands::pipelines_delete,
            commands::pipelines_presets,
            commands::pipelines_run,
            commands::developer_send_request,
            commands::developer_list_presets,
            commands::developer_save_preset,
            commands::developer_delete_preset,
            commands::search_unified,
            commands::assets_list,
            commands::search_rebuild_index,
            commands::storage_inspect,
            commands::storage_clean,
            commands::db_integrity_check,
            commands::diagnostics_run,
            commands::diagnostics_export_bundle,
            commands::logs_list,
        ]);

    if let Err(err) = builder.run(tauri::generate_context!()) {
        #[cfg(windows)]
        windows_util::show_fatal_error(&format!("ModelDeck 启动发生异常:\n{err}"));
        eprintln!("error while running tauri application: {err}");
    }
}
