use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::path::Path;

use crate::domain::now_iso;
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticItem {
    pub category: String,
    pub name: String,
    pub status: String, // "ok" | "warn" | "error"
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemDiagnosticsReport {
    pub generated_at: String,
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub items: Vec<DiagnosticItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticBundle {
    pub report: SystemDiagnosticsReport,
    pub sanitized_providers: Vec<serde_json::Value>,
    pub storage: crate::services::storage_maintenance::StorageReport,
    pub recent_task_errors: Vec<serde_json::Value>,
    pub recent_logs: Vec<crate::logging::LogEntry>,
}

pub async fn run_diagnostics(
    data_dir: &Path,
    pool: &SqlitePool,
) -> Result<SystemDiagnosticsReport, AppError> {
    let mut items = Vec::new();

    // 1. SQLite 完整性检查
    let integrity_res = crate::db::integrity_check(pool).await;
    match integrity_res {
        Ok(lines) => {
            let is_ok = lines.iter().all(|l| l == "ok");
            items.push(DiagnosticItem {
                category: "数据库".to_string(),
                name: "SQLite Integrity".to_string(),
                status: if is_ok {
                    "ok".to_string()
                } else {
                    "error".to_string()
                },
                detail: if is_ok {
                    "PRAGMA integrity_check 通过".to_string()
                } else {
                    lines.join("; ")
                },
            });
        }
        Err(e) => {
            items.push(DiagnosticItem {
                category: "数据库".to_string(),
                name: "SQLite Integrity".to_string(),
                status: "error".to_string(),
                detail: e.message,
            });
        }
    }

    // 2. Schema Migration 版本
    let max_migration: Option<i64> =
        sqlx::query_scalar("SELECT MAX(version) FROM schema_migrations")
            .fetch_optional(pool)
            .await
            .unwrap_or(None);
    items.push(DiagnosticItem {
        category: "数据库".to_string(),
        name: "Schema Version".to_string(),
        status: "ok".to_string(),
        detail: format!("当前迁移版本: {}", max_migration.unwrap_or(0)),
    });

    // 3. 磁盘空间检查
    #[cfg(windows)]
    {
        if let Some(free_bytes) = crate::storage_util::platform_free_space(data_dir) {
            let free_mb = free_bytes / (1024 * 1024);
            let status = if free_mb > 1000 {
                "ok"
            } else if free_mb > 500 {
                "warn"
            } else {
                "error"
            };
            items.push(DiagnosticItem {
                category: "存储".to_string(),
                name: "磁盘可用空间".to_string(),
                status: status.to_string(),
                detail: format!("剩余可用空间: {} MB (底线安全阈值: 500 MB)", free_mb),
            });
        }
    }

    // 4. 数据目录读写检查
    let test_file = data_dir.join(".diag_test.tmp");
    let rw_ok =
        std::fs::write(&test_file, b"test").is_ok() && std::fs::remove_file(&test_file).is_ok();
    items.push(DiagnosticItem {
        category: "存储".to_string(),
        name: "数据目录权限".to_string(),
        status: if rw_ok {
            "ok".to_string()
        } else {
            "error".to_string()
        },
        detail: format!("路径: {}", data_dir.display()),
    });

    // 5. Provider 与模型统计
    let prov_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM providers WHERE enabled = 1")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let model_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM models WHERE enabled = 1")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    items.push(DiagnosticItem {
        category: "配置".to_string(),
        name: "活跃 Provider 与模型".to_string(),
        status: if prov_count > 0 {
            "ok".to_string()
        } else {
            "warn".to_string()
        },
        detail: format!(
            "启用 Provider: {} 个，启用模型: {} 个",
            prov_count, model_count
        ),
    });

    // 6. 存储孤儿与碎片情况
    let storage_report =
        crate::services::storage_maintenance::inspect_storage(data_dir, pool).await?;
    let _has_temp = !storage_report.temp_files.is_empty();
    let has_missing = !storage_report.missing_files.is_empty();
    let status = if has_missing { "warn" } else { "ok" };
    items.push(DiagnosticItem {
        category: "存储维护".to_string(),
        name: "媒体资产健康度".to_string(),
        status: status.to_string(),
        detail: format!(
            "登记资产: {} 个，磁盘媒体文件: {} 个，孤儿文件: {} 个，临时碎片: {} 个，丢失引用: {} 个",
            storage_report.total_asset_records,
            storage_report.disk_files_count,
            storage_report.orphan_files.len(),
            storage_report.temp_files.len(),
            storage_report.missing_files.len()
        ),
    });

    Ok(SystemDiagnosticsReport {
        generated_at: now_iso(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        items,
    })
}

/// 导出完全脱敏的诊断包（排除所有 Authorization, API Key, 用户 Prompt 与隐私媒体）
pub async fn export_diagnostic_bundle(
    data_dir: &Path,
    pool: &SqlitePool,
    secrets: Option<&dyn crate::credentials::SecretStore>,
) -> Result<DiagnosticBundle, AppError> {
    let report = run_diagnostics(data_dir, pool).await?;
    let storage = crate::services::storage_maintenance::inspect_storage(data_dir, pool).await?;

    // 收集所有已配置 Provider 的真实密钥值（用于精准内容脱敏，绝不让真实密钥泄漏至错误信息或日志中）
    let mut known_secrets: Vec<String> = Vec::new();
    if let Some(store) = secrets {
        let cred_ids: Vec<String> = sqlx::query_scalar(
            "SELECT credential_id FROM providers WHERE credential_id IS NOT NULL",
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default();
        for cid in cred_ids {
            if let Ok(Some(secret)) = store.get(&cid) {
                let trimmed = secret.expose().trim().to_string();
                if trimmed.len() >= 6 {
                    known_secrets.push(trimmed);
                }
            }
        }
    }
    let secret_refs: Vec<&str> = known_secrets.iter().map(String::as_str).collect();

    // 脱敏后的 Provider 列表（只暴露 id, name, type, base_url，绝对不包含密钥或 headers）
    let prov_rows = sqlx::query("SELECT id, name, type, base_url, enabled FROM providers")
        .fetch_all(pool)
        .await
        .map_err(AppError::db)?;

    let mut sanitized_providers = Vec::new();
    for row in prov_rows {
        sanitized_providers.push(serde_json::json!({
            "id": row.try_get::<String, _>("id").unwrap_or_default(),
            "name": row.try_get::<String, _>("name").unwrap_or_default(),
            "type": row.try_get::<String, _>("type").unwrap_or_default(),
            "baseUrl": row.try_get::<String, _>("base_url").unwrap_or_default(),
            "enabled": row.try_get::<i64, _>("enabled").unwrap_or(0) != 0,
        }));
    }

    // 最近失败任务的错误信息（不包含 prompt）
    let error_rows = sqlx::query(
        "SELECT id, modality, provider_id, model_id, status, error, created_at, completed_at
         FROM generation_tasks
         WHERE status IN ('failed', 'interrupted')
         ORDER BY created_at DESC
         LIMIT 20",
    )
    .fetch_all(pool)
    .await
    .map_err(AppError::db)?;

    let mut recent_task_errors = Vec::new();
    for row in error_rows {
        let raw_err = row
            .try_get::<Option<String>, _>("error")
            .unwrap_or_default();
        let sanitized_err =
            raw_err.map(|e| crate::redact::mask_with_known_secrets(&e, &secret_refs));
        recent_task_errors.push(serde_json::json!({
            "id": row.try_get::<String, _>("id").unwrap_or_default(),
            "modality": row.try_get::<String, _>("modality").unwrap_or_default(),
            "providerId": row.try_get::<String, _>("provider_id").unwrap_or_default(),
            "modelId": row.try_get::<String, _>("model_id").unwrap_or_default(),
            "status": row.try_get::<String, _>("status").unwrap_or_default(),
            "error": sanitized_err,
            "createdAt": row.try_get::<String, _>("created_at").unwrap_or_default(),
        }));
    }

    let recent_logs = crate::logging::global_logger()
        .recent(100)
        .into_iter()
        .map(|mut log| {
            log.message = crate::redact::mask_with_known_secrets(&log.message, &secret_refs);
            log
        })
        .collect();

    Ok(DiagnosticBundle {
        report,
        sanitized_providers,
        storage,
        recent_task_errors,
        recent_logs,
    })
}
