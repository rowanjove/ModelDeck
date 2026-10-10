use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use tokio::time::timeout;

use crate::credentials::SecretStore;
use crate::domain::{
    now_iso, GenerateAudioInput, GenerateImageInput, GenerateVideoInput, Modality,
};
use crate::error::AppError;
use crate::services::batch::BatchGenerateInput;
use crate::services::pipeline::PipelineExecutionInput;
use crate::services::task_scheduler::{TaskScheduler, TaskStatus};

pub type EventSender = Arc<dyn Fn(&str, &TaskEvent) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
    pub task_id: String,
    pub event_type: String, // "queued" | "started" | "progress" | "intermediate" | "completed" | "failed" | "cancelled"
    pub modality: String,
    pub status: String,
    pub progress: Option<f64>,
    pub stage: Option<String>,
    pub message: Option<String>,
    pub output_asset: Option<Value>,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub billing_risk: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInfo {
    pub id: String,
    pub modality: String,
    pub status: String,
    pub progress: Option<f64>,
    pub stage: Option<String>,
    pub message: Option<String>,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub billing_risk: Option<String>,
    pub output: Option<Value>,
    pub created_at: String,
    pub updated_at: String,
}

struct TaskProgress<'a> {
    task_id: &'a str,
    modality: &'a str,
    event_type: &'a str,
    progress: f64,
    stage: &'a str,
    message: &'a str,
    output: Option<Value>,
}

pub struct JobQueue {
    db: SqlitePool,
    http: RwLock<Client>,
    secrets: Arc<dyn SecretStore>,
    data_dir: PathBuf,
    scheduler: Arc<TaskScheduler>,
    cancels: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    active_tasks: Arc<RwLock<HashMap<String, TaskInfo>>>,
    deleted_tasks: RwLock<HashSet<String>>,
    emitter: RwLock<Option<EventSender>>,
}

