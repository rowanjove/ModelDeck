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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>, // "network" | "credentials" | "model" | "disk" | "webview2"
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

pub fn classify_failure_reason(error_msg: &str) -> &'static str {
    let lower = error_msg.to_lowercase();
    if lower.contains("401")
        || lower.contains("unauthorized")
        || lower.contains("api key")
        || lower.contains("api_key")
        || lower.contains("credential")
        || lower.contains("forbidden")
        || lower.contains("authentication")
        || lower.contains("token")
        || lower.contains("bearer")
    {
        "credentials"
    } else if lower.contains("404")
        || lower.contains("model")
        || lower.contains("not found")
        || lower.contains("does not exist")
        || lower.contains("unsupported")
    {
        "model"
    } else if lower.contains("disk")
        || lower.contains("space")
        || lower.contains("full")
        || lower.contains("permission denied")
        || lower.contains("read-only")
    {
        "disk"
    } else if lower.contains("webview2") || lower.contains("edge") {
        "webview2"
    } else if lower.contains("timeout")
        || lower.contains("connect")
        || lower.contains("dns")
        || lower.contains("proxy")
        || lower.contains("network")
        || lower.contains("reset")
        || lower.contains("eof")
    {
        "network"
    } else {
        "unknown"
    }
}

async fn probe_connectivity(
    settings: &crate::services::network::ProxySettings,
) -> Result<u64, String> {
    let client = crate::services::network::build_client_with_proxy(settings)
        .map_err(|e| format!("构建代理客户端失败: {e}"))?;
    let start = std::time::Instant::now();
    let resp = client
        .get("https://1.1.1.1")
        .timeout(std::time::Duration::from_millis(2000))
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                "公网连接超时 (2s)".to_string()
            } else if e.is_connect() {
                "目标地址不可达或代理未启动".to_string()
            } else {
                e.to_string()
            }
        })?;
    if resp.status().is_success() || resp.status().as_u16() < 500 {
        Ok(start.elapsed().as_millis() as u64)
    } else {
        Err(format!("端点响应 HTTP {}", resp.status().as_u16()))
    }
}