impl JobQueue {
    pub fn new(
        db: SqlitePool,
        http: Client,
        secrets: Arc<dyn SecretStore>,
        data_dir: PathBuf,
        scheduler: Arc<TaskScheduler>,
        cancels: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
        emitter: Option<EventSender>,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            http: RwLock::new(http),
            secrets,
            data_dir,
            scheduler,
            cancels,
            active_tasks: Arc::new(RwLock::new(HashMap::new())),
            deleted_tasks: RwLock::new(HashSet::new()),
            emitter: RwLock::new(emitter),
        })
    }

    pub fn http(&self) -> Client {
        self.http.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn update_http(&self, client: Client) {
        let mut guard = self.http.write().unwrap_or_else(|e| e.into_inner());
        *guard = client;
    }

    pub fn set_emitter(&self, emitter: EventSender) {
        let mut guard = self.emitter.write().unwrap_or_else(|e| e.into_inner());
        *guard = Some(emitter);
    }

    pub async fn hydrate_recent_tasks(&self) -> Result<(), AppError> {
        let rows = sqlx::query(
            "SELECT id, modality, status, error, queue_output_json, remote_job_id, billing_risk, error_code, retryable, created_at,
                    COALESCE(completed_at, created_at) AS updated_at
             FROM generation_tasks
             WHERE status != 'claimed'
             ORDER BY created_at DESC
             LIMIT 200",
        )
        .fetch_all(&self.db)
        .await
        .map_err(AppError::db)?;
        let mut tasks = self.active_tasks.write().unwrap_or_else(|e| e.into_inner());
        for row in rows {
            let raw_output: Option<String> =
                row.try_get("queue_output_json").map_err(AppError::db)?;
            let output = raw_output.and_then(|value| serde_json::from_str(&value).ok());
            let remote_job_id: Option<String> = row.try_get("remote_job_id").ok().flatten();
            let billing_risk: Option<String> = row.try_get("billing_risk").ok().flatten();
            let error_code: Option<String> = row.try_get("error_code").ok().flatten();
            let retryable: Option<bool> = row
                .try_get::<Option<i64>, _>("retryable")
                .ok()
                .flatten()
                .map(|v| v != 0);
            let task = TaskInfo {
                id: row.try_get("id").map_err(AppError::db)?,
                modality: row.try_get("modality").map_err(AppError::db)?,
                status: row.try_get("status").map_err(AppError::db)?,
                progress: None,
                stage: None,
                message: None,
                error: row.try_get("error").map_err(AppError::db)?,
                error_code,
                retryable,
                remote_job_id,
                billing_risk,
                output,
                created_at: row.try_get("created_at").map_err(AppError::db)?,
                updated_at: row.try_get("updated_at").map_err(AppError::db)?,
            };
            tasks.insert(task.id.clone(), task);
        }
        Ok(())
    }

    fn emit(&self, event_type: &str, event: &TaskEvent) {
        let guard = self.emitter.read().unwrap_or_else(|e| e.into_inner());
        if let Some(emitter) = guard.as_ref() {
            let specific_event = format!("task:{event_type}");
            emitter(&specific_event, event);
            emitter("task:event", event);
        }
    }

    pub async fn prepare_task_id(
        &self,
        task_id: Option<String>,
        modality: &str,
        model_id: Option<&str>,
    ) -> Result<(String, Arc<AtomicBool>), AppError> {
        let id = match task_id {
            Some(id) if !id.trim().is_empty() => id.trim().to_string(),
            _ => uuid::Uuid::new_v4().to_string(),
        };
        crate::storage_util::validate_task_id(&id)?;
        if self
            .active_tasks
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .contains_key(&id)
        {
            return Err(AppError::conflict(format!(
                "任务 ID '{id}' 已存在于任务队列中"
            )));
        }

        // Register before the first database await so cancellation/deletion can
        // reach a submission while its claimed row is being created.
        let cancel_flag = self.register_cancel(&id)?;
        match crate::db::claim_task_id(&self.db, Some(id.clone()), modality, model_id).await {
            Ok(claimed_id) => Ok((claimed_id, cancel_flag)),
            Err(err) => {
                self.unregister_cancel(&id, &cancel_flag);
                Err(err)
            }
        }
    }

    pub async fn persist_queued_task(
        &self,
        id: &str,
        modality: &str,
        model_definition_id: &str,
        input_value: &Value,
        parameters_value: &serde_json::Map<String, Value>,
    ) -> Result<(), AppError> {
        let (provider_id, model_id) =
            if let Ok(model) = crate::db::get_model(&self.db, model_definition_id).await {
                (model.provider_id, model.id)
            } else {
                (
                    model_definition_id.to_string(),
                    model_definition_id.to_string(),
                )
            };
        let input_str = serde_json::to_string(input_value).unwrap_or_else(|_| "{}".to_string());
        let params_str =
            serde_json::to_string(parameters_value).unwrap_or_else(|_| "{}".to_string());

        let result = sqlx::query(
            "UPDATE generation_tasks
             SET modality = ?, provider_id = ?, model_id = ?, status = 'queued', input_json = ?, parameters_json = ?
             WHERE id = ? AND status = 'claimed'",
        )
        .bind(modality)
        .bind(provider_id)
        .bind(model_id)
        .bind(input_str)
        .bind(params_str)
        .bind(id)
        .execute(&self.db)
        .await;
        let result = match result {
            Ok(result) => result,
            Err(err) => {
                let now = now_iso();
                let _ = sqlx::query(
                    "UPDATE generation_tasks SET status = 'failed', error = ?, completed_at = ?
                     WHERE id = ? AND status = 'claimed'",
                )
                .bind(format!("任务入队失败: {err}"))
                .bind(now)
                .bind(id)
                .execute(&self.db)
                .await;
                return Err(AppError::db(err));
            }
        };
        if result.rows_affected() != 1 {
            return Err(AppError::conflict("任务已被取消、删除或已开始，不能排队"));
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_task_state_ext(
        &self,
        task_id: &str,
        modality: &str,
        event_type: &str,
        status: &str,
        progress: Option<f64>,
        stage: Option<&str>,
        message: Option<&str>,
        output_asset: Option<Value>,
        error: Option<&str>,
        error_code: Option<&str>,
        retryable: Option<bool>,
        remote_job_id: Option<&str>,
        billing_risk: Option<&str>,
    ) {
        if self
            .deleted_tasks
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains(task_id)
        {
            return;
        }
        let mut retryable = retryable;
        let now = now_iso();
        let is_terminal = matches!(
            status,
            "completed" | "failed" | "cancelled" | "interrupted" | "unknown_submission"
        );
        let mut status_owned = status.to_string();
        let mut event_type_owned = event_type.to_string();
        let mut stage_owned = stage.map(str::to_string);
        let mut message_owned = message.map(str::to_string);
        let mut error_owned = error.map(str::to_string);
        let mut output_asset_owned = output_asset.clone();
        let mut error_code_owned = error_code.map(str::to_string);
        let mut remote_job_id_owned = remote_job_id.map(str::to_string);
        let mut billing_risk_owned = billing_risk.map(str::to_string);

        if is_terminal {
            let output_json = output_asset
                .as_ref()
                .and_then(|value| serde_json::to_string(value).ok());
            let retryable_int = retryable.map(|r| if r { 1_i64 } else { 0_i64 });
            let result = sqlx::query(
                "UPDATE generation_tasks
                 SET status = ?, error = COALESCE(?, error), completed_at = COALESCE(?, completed_at),
                     queue_output_json = COALESCE(?, queue_output_json),
                     remote_job_id = COALESCE(?, remote_job_id),
                     billing_risk = COALESCE(?, billing_risk),
                     error_code = COALESCE(?, error_code),
                     retryable = COALESCE(?, retryable)
                 WHERE id = ? AND (status NOT IN ('completed', 'failed', 'cancelled', 'interrupted', 'unknown_submission') OR status = ?)",
            )
            .bind(status)
            .bind(error)
            .bind(&now)
            .bind(output_json.as_deref())
            .bind(remote_job_id)
            .bind(billing_risk)
            .bind(error_code)
            .bind(retryable_int)
            .bind(task_id)
            .bind(status)
            .execute(&self.db)
            .await;
            let result = match result {
                Ok(result) => result,
                Err(err) => {
                    crate::logging::global_logger().error(
                        "task",
                        None,
                        Some(task_id),
                        None,
                        None,
                        "task_state_persist_failed",
                        &err.to_string(),
                    );
                    return;
                }
            };
            if result.rows_affected() == 0 {
                let row = sqlx::query(
                    "SELECT status, error, queue_output_json, remote_job_id, billing_risk, error_code, retryable FROM generation_tasks WHERE id = ?",
                )
                .bind(task_id)
                .fetch_optional(&self.db)
                .await;
                let row = match row {
                    Ok(Some(row)) => row,
                    Ok(None) => return,
                    Err(err) => {
                        crate::logging::global_logger().error(
                            "task",
                            None,
                            Some(task_id),
                            None,
                            None,
                            "task_state_persist_failed",
                            &err.to_string(),
                        );
                        return;
                    }
                };
                status_owned = row.try_get("status").unwrap_or_default();
                if !matches!(
                    status_owned.as_str(),
                    "completed" | "failed" | "cancelled" | "interrupted" | "unknown_submission"
                ) {
                    return;
                }
                event_type_owned = status_owned.clone();
                error_owned = row.try_get("error").ok();
                if remote_job_id_owned.is_none() {
                    remote_job_id_owned = row.try_get("remote_job_id").ok().flatten();
                }
                if billing_risk_owned.is_none() {
                    billing_risk_owned = row.try_get("billing_risk").ok().flatten();
                }
                if error_code_owned.is_none() {
                    error_code_owned = row.try_get("error_code").ok().flatten();
                }
                if retryable.is_none() {
                    retryable = row
                        .try_get::<Option<i64>, _>("retryable")
                        .ok()
                        .flatten()
                        .map(|v| v != 0);
                }
                output_asset_owned = row
                    .try_get::<Option<String>, _>("queue_output_json")
                    .ok()
                    .flatten()
                    .and_then(|value| serde_json::from_str(&value).ok());
                if status_owned != status {
                    stage_owned = Some(status_owned.clone());
                    message_owned = Some(match status_owned.as_str() {
                        "cancelled" => "任务已取消".to_string(),
                        "interrupted" => "任务已中断".to_string(),
                        "unknown_submission" => "提交未决（防重复扣费）".to_string(),
                        _ => "任务状态已由其他操作完成".to_string(),
                    });
                }
            }
        }

        {
            let mut tasks = self.active_tasks.write().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = tasks.get_mut(task_id) {
                let current_is_terminal = matches!(
                    entry.status.as_str(),
                    "completed" | "failed" | "cancelled" | "interrupted" | "unknown_submission"
                );
                // 任何终态一旦落定都不可再被另一个终态覆盖。
                if current_is_terminal && entry.status != status_owned {
                    return;
                }

                entry.status = status_owned.clone();
                entry.updated_at = now.clone();
                if progress.is_some() {
                    entry.progress = progress;
                }
                if stage_owned.is_some() {
                    entry.stage = stage_owned.clone();
                }
                if message_owned.is_some() {
                    entry.message = message_owned.clone();
                }
                if error_owned.is_some() {
                    entry.error = error_owned.clone();
                }
                if error_code_owned.is_some() {
                    entry.error_code = error_code_owned.clone();
                }
                if retryable.is_some() {
                    entry.retryable = retryable;
                }
                if remote_job_id_owned.is_some() {
                    entry.remote_job_id = remote_job_id_owned.clone();
                }
                if billing_risk_owned.is_some() {
                    entry.billing_risk = billing_risk_owned.clone();
                }
                if output_asset_owned.is_some() {
                    entry.output = output_asset_owned.clone();
                }
            } else {
                tasks.insert(
                    task_id.to_string(),
                    TaskInfo {
                        id: task_id.to_string(),
                        modality: modality.to_string(),
                        status: status_owned.clone(),
                        progress,
                        stage: stage_owned.clone(),
                        message: message_owned.clone(),
                        error: error_owned.clone(),
                        error_code: error_code_owned.clone(),
                        retryable,
                        remote_job_id: remote_job_id_owned.clone(),
                        billing_risk: billing_risk_owned.clone(),
                        output: output_asset_owned.clone(),
                        created_at: now.clone(),
                        updated_at: now.clone(),
                    },
                );
            }

            // 限制内存中终态缓存容量上限（FIFO 淘汰超出部分）
            if tasks.len() > 200 {
                let terminal_keys: Vec<String> = tasks
                    .iter()
                    .filter(|(_, t)| {
                        matches!(
                            t.status.as_str(),
                            "completed"
                                | "failed"
                                | "cancelled"
                                | "interrupted"
                                | "unknown_submission"
                        )
                    })
                    .map(|(k, _)| k.clone())
                    .take(tasks.len() - 150)
                    .collect();
                for k in terminal_keys {
                    tasks.remove(&k);
                }
            }
        }

        // Nonterminal progress is best-effort; terminal states were committed
        // above before changing memory or emitting an event.
        if !is_terminal {
            let db = self.db.clone();
            let task_id_owned = task_id.to_string();
            let status_owned = status.to_string();
            tokio::spawn(async move {
                let result = if status_owned == "running" {
                    sqlx::query(
                        "UPDATE generation_tasks SET status = ?, execution_phase = 'running'
                         WHERE id = ? AND status IN ('queued', 'submitting', 'running')",
                    )
                    .bind(&status_owned)
                    .bind(&task_id_owned)
                    .execute(&db)
                    .await
                } else if status_owned == "submitting" {
                    sqlx::query(
                        "UPDATE generation_tasks SET status = ?, execution_phase = 'submitting', billing_risk = 'possible'
                         WHERE id = ? AND status IN ('queued', 'claimed')",
                    )
                    .bind(&status_owned)
                    .bind(&task_id_owned)
                    .execute(&db)
                    .await
                } else if status_owned == "downloading" {
                    sqlx::query(
                        "UPDATE generation_tasks SET status = ?, execution_phase = 'downloading'
                         WHERE id = ? AND status IN ('running', 'downloading')",
                    )
                    .bind(&status_owned)
                    .bind(&task_id_owned)
                    .execute(&db)
                    .await
                } else {
                    sqlx::query(
                        "UPDATE generation_tasks SET status = ? WHERE id = ? AND status = 'queued'",
                    )
                    .bind(&status_owned)
                    .bind(&task_id_owned)
                    .execute(&db)
                    .await
                };
                if let Err(err) = result {
                    crate::logging::global_logger().error(
                        "task",
                        None,
                        Some(&task_id_owned),
                        None,
                        None,
                        "task_state_persist_failed",
                        &err.to_string(),
                    );
                }
            });
        }

        let event = TaskEvent {
            task_id: task_id.to_string(),
            event_type: event_type_owned.clone(),
            modality: modality.to_string(),
            status: status_owned.clone(),
            progress,
            stage: stage_owned,
            message: message_owned,
            output_asset: output_asset_owned,
            error: error_owned,
            error_code: error_code_owned,
            retryable,
            remote_job_id: remote_job_id_owned,
            billing_risk: billing_risk_owned,
            timestamp: now,
        };

        self.emit(&event_type_owned, &event);
    }

    #[allow(clippy::too_many_arguments)]
    async fn update_task_state(
        &self,
        task_id: &str,
        modality: &str,
        event_type: &str,
        status: &str,
        progress: Option<f64>,
        stage: Option<&str>,
        message: Option<&str>,
        output_asset: Option<Value>,
        error: Option<&str>,
    ) {
        self.update_task_state_ext(
            task_id,
            modality,
            event_type,
            status,
            progress,
            stage,
            message,
            output_asset,
            error,
            None,
            None,
            None,
            None,
        )
        .await;
    }

    pub async fn classify_timeout_failure(
        &self,
        task_id: &str,
        modality: &str,
        default_secs: u64,
    ) -> (String, String, bool, Option<String>, String) {
        let row = sqlx::query(
            "SELECT execution_phase, remote_job_id, billing_risk FROM generation_tasks WHERE id = ?",
        )
        .bind(task_id)
        .fetch_optional(&self.db)
        .await
        .ok()
        .flatten();

        let phase = row
            .as_ref()
            .and_then(|r| r.try_get::<Option<String>, _>("execution_phase").ok())
            .flatten();
        let remote_id = row
            .as_ref()
            .and_then(|r| r.try_get::<Option<String>, _>("remote_job_id").ok())
            .flatten();

        match phase.as_deref() {
            Some("submitting") => (
                format!("{modality}任务向服务商提交超时（超过 {default_secs} 秒，尚未收到远端受理确认，可能存在计费风险）"),
                "submit_timeout".to_string(),
                false,
                remote_id,
                "possible".to_string(),
            ),
            Some("downloading") => (
                format!("{modality}生成已完成，但产物下载超时（超过 {default_secs} 秒，可重新尝试下载）"),
                "download_timeout".to_string(),
                true,
                remote_id,
                "confirmed".to_string(),
            ),
            Some("running") if remote_id.is_some() => {
                let rid = remote_id.as_deref().unwrap_or_default();
                (
                    format!("远端{modality}生成等待超时（已等待超过 {default_secs} 秒）。已保存远端任务 ID [{rid}]，远端可能仍在执行并计费，可稍后在历史中刷新"),
                    "generation_timeout".to_string(),
                    false,
                    remote_id,
                    "possible".to_string(),
                )
            }
            _ => (
                format!("{modality}任务执行超时 (超过 {default_secs} 秒)"),
                "timeout".to_string(),
                true,
                remote_id,
                "none".to_string(),
            ),
        }
    }

    pub async fn classify_cancellation(
        &self,
        task_id: &str,
        modality: &str,
    ) -> (String, bool, Option<String>, String) {
        let row = sqlx::query(
            "SELECT execution_phase, remote_job_id, billing_risk FROM generation_tasks WHERE id = ?",
        )
        .bind(task_id)
        .fetch_optional(&self.db)
        .await
        .ok()
        .flatten();

        let phase = row
            .as_ref()
            .and_then(|r| r.try_get::<Option<String>, _>("execution_phase").ok())
            .flatten();
        let remote_id = row
            .as_ref()
            .and_then(|r| r.try_get::<Option<String>, _>("remote_job_id").ok())
            .flatten();

        match phase.as_deref() {
            Some("queued") | None => (
                format!("{modality}任务已在排队阶段取消（未向服务商发起网络请求，无费用）"),
                true,
                None,
                "none".to_string(),
            ),
            Some("downloading") => (
                format!("{modality}生成已完成，下载已取消"),
                true,
                remote_id,
                "confirmed".to_string(),
            ),
            _ => {
                if let Some(ref rid) = remote_id {
                    (
                        format!("客户端已停止等待{modality}任务。服务商不支持远端中断，远端任务 [{rid}] 可能仍在继续并产生费用"),
                        false,
                        remote_id,
                        "possible".to_string(),
                    )
                } else {
                    (
                        format!("客户端已停止等待{modality}任务。由于请求已发出，远端服务商可能仍在继续执行并产生费用"),
                        false,
                        None,
                        "possible".to_string(),
                    )
                }
            }
        }
    }

    pub fn get_task_info(&self, task_id: &str) -> Option<TaskInfo> {
        let tasks = self.active_tasks.read().unwrap_or_else(|e| e.into_inner());
        tasks.get(task_id).cloned()
    }

    pub fn list_active_tasks(&self) -> Vec<TaskInfo> {
        let tasks = self.active_tasks.read().unwrap_or_else(|e| e.into_inner());
        tasks
            .values()
            .filter(|t| {
                !matches!(
                    t.status.as_str(),
                    "completed" | "failed" | "cancelled" | "interrupted" | "unknown_submission"
                )
            })
            .cloned()
            .collect()
    }

    pub fn cancel(&self, task_id: &str) -> bool {
        let map = self.cancels.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(flag) = map.get(task_id) {
            flag.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    pub async fn mark_cancelled(&self, task_id: &str) {
        let modality = self
            .active_tasks
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(task_id)
            .map(|task| task.modality.clone());
        if let Some(modality) = modality {
            let (msg, retryable, remote_id, billing_risk) =
                self.classify_cancellation(task_id, &modality).await;
            self.update_task_state_ext(
                task_id,
                &modality,
                "cancelled",
                "cancelled",
                None,
                Some("cancelled"),
                Some(&msg),
                None,
                None,
                Some("cancelled"),
                Some(retryable),
                remote_id.as_deref(),
                Some(&billing_risk),
            )
            .await;
        }
    }

    pub fn cancel_tasks(&self, task_ids: &[String]) {
        for id in task_ids {
            self.cancel(id);
        }
    }

    pub fn forget_deleted_tasks(&self, task_ids: &[String]) {
        let mut deleted_tasks = self
            .deleted_tasks
            .write()
            .unwrap_or_else(|e| e.into_inner());
        let mut cancel_map = self.cancels.lock().unwrap_or_else(|e| e.into_inner());
        let mut tasks = self.active_tasks.write().unwrap_or_else(|e| e.into_inner());
        for id in task_ids {
            deleted_tasks.insert(id.clone());
            if let Some(flag) = cancel_map.remove(id) {
                flag.store(true, Ordering::Relaxed);
            }
            tasks.remove(id);
        }
    }

    fn register_cancel(&self, task_id: &str) -> Result<Arc<AtomicBool>, AppError> {
        let flag = Arc::new(AtomicBool::new(false));
        let mut map = self.cancels.lock().unwrap_or_else(|err| err.into_inner());
        if map.contains_key(task_id) {
            return Err(AppError::conflict(format!(
                "任务 ID '{task_id}' 已有活动任务"
            )));
        }
        map.insert(task_id.to_string(), Arc::clone(&flag));
        Ok(flag)
    }

    fn unregister_cancel(&self, task_id: &str, flag: &Arc<AtomicBool>) {
        let mut map = self.cancels.lock().unwrap_or_else(|err| err.into_inner());
        if map
            .get(task_id)
            .is_some_and(|current| Arc::ptr_eq(current, flag))
        {
            map.remove(task_id);
        }
    }

    async fn scheduler_acquire_failed(
        &self,
        task_id: &str,
        modality: &str,
        cancel_flag: &AtomicBool,
        err: &AppError,
    ) {
        if cancel_flag.load(Ordering::Relaxed) || err.code == "cancelled" {
            let (msg, retryable, remote_id, billing_risk) =
                self.classify_cancellation(task_id, modality).await;
            self.update_task_state_ext(
                task_id,
                modality,
                "cancelled",
                "cancelled",
                None,
                Some("cancelled"),
                Some(&msg),
                None,
                None,
                Some("cancelled"),
                Some(retryable),
                remote_id.as_deref(),
                Some(&billing_risk),
            )
            .await;
        } else {
            self.update_task_state_ext(
                task_id,
                modality,
                "failed",
                "failed",
                None,
                Some("failed"),
                Some(&err.message),
                None,
                Some(&err.message),
                Some(err.code),
                Some(true),
                None,
                Some("none"),
            )
            .await;
        }
    }

    async fn stop_if_cancelled(
        &self,
        task_id: &str,
        modality: &str,
        cancel_flag: &Arc<AtomicBool>,
    ) -> bool {
        if !cancel_flag.load(Ordering::Relaxed) {
            return false;
        }
        let (msg, retryable, remote_id, billing_risk) =
            self.classify_cancellation(task_id, modality).await;
        self.update_task_state_ext(
            task_id,
            modality,
            "cancelled",
            "cancelled",
            None,
            Some("cancelled"),
            Some(&msg),
            None,
            None,
            Some("cancelled"),
            Some(retryable),
            remote_id.as_deref(),
            Some(&billing_risk),
        )
        .await;
        self.unregister_cancel(task_id, cancel_flag);
        true
    }

    fn update_task_progress(&self, update: TaskProgress<'_>) {
        let TaskProgress {
            task_id,
            modality,
            event_type,
            progress,
            stage,
            message,
            output,
        } = update;
        if self
            .deleted_tasks
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains(task_id)
        {
            return;
        }
        let now = now_iso();
        if let Some(task) = self
            .active_tasks
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(task_id)
        {
            if !matches!(
                task.status.as_str(),
                "completed" | "failed" | "cancelled" | "interrupted" | "unknown_submission"
            ) {
                task.status = "running".to_string();
                task.progress = Some(progress);
                task.stage = Some(stage.to_string());
                task.message = Some(message.to_string());
                task.output = output.clone();
                task.updated_at = now.clone();
            } else {
                return;
            }
        } else {
            return;
        }
        let event = TaskEvent {
            task_id: task_id.to_string(),
            event_type: event_type.to_string(),
            modality: modality.to_string(),
            status: "running".to_string(),
            progress: Some(progress),
            stage: Some(stage.to_string()),
            message: Some(message.to_string()),
            output_asset: output,
            error: None,
            error_code: None,
            retryable: None,
            remote_job_id: None,
            billing_risk: None,
            timestamp: now,
        };
        self.emit(event_type, &event);
    }

    pub async fn submit_image(
        self: &Arc<Self>,
        input: GenerateImageInput,
        task_id: Option<String>,
    ) -> Result<String, AppError> {
        let (id, cancel_flag) = self
            .prepare_task_id(task_id, "image", Some(&input.model_definition_id))
            .await?;
        let is_comfyui = crate::db::is_comfyui_model(&self.db, &input.model_definition_id).await;
        if let Err(err) = self
            .persist_queued_task(
                &id,
                "image",
                &input.model_definition_id,
                &serde_json::to_value(&input).unwrap_or_default(),
                &input.parameters,
            )
            .await
        {
            self.unregister_cancel(&id, &cancel_flag);
            return Err(err);
        }

        let this = Arc::clone(self);
        let id_clone = id.clone();

        this.update_task_state(
            &id,
            "image",
            "queued",
            "queued",
            Some(0.0),
            Some("queued"),
            Some("任务已提交至队列"),
            None,
            None,
        )
        .await;

        tokio::spawn(async move {
            let cancel_flag = Arc::clone(&cancel_flag);
            let _permit = match this
                .scheduler
                .acquire_cancellable(Modality::Image, is_comfyui, Some(&cancel_flag))
                .await
            {
                Ok(p) => p,
                Err(err) => {
                    this.scheduler_acquire_failed(&id_clone, "image", &cancel_flag, &err)
                        .await;
                    this.unregister_cancel(&id_clone, &cancel_flag);
                    return;
                }
            };
            if this
                .stop_if_cancelled(&id_clone, "image", &cancel_flag)
                .await
            {
                return;
            }

            this.update_task_state(
                &id_clone,
                "image",
                "started",
                "running",
                Some(0.1),
                Some("running"),
                Some("任务正在执行"),
                None,
                None,
            )
            .await;

            let http = this.http();
            let res = timeout(
                Duration::from_secs(300),
                crate::services::image::execute_with_cancel(
                    &this.db,
                    &http,
                    this.secrets.as_ref(),
                    &this.data_dir,
                    input,
                    Some(id_clone.clone()),
                    Some(Arc::clone(&cancel_flag)),
                ),
            )
            .await;

            match res {
                Ok(Ok(result)) => {
                    let out_val = serde_json::to_value(&result).ok();
                    this.update_task_state(
                        &id_clone,
                        "image",
                        "completed",
                        "completed",
                        Some(1.0),
                        Some("completed"),
                        Some("生成完成"),
                        out_val,
                        None,
                    )
                    .await;
                }
                Ok(Err(err)) => {
                    let is_cancelled =
                        cancel_flag.load(Ordering::Relaxed) || err.code == "cancelled";
                    if is_cancelled {
                        let (msg, retryable, remote_id, billing_risk) =
                            this.classify_cancellation(&id_clone, "image").await;
                        this.update_task_state_ext(
                            &id_clone,
                            "image",
                            "cancelled",
                            "cancelled",
                            None,
                            Some("cancelled"),
                            Some(&msg),
                            None,
                            None,
                            Some("cancelled"),
                            Some(retryable),
                            remote_id.as_deref(),
                            Some(&billing_risk),
                        )
                        .await;
                    } else {
                        let retryable = crate::resilience::is_retryable(&err);
                        this.update_task_state_ext(
                            &id_clone,
                            "image",
                            "failed",
                            "failed",
                            None,
                            Some("failed"),
                            Some(&err.message),
                            None,
                            Some(&err.message),
                            Some(err.code),
                            Some(retryable),
                            None,
                            Some("none"),
                        )
                        .await;
                    }
                }
                Err(_) => {
                    let (err_msg, error_code, retryable, remote_id, billing_risk) =
                        this.classify_timeout_failure(&id_clone, "image", 300).await;
                    let _ = crate::services::task_scheduler::update_task_status(
                        &this.db,
                        &id_clone,
                        TaskStatus::Failed,
                        Some(&err_msg),
                    )
                    .await;
                    this.update_task_state_ext(
                        &id_clone,
                        "image",
                        "failed",
                        "failed",
                        None,
                        Some("failed"),
                        Some(&err_msg),
                        None,
                        Some(&err_msg),
                        Some(&error_code),
                        Some(retryable),
                        remote_id.as_deref(),
                        Some(&billing_risk),
                    )
                    .await;
                }
            }
            this.unregister_cancel(&id_clone, &cancel_flag);
        });

        Ok(id)
    }

    pub async fn submit_video(
        self: &Arc<Self>,
        input: GenerateVideoInput,
        task_id: Option<String>,
    ) -> Result<String, AppError> {
        let (id, cancel_flag) = self
            .prepare_task_id(task_id, "video", Some(&input.model_definition_id))
            .await?;
        let is_comfyui = crate::db::is_comfyui_model(&self.db, &input.model_definition_id).await;
        if let Err(err) = self
            .persist_queued_task(
                &id,
                "video",
                &input.model_definition_id,
                &serde_json::to_value(&input).unwrap_or_default(),
                &input.parameters,
            )
            .await
        {
            self.unregister_cancel(&id, &cancel_flag);
            return Err(err);
        }

        let this = Arc::clone(self);
        let id_clone = id.clone();

        this.update_task_state(
            &id,
            "video",
            "queued",
            "queued",
            Some(0.0),
            Some("queued"),
            Some("视频任务已提交至队列"),
            None,
            None,
        )
        .await;

        tokio::spawn(async move {
            let cancel_flag = Arc::clone(&cancel_flag);
            let _permit = match this
                .scheduler
                .acquire_cancellable(Modality::Video, is_comfyui, Some(&cancel_flag))
                .await
            {
                Ok(p) => p,
                Err(err) => {
                    this.scheduler_acquire_failed(&id_clone, "video", &cancel_flag, &err)
                        .await;
                    this.unregister_cancel(&id_clone, &cancel_flag);
                    return;
                }
            };
            if this
                .stop_if_cancelled(&id_clone, "video", &cancel_flag)
                .await
            {
                return;
            }

            this.update_task_state(
                &id_clone,
                "video",
                "started",
                "running",
                Some(0.1),
                Some("running"),
                Some("视频生成与轮询中"),
                None,
                None,
            )
            .await;

            let http = this.http();
            let res = timeout(
                Duration::from_secs(600),
                crate::services::video::execute_with_cancel(
                    &this.db,
                    &http,
                    this.secrets.as_ref(),
                    &this.data_dir,
                    input,
                    Some(id_clone.clone()),
                    Some(Arc::clone(&cancel_flag)),
                ),
            )
            .await;

            match res {
                Ok(Ok(result)) => {
                    let out_val = serde_json::to_value(&result).ok();
                    this.update_task_state(
                        &id_clone,
                        "video",
                        "completed",
                        "completed",
                        Some(1.0),
                        Some("completed"),
                        Some("视频生成完成"),
                        out_val,
                        None,
                    )
                    .await;
                }
                Ok(Err(err)) => {
                    let is_cancelled =
                        cancel_flag.load(Ordering::Relaxed) || err.code == "cancelled";
                    if is_cancelled {
                        let (msg, retryable, remote_id, billing_risk) =
                            this.classify_cancellation(&id_clone, "video").await;
                        this.update_task_state_ext(
                            &id_clone,
                            "video",
                            "cancelled",
                            "cancelled",
                            None,
                            Some("cancelled"),
                            Some(&msg),
                            None,
                            None,
                            Some("cancelled"),
                            Some(retryable),
                            remote_id.as_deref(),
                            Some(&billing_risk),
                        )
                        .await;
                    } else {
                        let retryable = crate::resilience::is_retryable(&err);
                        this.update_task_state_ext(
                            &id_clone,
                            "video",
                            "failed",
                            "failed",
                            None,
                            Some("failed"),
                            Some(&err.message),
                            None,
                            Some(&err.message),
                            Some(err.code),
                            Some(retryable),
                            None,
                            Some("none"),
                        )
                        .await;
                    }
                }
                Err(_) => {
                    let (err_msg, error_code, retryable, remote_id, billing_risk) =
                        this.classify_timeout_failure(&id_clone, "video", 600).await;
                    let _ = crate::services::task_scheduler::update_task_status(
                        &this.db,
                        &id_clone,
                        TaskStatus::Failed,
                        Some(&err_msg),
                    )
                    .await;
                    this.update_task_state_ext(
                        &id_clone,
                        "video",
                        "failed",
                        "failed",
                        None,
                        Some("failed"),
                        Some(&err_msg),
                        None,
                        Some(&err_msg),
                        Some(&error_code),
                        Some(retryable),
                        remote_id.as_deref(),
                        Some(&billing_risk),
                    )
                    .await;
                }
            }
            this.unregister_cancel(&id_clone, &cancel_flag);
        });

        Ok(id)
    }

    pub async fn submit_audio(
        self: &Arc<Self>,
        input: GenerateAudioInput,
        task_id: Option<String>,
    ) -> Result<String, AppError> {
        let (id, cancel_flag) = self
            .prepare_task_id(task_id, "audio", Some(&input.model_definition_id))
            .await?;
        let is_comfyui = crate::db::is_comfyui_model(&self.db, &input.model_definition_id).await;
        if let Err(err) = self
            .persist_queued_task(
                &id,
                "audio",
                &input.model_definition_id,
                &serde_json::to_value(&input).unwrap_or_default(),
                &input.parameters,
            )
            .await
        {
            self.unregister_cancel(&id, &cancel_flag);
            return Err(err);
        }

        let this = Arc::clone(self);
        let id_clone = id.clone();

        this.update_task_state(
            &id,
            "audio",
            "queued",
            "queued",
            Some(0.0),
            Some("queued"),
            Some("音频任务已进入队列"),
            None,
            None,
        )
        .await;

        tokio::spawn(async move {
            let cancel_flag = Arc::clone(&cancel_flag);
            let _permit = match this
                .scheduler
                .acquire_cancellable(Modality::Audio, is_comfyui, Some(&cancel_flag))
                .await
            {
                Ok(p) => p,
                Err(err) => {
                    this.scheduler_acquire_failed(&id_clone, "audio", &cancel_flag, &err)
                        .await;
                    this.unregister_cancel(&id_clone, &cancel_flag);
                    return;
                }
            };
            if this
                .stop_if_cancelled(&id_clone, "audio", &cancel_flag)
                .await
            {
                return;
            }

            this.update_task_state(
                &id_clone,
                "audio",
                "started",
                "running",
                Some(0.1),
                Some("running"),
                Some("音频合成中"),
                None,
                None,
            )
            .await;

            let http = this.http();
            let res = timeout(
                Duration::from_secs(300),
                crate::services::audio::execute_with_cancel(
                    &this.db,
                    &http,
                    this.secrets.as_ref(),
                    &this.data_dir,
                    input,
                    Some(id_clone.clone()),
                    Some(Arc::clone(&cancel_flag)),
                ),
            )
            .await;

            match res {
                Ok(Ok(result)) => {
                    let out_val = serde_json::to_value(&result).ok();
                    this.update_task_state(
                        &id_clone,
                        "audio",
                        "completed",
                        "completed",
                        Some(1.0),
                        Some("completed"),
                        Some("音频合成完成"),
                        out_val,
                        None,
                    )
                    .await;
                }
                Ok(Err(err)) => {
                    let is_cancelled =
                        cancel_flag.load(Ordering::Relaxed) || err.code == "cancelled";
                    if is_cancelled {
                        let (msg, retryable, remote_id, billing_risk) =
                            this.classify_cancellation(&id_clone, "audio").await;
                        this.update_task_state_ext(
                            &id_clone,
                            "audio",
                            "cancelled",
                            "cancelled",
                            None,
                            Some("cancelled"),
                            Some(&msg),
                            None,
                            None,
                            Some("cancelled"),
                            Some(retryable),
                            remote_id.as_deref(),
                            Some(&billing_risk),
                        )
                        .await;
                    } else {
                        let retryable = crate::resilience::is_retryable(&err);
                        this.update_task_state_ext(
                            &id_clone,
                            "audio",
                            "failed",
                            "failed",
                            None,
                            Some("failed"),
                            Some(&err.message),
                            None,
                            Some(&err.message),
                            Some(err.code),
                            Some(retryable),
                            None,
                            Some("none"),
                        )
                        .await;
                    }
                }
                Err(_) => {
                    let (err_msg, error_code, retryable, remote_id, billing_risk) =
                        this.classify_timeout_failure(&id_clone, "audio", 300).await;
                    let _ = crate::services::task_scheduler::update_task_status(
                        &this.db,
                        &id_clone,
                        TaskStatus::Failed,
                        Some(&err_msg),
                    )
                    .await;
                    this.update_task_state_ext(
                        &id_clone,
                        "audio",
                        "failed",
                        "failed",
                        None,
                        Some("failed"),
                        Some(&err_msg),
                        None,
                        Some(&err_msg),
                        Some(&error_code),
                        Some(retryable),
                        remote_id.as_deref(),
                        Some(&billing_risk),
                    )
                    .await;
                }
            }
            this.unregister_cancel(&id_clone, &cancel_flag);
        });

        Ok(id)
    }

    pub async fn submit_batch(
        self: &Arc<Self>,
        input: BatchGenerateInput,
        task_id: Option<String>,
    ) -> Result<String, AppError> {
        let (id, cancel_flag) = self
            .prepare_task_id(task_id, "batch", Some(&input.model_id))
            .await?;
        if let Err(err) = self
            .persist_queued_task(
                &id,
                "batch",
                &input.model_id,
                &serde_json::to_value(&input).unwrap_or_default(),
                &input.parameters,
            )
            .await
        {
            self.unregister_cancel(&id, &cancel_flag);
            return Err(err);
        }

        let this = Arc::clone(self);
        let id_clone = id.clone();

        this.update_task_state(
            &id,
            "batch",
            "queued",
            "queued",
            Some(0.0),
            Some("queued"),
            Some("批量任务已排队"),
            None,
            None,
        )
        .await;

        tokio::spawn(async move {
            let cancel_flag = Arc::clone(&cancel_flag);
            if this
                .stop_if_cancelled(&id_clone, "batch", &cancel_flag)
                .await
            {
                return;
            }
            this.update_task_state(
                &id_clone,
                "batch",
                "started",
                "running",
                Some(0.1),
                Some("running"),
                Some("批量生成中"),
                None,
                None,
            )
            .await;

            let http = this.http();
            let res = timeout(
                Duration::from_secs(600),
                crate::services::batch::execute(
                    &this.db,
                    &http,
                    this.secrets.as_ref(),
                    &this.data_dir,
                    input,
                    Some(Arc::clone(&cancel_flag)),
                    Some(Arc::clone(&this.scheduler)),
                ),
            )
            .await;

            let is_cancelled = cancel_flag.load(Ordering::Relaxed);
            if is_cancelled {
                let (msg, retryable, remote_id, billing_risk) =
                    this.classify_cancellation(&id_clone, "batch").await;
                this.update_task_state_ext(
                    &id_clone,
                    "batch",
                    "cancelled",
                    "cancelled",
                    None,
                    Some("cancelled"),
                    Some(&msg),
                    None,
                    None,
                    Some("cancelled"),
                    Some(retryable),
                    remote_id.as_deref(),
                    Some(&billing_risk),
                )
                .await;
            } else {
                match res {
                    Ok(Ok(result)) => {
                        let out_val = serde_json::to_value(&result).ok();
                        this.update_task_state(
                            &id_clone,
                            "batch",
                            "completed",
                            "completed",
                            Some(1.0),
                            Some("completed"),
                            Some("批量生成完成"),
                            out_val,
                            None,
                        )
                        .await;
                    }
                    Ok(Err(err)) => {
                        if err.code == "cancelled" {
                            let (msg, retryable, remote_id, billing_risk) =
                                this.classify_cancellation(&id_clone, "batch").await;
                            this.update_task_state_ext(
                                &id_clone,
                                "batch",
                                "cancelled",
                                "cancelled",
                                None,
                                Some("cancelled"),
                                Some(&msg),
                                None,
                                None,
                                Some("cancelled"),
                                Some(retryable),
                                remote_id.as_deref(),
                                Some(&billing_risk),
                            )
                            .await;
                        } else {
                            let retryable = crate::resilience::is_retryable(&err);
                            this.update_task_state_ext(
                                &id_clone,
                                "batch",
                                "failed",
                                "failed",
                                None,
                                Some("failed"),
                                Some(&err.message),
                                None,
                                Some(&err.message),
                                Some(err.code),
                                Some(retryable),
                                None,
                                Some("none"),
                            )
                            .await;
                        }
                    }
                    Err(_) => {
                        let (err_msg, error_code, retryable, remote_id, billing_risk) =
                            this.classify_timeout_failure(&id_clone, "batch", 600).await;
                        this.update_task_state_ext(
                            &id_clone,
                            "batch",
                            "failed",
                            "failed",
                            None,
                            Some("failed"),
                            Some(&err_msg),
                            None,
                            Some(&err_msg),
                            Some(&error_code),
                            Some(retryable),
                            remote_id.as_deref(),
                            Some(&billing_risk),
                        )
                        .await;
                    }
                }
            }
            this.unregister_cancel(&id_clone, &cancel_flag);
        });

        Ok(id)
    }

    pub async fn submit_pipeline(
        self: &Arc<Self>,
        input: PipelineExecutionInput,
        task_id: Option<String>,
    ) -> Result<String, AppError> {
        let (id, cancel_flag) = self.prepare_task_id(task_id, "pipeline", None).await?;
        let empty_params = serde_json::Map::new();
        if let Err(err) = self
            .persist_queued_task(
                &id,
                "pipeline",
                "pipeline",
                &serde_json::to_value(&input).unwrap_or_default(),
                &empty_params,
            )
            .await
        {
            self.unregister_cancel(&id, &cancel_flag);
            return Err(err);
        }

        let this = Arc::clone(self);
        let id_clone = id.clone();

        this.update_task_state(
            &id,
            "pipeline",
            "queued",
            "queued",
            Some(0.0),
            Some("queued"),
            Some("工作流已排队"),
            None,
            None,
        )
        .await;

        tokio::spawn(async move {
            let cancel_flag = Arc::clone(&cancel_flag);
            if this
                .stop_if_cancelled(&id_clone, "pipeline", &cancel_flag)
                .await
            {
                return;
            }
            this.update_task_state(
                &id_clone,
                "pipeline",
                "started",
                "running",
                Some(0.05),
                Some("running"),
                Some("工作流开始执行"),
                None,
                None,
            )
            .await;

            let queue_for_step = Arc::clone(&this);
            let id_for_step = id_clone.clone();
            let on_step = Arc::new(
                move |step_idx: usize,
                      total_steps: usize,
                      step_res: &crate::services::pipeline::PipelineStepResult| {
                    let progress = (step_idx + 1) as f64 / total_steps.max(1) as f64;
                    let msg = format!(
                        "步骤 {}/{} 完成: {}",
                        step_idx + 1,
                        total_steps,
                        step_res.step_label
                    );
                    let out = serde_json::to_value(step_res).ok();
                    queue_for_step.update_task_progress(TaskProgress {
                        task_id: &id_for_step,
                        modality: "pipeline",
                        event_type: "intermediate",
                        progress,
                        stage: &step_res.step_id,
                        message: &msg,
                        output: out,
                    });
                },
            );

            let http = this.http();
            let res = timeout(
                Duration::from_secs(600),
                crate::services::pipeline::run_with_cancel_and_callback(
                    &this.db,
                    &http,
                    this.secrets.as_ref(),
                    &this.data_dir,
                    input,
                    Some(Arc::clone(&cancel_flag)),
                    Some(on_step),
                    Some(&this.scheduler),
                ),
            )
            .await;

            let is_cancelled = cancel_flag.load(Ordering::Relaxed);
            if is_cancelled {
                let (msg, retryable, remote_id, billing_risk) =
                    this.classify_cancellation(&id_clone, "pipeline").await;
                this.update_task_state_ext(
                    &id_clone,
                    "pipeline",
                    "cancelled",
                    "cancelled",
                    None,
                    Some("cancelled"),
                    Some(&msg),
                    None,
                    None,
                    Some("cancelled"),
                    Some(retryable),
                    remote_id.as_deref(),
                    Some(&billing_risk),
                )
                .await;
            } else {
                match res {
                    Ok(Ok(result)) => {
                        let out_val = serde_json::to_value(&result).ok();
                        this.update_task_state(
                            &id_clone,
                            "pipeline",
                            "completed",
                            "completed",
                            Some(1.0),
                            Some("completed"),
                            Some("工作流执行完毕"),
                            out_val,
                            None,
                        )
                        .await;
                    }
                    Ok(Err(err)) => {
                        if err.code == "cancelled" {
                            let (msg, retryable, remote_id, billing_risk) =
                                this.classify_cancellation(&id_clone, "pipeline").await;
                            this.update_task_state_ext(
                                &id_clone,
                                "pipeline",
                                "cancelled",
                                "cancelled",
                                None,
                                Some("cancelled"),
                                Some(&msg),
                                None,
                                None,
                                Some("cancelled"),
                                Some(retryable),
                                remote_id.as_deref(),
                                Some(&billing_risk),
                            )
                            .await;
                        } else {
                            let retryable = crate::resilience::is_retryable(&err);
                            this.update_task_state_ext(
                                &id_clone,
                                "pipeline",
                                "failed",
                                "failed",
                                None,
                                Some("failed"),
                                Some(&err.message),
                                None,
                                Some(&err.message),
                                Some(err.code),
                                Some(retryable),
                                None,
                                Some("none"),
                            )
                            .await;
                        }
                    }
                    Err(_) => {
                        let (err_msg, error_code, retryable, remote_id, billing_risk) = this
                            .classify_timeout_failure(&id_clone, "pipeline", 600)
                            .await;
                        this.update_task_state_ext(
                            &id_clone,
                            "pipeline",
                            "failed",
                            "failed",
                            None,
                            Some("failed"),
                            Some(&err_msg),
                            None,
                            Some(&err_msg),
                            Some(&error_code),
                            Some(retryable),
                            remote_id.as_deref(),
                            Some(&billing_risk),
                        )
                        .await;
                    }
                }
            }
            this.unregister_cancel(&id_clone, &cancel_flag);
        });

        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::MemorySecretStore;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::db::migrate(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn terminal_queue_output_preserves_provider_response_and_terminal_status() {
        let pool = test_pool().await;
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO generation_tasks
             (id, modality, provider_id, model_id, status, input_json, parameters_json, raw_response_json, created_at)
             VALUES (?, 'image', 'provider', 'model', 'queued', '{}', '{}', '{\"provider\":\"raw response\"}', ?) ",
        )
        .bind(&id)
        .bind(now_iso())
        .execute(&pool)
        .await
        .unwrap();

        let queue = JobQueue::new(
            pool.clone(),
            Client::new(),
            Arc::new(MemorySecretStore::default()),
            std::env::temp_dir().join(format!("ai-workbench-{id}")),
            Arc::new(TaskScheduler::new()),
            Arc::new(Mutex::new(HashMap::new())),
            None,
        );
        queue
            .update_task_state(
                &id,
                "image",
                "completed",
                "completed",
                Some(1.0),
                Some("completed"),
                Some("done"),
                Some(serde_json::json!({"asset": "image.png"})),
                None,
            )
            .await;
        queue
            .update_task_state(
                &id,
                "image",
                "cancelled",
                "cancelled",
                None,
                Some("cancelled"),
                Some("late cancellation"),
                None,
                None,
            )
            .await;

        let row = sqlx::query(
            "SELECT status, raw_response_json, queue_output_json FROM generation_tasks WHERE id = ?",
        )
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let response: String = row.try_get("raw_response_json").unwrap();
        let output: String = row.try_get("queue_output_json").unwrap();
        let status: String = row.try_get("status").unwrap();
        assert_eq!(status, "completed");
        assert_eq!(response, r#"{"provider":"raw response"}"#);
        assert_eq!(
            serde_json::from_str::<Value>(&output).unwrap(),
            serde_json::json!({"asset": "image.png"})
        );
        assert_eq!(queue.get_task_info(&id).unwrap().status, "completed");
    }

    #[tokio::test]
    async fn test_classify_timeout_failure_scenarios() {
        let pool = test_pool().await;
        let id_submitting = "task-timeout-submitting";
        sqlx::query(
            "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, execution_phase, input_json, parameters_json, created_at)
             VALUES (?, 'image', 'p', 'm', 'running', 'submitting', '{}', '{}', ?) ",
        )
        .bind(id_submitting)
        .bind(now_iso())
        .execute(&pool)
        .await
        .unwrap();

        let id_running_remote = "task-timeout-remote";
        sqlx::query(
            "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, execution_phase, remote_job_id, input_json, parameters_json, created_at)
             VALUES (?, 'image', 'p', 'm', 'running', 'running', 'remote-xyz', '{}', '{}', ?) ",
        )
        .bind(id_running_remote)
        .bind(now_iso())
        .execute(&pool)
        .await
        .unwrap();

        let id_downloading = "task-timeout-download";
        sqlx::query(
            "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, execution_phase, input_json, parameters_json, created_at)
             VALUES (?, 'image', 'p', 'm', 'running', 'downloading', '{}', '{}', ?) ",
        )
        .bind(id_downloading)
        .bind(now_iso())
        .execute(&pool)
        .await
        .unwrap();

        let queue = JobQueue::new(
            pool.clone(),
            Client::new(),
            Arc::new(MemorySecretStore::default()),
            std::env::temp_dir().join(format!("ai-wb-{id_submitting}")),
            Arc::new(TaskScheduler::new()),
            Arc::new(Mutex::new(HashMap::new())),
            None,
        );

        // 1. submitting timeout
        let (msg1, code1, retry1, _remote1, risk1) = queue
            .classify_timeout_failure(id_submitting, "image", 60)
            .await;
        assert_eq!(code1, "submit_timeout");
        assert!(!retry1);
        assert_eq!(risk1, "possible");
        assert!(msg1.contains("提交超时"));

        // 2. remote generating timeout
        let (msg2, code2, retry2, remote2, risk2) = queue
            .classify_timeout_failure(id_running_remote, "image", 60)
            .await;
        assert_eq!(code2, "generation_timeout");
        assert!(!retry2);
        assert_eq!(remote2.as_deref(), Some("remote-xyz"));
        assert_eq!(risk2, "possible");
        assert!(msg2.contains("remote-xyz"));

        // 3. downloading timeout
        let (msg3, code3, retry3, _remote3, risk3) = queue
            .classify_timeout_failure(id_downloading, "image", 60)
            .await;
        assert_eq!(code3, "download_timeout");
        assert!(retry3);
        assert_eq!(risk3, "confirmed");
        assert!(msg3.contains("下载超时"));
    }

    #[tokio::test]
    async fn test_classify_cancellation_semantics() {
        let pool = test_pool().await;
        let id_queued = "task-cancel-queued";
        sqlx::query(
            "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, execution_phase, input_json, parameters_json, created_at)
             VALUES (?, 'video', 'p', 'm', 'queued', 'queued', '{}', '{}', ?) ",
        )
        .bind(id_queued)
        .bind(now_iso())
        .execute(&pool)
        .await
        .unwrap();

        let id_running = "task-cancel-running";
        sqlx::query(
            "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, execution_phase, remote_job_id, input_json, parameters_json, created_at)
             VALUES (?, 'video', 'p', 'm', 'running', 'running', 'rem-vid', '{}', '{}', ?) ",
        )
        .bind(id_running)
        .bind(now_iso())
        .execute(&pool)
        .await
        .unwrap();

        let queue = JobQueue::new(
            pool.clone(),
            Client::new(),
            Arc::new(MemorySecretStore::default()),
            std::env::temp_dir().join(format!("ai-wb-{id_queued}")),
            Arc::new(TaskScheduler::new()),
            Arc::new(Mutex::new(HashMap::new())),
            None,
        );

        // Queued cancellation: no billing risk
        let (msg1, retry1, _, risk1) = queue.classify_cancellation(id_queued, "video").await;
        assert!(msg1.contains("无费用"));
        assert!(retry1);
        assert_eq!(risk1, "none");

        // Running cancellation: explicit warning that remote provider might still charge
        let (msg2, retry2, remote2, risk2) = queue.classify_cancellation(id_running, "video").await;
        assert!(msg2.contains("可能仍在继续") && msg2.contains("产生费用"));
        assert!(!retry2);
        assert_eq!(remote2.as_deref(), Some("rem-vid"));
        assert_eq!(risk2, "possible");
    }

    #[tokio::test]
    async fn test_update_task_state_ext_persistence() {
        let pool = test_pool().await;
        let id = "task-state-ext";
        sqlx::query(
            "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, input_json, parameters_json, created_at)
             VALUES (?, 'image', 'p', 'm', 'queued', '{}', '{}', ?) ",
        )
        .bind(id)
        .bind(now_iso())
        .execute(&pool)
        .await
        .unwrap();

        let queue = JobQueue::new(
            pool.clone(),
            Client::new(),
            Arc::new(MemorySecretStore::default()),
            std::env::temp_dir().join(format!("ai-wb-{id}")),
            Arc::new(TaskScheduler::new()),
            Arc::new(Mutex::new(HashMap::new())),
            None,
        );

        queue
            .update_task_state_ext(
                id,
                "image",
                "failed",
                "failed",
                None,
                Some("failed"),
                Some("远程生成超时"),
                None,
                Some("远程生成超时"),
                Some("generation_timeout"),
                Some(false),
                Some("remote-job-999"),
                Some("possible"),
            )
            .await;

        let info = queue.get_task_info(id).unwrap();
        assert_eq!(info.status, "failed");
        assert_eq!(info.error_code.as_deref(), Some("generation_timeout"));
        assert_eq!(info.retryable, Some(false));
        assert_eq!(info.remote_job_id.as_deref(), Some("remote-job-999"));
        assert_eq!(info.billing_risk.as_deref(), Some("possible"));

        // Check DB row
        let row = sqlx::query(
            "SELECT status, error_code, retryable, remote_job_id, billing_risk FROM generation_tasks WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let ec: Option<String> = row.try_get("error_code").unwrap();
        let ret: Option<bool> = row.try_get("retryable").unwrap();
        let rem: Option<String> = row.try_get("remote_job_id").unwrap();
        let risk: Option<String> = row.try_get("billing_risk").unwrap();
        assert_eq!(ec.as_deref(), Some("generation_timeout"));
        assert_eq!(ret, Some(false));
        assert_eq!(rem.as_deref(), Some("remote-job-999"));
        assert_eq!(risk.as_deref(), Some("possible"));
    }
}