pub async fn run_diagnostics(
    data_dir: &Path,
    pool: &SqlitePool,
    secrets: Option<&dyn crate::credentials::SecretStore>,
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
                failure_reason: if is_ok {
                    None
                } else {
                    Some("disk".to_string())
                },
            });
        }
        Err(e) => {
            items.push(DiagnosticItem {
                category: "数据库".to_string(),
                name: "SQLite Integrity".to_string(),
                status: "error".to_string(),
                detail: e.message,
                failure_reason: Some("disk".to_string()),
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
        failure_reason: None,
    });

    // 3. 磁盘空间检查
    #[cfg(windows)]
    {
        if let Some(free_bytes) = crate::storage_util::platform_free_space(data_dir) {
            let free_mb = free_bytes / (1024 * 1024);
            let (status, reason) = if free_mb > 1000 {
                ("ok", None)
            } else if free_mb > 500 {
                ("warn", Some("disk".to_string()))
            } else {
                ("error", Some("disk".to_string()))
            };
            items.push(DiagnosticItem {
                category: "存储".to_string(),
                name: "磁盘可用空间".to_string(),
                status: status.to_string(),
                detail: format!("剩余可用空间: {} MB (底线安全阈值: 500 MB)", free_mb),
                failure_reason: reason,
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
        failure_reason: if rw_ok {
            None
        } else {
            Some("disk".to_string())
        },
    });

    // 5. WebView2 运行时检查
    let webview2_installed = crate::windows_util::is_webview2_installed();
    items.push(DiagnosticItem {
        category: "系统与环境".to_string(),
        name: "WebView2 Runtime".to_string(),
        status: if webview2_installed {
            "ok".to_string()
        } else {
            "error".to_string()
        },
        detail: if webview2_installed {
            "Microsoft Edge WebView2 运行时已安装且就绪".to_string()
        } else {
            "未检测到 WebView2 运行时，可能导致界面无法渲染。请从微软官方下载安装".to_string()
        },
        failure_reason: if webview2_installed {
            None
        } else {
            Some("webview2".to_string())
        },
    });

    // 6. 网络与代理通道自检
    let proxy_settings = crate::services::network::get_proxy_settings(pool)
        .await
        .unwrap_or_default();
    let mode_desc = match proxy_settings.mode.as_str() {
        "system" => "系统代理 (跟随系统)".to_string(),
        "none" => "直连 (已禁用代理)".to_string(),
        "http" => format!(
            "HTTP 代理 ({})",
            proxy_settings.http_proxy.as_deref().unwrap_or("未配置")
        ),
        "socks5" => format!(
            "SOCKS5 代理 ({})",
            proxy_settings.socks5_proxy.as_deref().unwrap_or("未配置")
        ),
        other => other.to_string(),
    };

    let probe_res = probe_connectivity(&proxy_settings).await;
    match probe_res {
        Ok(latency_ms) => {
            items.push(DiagnosticItem {
                category: "网络与代理".to_string(),
                name: "网络与代理连通性".to_string(),
                status: "ok".to_string(),
                detail: format!(
                    "当前模式: {}；公网端点连通正常 (探测延迟: {} ms)",
                    mode_desc, latency_ms
                ),
                failure_reason: None,
            });
        }
        Err(err) => {
            items.push(DiagnosticItem {
                category: "网络与代理".to_string(),
                name: "网络与代理连通性".to_string(),
                status: "warn".to_string(),
                detail: format!(
                    "当前模式: {}；外网连通探测未通: {} (内网离线或仅使用本地模型时可忽略)",
                    mode_desc, err
                ),
                failure_reason: Some("network".to_string()),
            });
        }
    }

    // 7. 关键 Provider 连通性与凭据有效性自检
    let prov_rows = sqlx::query(
        "SELECT id, name, type, base_url, credential_id, enabled FROM providers WHERE enabled = 1",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let model_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM models WHERE enabled = 1")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    if prov_rows.is_empty() {
        items.push(DiagnosticItem {
            category: "模型与凭据".to_string(),
            name: "关键 Provider 凭据".to_string(),
            status: "warn".to_string(),
            detail: "当前未启用任何 AI Provider，无法发起任务生成".to_string(),
            failure_reason: Some("model".to_string()),
        });
    } else {
        let mut missing_creds = Vec::new();
        for row in &prov_rows {
            let p_name: String = row.try_get("name").unwrap_or_default();
            let p_type: String = row.try_get("type").unwrap_or_default();
            let cred_id: Option<String> = row.try_get("credential_id").unwrap_or(None);

            match cred_id {
                Some(cid) if !cid.trim().is_empty() => {
                    if let Some(store) = secrets {
                        if let Ok(Some(secret)) = store.get(&cid) {
                            if secret.expose().trim().is_empty() {
                                missing_creds.push(format!("{p_name}({p_type}) 密钥为空"));
                            }
                        } else {
                            missing_creds.push(format!("{p_name}({p_type}) 凭据丢失"));
                        }
                    }
                }
                _ => {
                    missing_creds.push(format!("{p_name}({p_type}) 未配置凭据"));
                }
            }
        }

        if !missing_creds.is_empty() {
            items.push(DiagnosticItem {
                category: "模型与凭据".to_string(),
                name: "关键 Provider 凭据".to_string(),
                status: "error".to_string(),
                detail: format!("发现凭据异常: {}", missing_creds.join("; ")),
                failure_reason: Some("credentials".to_string()),
            });
        } else {
            items.push(DiagnosticItem {
                category: "模型与凭据".to_string(),
                name: "关键 Provider 凭据".to_string(),
                status: "ok".to_string(),
                detail: format!(
                    "已启用 {} 个 Provider，凭据配置完整；启用模型 {} 个",
                    prov_rows.len(),
                    model_count
                ),
                failure_reason: None,
            });
        }
    }

    // 8. 存储孤儿与碎片情况
    let storage_report =
        crate::services::storage_maintenance::inspect_storage(data_dir, pool).await?;
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
        failure_reason: if has_missing {
            Some("disk".to_string())
        } else {
            None
        },
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
    let report = run_diagnostics(data_dir, pool, secrets).await?;
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

    // 最近失败任务的错误信息（不包含 prompt，增加 failureReason 归因）
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
        let sanitized_err = raw_err
            .as_deref()
            .map(|e| crate::redact::mask_with_known_secrets(e, &secret_refs));
        let failure_reason = raw_err
            .as_deref()
            .map(classify_failure_reason)
            .unwrap_or("unknown");

        recent_task_errors.push(serde_json::json!({
            "id": row.try_get::<String, _>("id").unwrap_or_default(),
            "modality": row.try_get::<String, _>("modality").unwrap_or_default(),
            "providerId": row.try_get::<String, _>("provider_id").unwrap_or_default(),
            "modelId": row.try_get::<String, _>("model_id").unwrap_or_default(),
            "status": row.try_get::<String, _>("status").unwrap_or_default(),
            "error": sanitized_err,
            "failureReason": failure_reason,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_failure_reason() {
        assert_eq!(
            classify_failure_reason("HTTP 401 Unauthorized: Invalid API key"),
            "credentials"
        );
        assert_eq!(
            classify_failure_reason("Bearer token expired"),
            "credentials"
        );
        assert_eq!(
            classify_failure_reason("Model gpt-5 not found or does not exist"),
            "model"
        );
        assert_eq!(
            classify_failure_reason("Connection reset by peer: timeout after 10s"),
            "network"
        );
        assert_eq!(
            classify_failure_reason("DNS probe failed: proxy connection refused"),
            "network"
        );
        assert_eq!(
            classify_failure_reason("IO Error: No space left on device disk full"),
            "disk"
        );
        assert_eq!(
            classify_failure_reason("Microsoft Edge WebView2 runtime failed to initialize"),
            "webview2"
        );
        assert_eq!(
            classify_failure_reason("Generic internal calculation syntax"),
            "unknown"
        );
    }

    #[tokio::test]
    async fn test_run_diagnostics_and_bundle_export() {
        let temp_dir = std::env::temp_dir().join(format!("ai-diag-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test.db");
        let pool = crate::db::open(&db_path).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();

        // 插入测试 Provider 和任务错误
        sqlx::query(
            "INSERT INTO providers (id, name, type, base_url, credential_id, headers_json, timeout_ms, enabled, created_at, updated_at)
             VALUES ('prov-1', 'Test Provider', 'openai', 'https://api.openai.com/v1', 'cred-1', '{}', 30000, 1, '2026-10-10T00:00:00Z', '2026-10-10T00:00:00Z')"
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, input_json, parameters_json, error, created_at)
             VALUES ('task-err-1', 'text', 'prov-1', 'gpt-4o', 'failed', '{}', '{}', 'Error 401: sk-secretkey123456 is invalid', '2026-10-10T00:00:00Z')"
        )
        .execute(&pool)
        .await
        .unwrap();

        let report = run_diagnostics(&temp_dir, &pool, None).await.unwrap();
        assert!(!report.items.is_empty());
        assert!(report.items.iter().any(|i| i.name == "SQLite Integrity"));
        assert!(report.items.iter().any(|i| i.name == "WebView2 Runtime"));
        assert!(report.items.iter().any(|i| i.name == "网络与代理连通性"));
        assert!(report.items.iter().any(|i| i.name == "关键 Provider 凭据"));

        let bundle = export_diagnostic_bundle(&temp_dir, &pool, None)
            .await
            .unwrap();
        assert_eq!(bundle.sanitized_providers.len(), 1);
        assert_eq!(bundle.recent_task_errors.len(), 1);
        let first_err = &bundle.recent_task_errors[0];
        assert_eq!(
            first_err.get("failureReason").unwrap().as_str().unwrap(),
            "credentials"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
