use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::domain::{now_iso, Modality};
use crate::error::AppError;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Submitting,
    Running,
    Downloading,
    Completed,
    Failed,
    Cancelled,
    Detached,
    Interrupted,
    UnknownSubmission,
}

#[allow(dead_code)]
impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Submitting => "submitting",
            Self::Running => "running",
            Self::Downloading => "downloading",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Detached => "detached",
            Self::Interrupted => "interrupted",
            Self::UnknownSubmission => "unknown_submission",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "queued" => Some(Self::Queued),
            "submitting" => Some(Self::Submitting),
            "running" => Some(Self::Running),
            "downloading" => Some(Self::Downloading),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            "detached" => Some(Self::Detached),
            "interrupted" => Some(Self::Interrupted),
            "unknown_submission" => Some(Self::UnknownSubmission),
            _ => None,
        }
    }

    pub const ALL: [TaskStatus; 10] = [
        Self::Queued,
        Self::Submitting,
        Self::Running,
        Self::Downloading,
        Self::Completed,
        Self::Failed,
        Self::Cancelled,
        Self::Detached,
        Self::Interrupted,
        Self::UnknownSubmission,
    ];

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed
                | Self::Failed
                | Self::Cancelled
                | Self::Interrupted
                | Self::UnknownSubmission
        )
    }

    pub fn can_transition_to(&self, next: TaskStatus) -> bool {
        if *self == next {
            return true;
        }
        // 终态不可逆流转回进行状态或其它状态
        if self.is_terminal() {
            return false;
        }
        match self {
            Self::Queued => matches!(
                next,
                Self::Submitting | Self::Cancelled | Self::Failed | Self::Interrupted
            ),
            Self::Submitting => matches!(
                next,
                Self::Running
                    | Self::Downloading
                    | Self::Completed
                    | Self::Cancelled
                    | Self::Failed
                    | Self::Interrupted
                    | Self::UnknownSubmission
            ),
            Self::Running => matches!(
                next,
                Self::Downloading
                    | Self::Completed
                    | Self::Cancelled
                    | Self::Failed
                    | Self::Detached
                    | Self::Interrupted
            ),
            Self::Downloading => matches!(
                next,
                Self::Completed | Self::Cancelled | Self::Failed | Self::Interrupted
            ),
            Self::Detached => matches!(
                next,
                Self::Running
                    | Self::Completed
                    | Self::Cancelled
                    | Self::Failed
                    | Self::Interrupted
            ),
            _ => false,
        }
    }
}

pub struct TaskScheduler {
    text_sem: Arc<Semaphore>,
    image_sem: Arc<Semaphore>,
    video_sem: Arc<Semaphore>,
    audio_sem: Arc<Semaphore>,
    comfyui_sem: Arc<Semaphore>,
}

pub struct SchedulerPermit {
    _modality_permit: OwnedSemaphorePermit,
    _comfyui_permit: Option<OwnedSemaphorePermit>,
}

impl Default for TaskScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskScheduler {
    pub fn new() -> Self {
        Self {
            text_sem: Arc::new(Semaphore::new(6)),
            image_sem: Arc::new(Semaphore::new(3)),
            video_sem: Arc::new(Semaphore::new(2)),
            audio_sem: Arc::new(Semaphore::new(3)),
            comfyui_sem: Arc::new(Semaphore::new(1)),
        }
    }

    pub async fn acquire(
        &self,
        modality: Modality,
        is_comfyui: bool,
    ) -> Result<SchedulerPermit, AppError> {
        self.acquire_cancellable(modality, is_comfyui, None).await
    }

    pub async fn acquire_cancellable(
        &self,
        modality: Modality,
        is_comfyui: bool,
        cancel: Option<&AtomicBool>,
    ) -> Result<SchedulerPermit, AppError> {
        let modality_sem = match modality {
            Modality::Text => &self.text_sem,
            Modality::Image => &self.image_sem,
            Modality::Video => &self.video_sem,
            Modality::Audio => &self.audio_sem,
        };

        let comfyui_permit = if is_comfyui {
            Some(acquire_owned(Arc::clone(&self.comfyui_sem), cancel).await?)
        } else {
            None
        };
        // Reserve the globally scarce ComfyUI lane first. Otherwise ComfyUI
        // waiters can consume every modality permit while doing no work.
        let modality_permit = acquire_owned(Arc::clone(modality_sem), cancel).await?;

        Ok(SchedulerPermit {
            _modality_permit: modality_permit,
            _comfyui_permit: comfyui_permit,
        })
    }
}

async fn acquire_owned(
    semaphore: Arc<Semaphore>,
    cancel: Option<&AtomicBool>,
) -> Result<OwnedSemaphorePermit, AppError> {
    let Some(cancel) = cancel else {
        return semaphore
            .acquire_owned()
            .await
            .map_err(|_| AppError::new("scheduler_closed", "TaskScheduler 信号量已关闭"));
    };

    let acquire = semaphore.acquire_owned();
    tokio::pin!(acquire);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::new("cancelled", "任务已取消"));
        }
        tokio::select! {
            permit = &mut acquire => {
                let permit = permit
                    .map_err(|_| AppError::new("scheduler_closed", "TaskScheduler 信号量已关闭"))?;
                if cancel.load(Ordering::Relaxed) {
                    drop(permit);
                    return Err(AppError::new("cancelled", "任务已取消"));
                }
                return Ok(permit);
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
        }
    }
}

/// 原子 CAS 状态更新并严格检查状态转移合法性
#[allow(dead_code)]
pub async fn update_task_status(
    pool: &SqlitePool,
    task_id: &str,
    next_status: TaskStatus,
    error: Option<&str>,
) -> Result<(), AppError> {
    let valid_sources: Vec<&'static str> = TaskStatus::ALL
        .iter()
        .copied()
        .filter(|s| s.can_transition_to(next_status))
        .map(|s| s.as_str())
        .collect();

    if valid_sources.is_empty() {
        return Err(AppError::validation(format!(
            "目标状态不可达: {}",
            next_status.as_str()
        )));
    }

    let now = now_iso();
    let is_terminal = next_status.is_terminal();
    let completed_at = if is_terminal {
        Some(now.as_str())
    } else {
        None
    };

    let placeholders = vec!["?"; valid_sources.len()].join(",");
    let sql = format!(
        "UPDATE generation_tasks 
         SET status = ?, error = COALESCE(?, error), completed_at = COALESCE(?, completed_at)
         WHERE id = ? AND status IN ({})",
        placeholders
    );

    let mut query = sqlx::query(&sql)
        .bind(next_status.as_str())
        .bind(error)
        .bind(completed_at)
        .bind(task_id);

    for s in valid_sources {
        query = query.bind(s);
    }

    let res = query.execute(pool).await.map_err(AppError::db)?;
    if res.rows_affected() == 0 {
        let current_str: Option<String> =
            sqlx::query_scalar("SELECT status FROM generation_tasks WHERE id = ?")
                .bind(task_id)
                .fetch_optional(pool)
                .await
                .map_err(AppError::db)?;

        match current_str {
            Some(curr) => {
                return Err(AppError::validation(format!(
                    "非法任务状态转换: 任务当前状态为 '{}'，无法转换至 '{}'",
                    curr,
                    next_status.as_str()
                )));
            }
            None => {
                return Err(AppError::validation(format!("任务不存在: {}", task_id)));
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryReport {
    pub total_recovered: usize,
    pub interrupted_queued: usize,
    pub unknown_submissions: usize,
    pub interrupted_with_remote_id: usize,
    pub interrupted_unrecoverable: usize,
    pub interrupted_downloads: usize,
}

/// 记录远端任务唯一标识（如 Replicate prediction id, fal request id）及轮询 URL
#[allow(dead_code)]
pub async fn record_remote_job(
    pool: &SqlitePool,
    task_id: &str,
    remote_job_id: &str,
    remote_poll_url: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE generation_tasks 
         SET remote_job_id = ?, remote_poll_url = COALESCE(?, remote_poll_url),
             execution_phase = 'running', billing_risk = 'none'
         WHERE id = ? AND status NOT IN ('completed', 'failed', 'cancelled', 'interrupted', 'unknown_submission')",
    )
    .bind(remote_job_id)
    .bind(remote_poll_url)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

/// 更新任务执行阶段与潜在计费风险
#[allow(dead_code)]
pub async fn update_task_phase(
    pool: &SqlitePool,
    task_id: &str,
    phase: &str,
    billing_risk: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE generation_tasks 
         SET execution_phase = ?, billing_risk = COALESCE(?, billing_risk)
         WHERE id = ? AND status NOT IN ('completed', 'failed', 'cancelled', 'interrupted', 'unknown_submission')",
    )
    .bind(phase)
    .bind(billing_risk)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

/// 启动时崩溃恢复：精准区分本地队列、提交未决、可续查远端 ID、下载中断与不可恢复任务
pub async fn recover_tasks_on_startup(pool: &SqlitePool) -> Result<RecoveryReport, AppError> {
    let now = now_iso();
    let mut tx = pool.begin().await.map_err(AppError::db)?;
    let mut report = RecoveryReport::default();

    // 1. 本地队列检查 (status IN ('claimed', 'queued'))：
    // 未向服务商发起请求。如果任务已被用户取消，保持/标记为 cancelled；
    // 内存中的执行 future 无法跨进程恢复，必须收敛为 interrupted，避免永久排队。
    let _ = sqlx::query(
        "UPDATE generation_tasks
         SET status = 'cancelled', execution_phase = 'cancelled', billing_risk = 'none', completed_at = ?
         WHERE status IN ('claimed', 'queued') AND error LIKE '%取消%'",
    )
    .bind(&now)
    .execute(&mut *tx)
    .await
    .map_err(AppError::db)?;

    let queued_res = sqlx::query(
        "UPDATE generation_tasks
         SET status = 'interrupted', execution_phase = 'interrupted_before_submission', billing_risk = 'none',
             error = '应用退出时任务尚未提交，排队已中断。请手动重新发起任务。', completed_at = ?, retryable = 1
         WHERE status IN ('claimed', 'queued') AND status != 'cancelled'",
    )
    .bind(&now)
    .execute(&mut *tx)
    .await
    .map_err(AppError::db)?;
    report.interrupted_queued = queued_res.rows_affected() as usize;

    // 2. 已发出请求但尚未收到远端任务 ID ('submitting' 且 remote_job_id IS NULL)
    // 标记为 unknown_submission，默认不自动重发，防止二次扣费
    let unknown_res = sqlx::query(
        "UPDATE generation_tasks
         SET status = 'unknown_submission',
             execution_phase = 'submitted_unknown',
             billing_risk = 'possible',
             error = '任务已向服务商提交，但进程在获取远端标识前中断。为防止重复扣费，系统默认不自动重发。如需继续请在服务商控制台核验后手动重试（可能再次计费）。',
             completed_at = ?
         WHERE (status = 'submitting' OR execution_phase = 'submitting')
           AND (remote_job_id IS NULL OR trim(remote_job_id) = '')
           AND status NOT IN ('completed', 'failed', 'cancelled', 'interrupted', 'unknown_submission')",
    )
    .bind(&now)
    .execute(&mut *tx)
    .await
    .map_err(AppError::db)?;
    report.unknown_submissions = unknown_res.rows_affected() as usize;

    // 3. 已获得可复查的远端任务 ID (remote_job_id IS NOT NULL 且处于 running/downloading/submitting)
    // 明确记录远端任务 ID，服务商支持断点续查，不会重复扣费
    let remote_res = sqlx::query(
        "UPDATE generation_tasks
         SET status = 'interrupted',
             execution_phase = 'interrupted_with_remote_id',
             billing_risk = 'none',
             error = '任务已获得远端任务 ID [' || remote_job_id || ']，因应用退出中断。服务商支持通过远端任务 ID 续查结果，不会重复计费。',
             completed_at = ?
         WHERE remote_job_id IS NOT NULL
           AND trim(remote_job_id) != ''
           AND status IN ('running', 'downloading', 'submitting')
           AND status NOT IN ('completed', 'failed', 'cancelled', 'interrupted', 'unknown_submission')",
    )
    .bind(&now)
    .execute(&mut *tx)
    .await
    .map_err(AppError::db)?;
    report.interrupted_with_remote_id = remote_res.rows_affected() as usize;

    // 4. 下载阶段退出的任务 (status = 'downloading' 或 execution_phase = 'downloading'，且 remote_job_id IS NULL)
    let download_res = sqlx::query(
        "UPDATE generation_tasks
         SET status = 'interrupted',
             execution_phase = 'interrupted_download',
             billing_risk = 'none',
             error = '应用在下载生成产物时退出。远端生成已完成，请核验产物或重新下载，无需重新调用生成 API。',
             completed_at = ?
         WHERE (status = 'downloading' OR execution_phase = 'downloading')
           AND (remote_job_id IS NULL OR trim(remote_job_id) = '')
           AND status NOT IN ('completed', 'failed', 'cancelled', 'interrupted', 'unknown_submission')",
    )
    .bind(&now)
    .execute(&mut *tx)
    .await
    .map_err(AppError::db)?;
    report.interrupted_downloads = download_res.rows_affected() as usize;

    // 5. 不支持断点续查的运行中任务 (status = 'running' 且 remote_job_id IS NULL)
    let unrecov_res = sqlx::query(
        "UPDATE generation_tasks
         SET status = 'interrupted',
             execution_phase = 'interrupted_unrecoverable',
             billing_risk = 'possible',
             error = '应用异常退出，该服务商不支持断点续查。任务已中断；如需重新生成请手动重试，可能会再次计费。',
             completed_at = ?
         WHERE status = 'running'
           AND (remote_job_id IS NULL OR trim(remote_job_id) = '')
           AND status NOT IN ('completed', 'failed', 'cancelled', 'interrupted', 'unknown_submission')",
    )
    .bind(&now)
    .execute(&mut *tx)
    .await
    .map_err(AppError::db)?;
    report.interrupted_unrecoverable = unrecov_res.rows_affected() as usize;

    tx.commit().await.map_err(AppError::db)?;

    report.total_recovered = report.interrupted_queued
        + report.unknown_submissions
        + report.interrupted_with_remote_id
        + report.interrupted_downloads
        + report.interrupted_unrecoverable;

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    use sqlx::Row;

    #[tokio::test]
    async fn test_recovery_scenario_1_unsubmitted_queued_restart() {
        let dir = std::env::temp_dir().join(format!("aiwb-test-rec1-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let pool = crate::db::open(&dir.join("test.db")).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();

        let now = now_iso();
        // 本地排队任务，尚未发起任何网络请求
        sqlx::query(
            "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, input_json, parameters_json, created_at)
             VALUES ('task_queued_1', 'video', 'prov1', 'model1', 'queued', '{}', '{}', ?)"
        )
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let report = recover_tasks_on_startup(&pool).await.unwrap();
        assert_eq!(report.interrupted_queued, 1);

        // 无执行 future 的队列任务必须成为终态，并提示用户手动重新发起。
        let row = sqlx::query("SELECT status, execution_phase, billing_risk, error FROM generation_tasks WHERE id = 'task_queued_1'")
            .fetch_one(&pool).await.unwrap();
        let status: String = row.get("status");
        let phase: Option<String> = row.get("execution_phase");
        let billing_risk: Option<String> = row.get("billing_risk");
        let error: Option<String> = row.get("error");

        assert_eq!(status, "interrupted");
        assert_eq!(phase.as_deref(), Some("interrupted_before_submission"));
        assert_eq!(billing_risk.as_deref(), Some("none"));
        assert!(error.unwrap().contains("尚未提交"));
        assert_eq!(
            recover_tasks_on_startup(&pool)
                .await
                .unwrap()
                .total_recovered,
            0
        );

        sqlx::query("INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, input_json, parameters_json, created_at) VALUES ('task_claimed_1', 'video', 'prov1', 'model1', 'claimed', '{}', '{}', ?)")
            .bind(&now).execute(&pool).await.unwrap();
        assert_eq!(
            recover_tasks_on_startup(&pool)
                .await
                .unwrap()
                .interrupted_queued,
            1
        );
        let claimed_status: String =
            sqlx::query_scalar("SELECT status FROM generation_tasks WHERE id = 'task_claimed_1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(claimed_status, "interrupted");
    }

    #[tokio::test]
    async fn test_recovery_scenario_2_submitting_network_break() {
        let dir = std::env::temp_dir().join(format!("aiwb-test-rec2-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let pool = crate::db::open(&dir.join("test.db")).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();

        let now = now_iso();
        // 已发出请求但断网或崩溃，尚未收到远端任务 ID
        sqlx::query(
            "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, execution_phase, input_json, parameters_json, created_at)
             VALUES ('task_submitting_1', 'video', 'prov1', 'model1', 'submitting', 'submitting', '{}', '{}', ?)"
        )
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let report = recover_tasks_on_startup(&pool).await.unwrap();
        assert_eq!(report.unknown_submissions, 1);

        // 验证最终状态：标记为 unknown_submission，默认不自动重发，防止二次扣费
        let row = sqlx::query("SELECT status, execution_phase, billing_risk, error, completed_at FROM generation_tasks WHERE id = 'task_submitting_1'")
            .fetch_one(&pool).await.unwrap();
        let status: String = row.get("status");
        let phase: Option<String> = row.get("execution_phase");
        let billing_risk: Option<String> = row.get("billing_risk");
        let error: Option<String> = row.get("error");
        let completed_at: Option<String> = row.get("completed_at");

        assert_eq!(status, "unknown_submission");
        assert_eq!(phase.as_deref(), Some("submitted_unknown"));
        assert_eq!(billing_risk.as_deref(), Some("possible"));
        assert!(completed_at.is_some());
        assert!(error.unwrap().contains("默认不自动重发"));
    }

    #[tokio::test]
    async fn test_recovery_scenario_3_running_with_remote_job_id_restart() {
        let dir = std::env::temp_dir().join(format!("aiwb-test-rec3-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let pool = crate::db::open(&dir.join("test.db")).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();

        let now = now_iso();
        // 已取得远端任务 ID 并处于运行中
        sqlx::query(
            "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, remote_job_id, remote_poll_url, input_json, parameters_json, created_at)
             VALUES ('task_remote_1', 'video', 'replicate', 'wan-2.1', 'running', 'pred_987654', 'https://api.replicate.com/v1/predictions/pred_987654', '{}', '{}', ?)"
        )
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let report = recover_tasks_on_startup(&pool).await.unwrap();
        assert_eq!(report.interrupted_with_remote_id, 1);

        // 验证最终状态：保留 remote_job_id，指示可续查，无重复扣费风险
        let row = sqlx::query("SELECT status, execution_phase, remote_job_id, billing_risk, error FROM generation_tasks WHERE id = 'task_remote_1'")
            .fetch_one(&pool).await.unwrap();
        let status: String = row.get("status");
        let phase: Option<String> = row.get("execution_phase");
        let remote_id: Option<String> = row.get("remote_job_id");
        let billing_risk: Option<String> = row.get("billing_risk");
        let error: Option<String> = row.get("error");

        assert_eq!(status, "interrupted");
        assert_eq!(phase.as_deref(), Some("interrupted_with_remote_id"));
        assert_eq!(remote_id.as_deref(), Some("pred_987654"));
        assert_eq!(billing_risk.as_deref(), Some("none"));
        assert!(error.unwrap().contains("pred_987654"));
    }

    #[tokio::test]
    async fn test_recovery_scenario_4_downloading_exit_restart() {
        let dir = std::env::temp_dir().join(format!("aiwb-test-rec4-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let pool = crate::db::open(&dir.join("test.db")).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();

        let now = now_iso();
        // 下载时退出
        sqlx::query(
            "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, execution_phase, input_json, parameters_json, created_at)
             VALUES ('task_dl_1', 'video', 'fal', 'fast-svd', 'downloading', 'downloading', '{}', '{}', ?)"
        )
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let report = recover_tasks_on_startup(&pool).await.unwrap();
        assert_eq!(report.interrupted_downloads, 1);

        // 验证最终状态：下载中断，远端已生成，无二次计费
        let row = sqlx::query("SELECT status, execution_phase, billing_risk, error FROM generation_tasks WHERE id = 'task_dl_1'")
            .fetch_one(&pool).await.unwrap();
        let status: String = row.get("status");
        let phase: Option<String> = row.get("execution_phase");
        let billing_risk: Option<String> = row.get("billing_risk");
        let error: Option<String> = row.get("error");

        assert_eq!(status, "interrupted");
        assert_eq!(phase.as_deref(), Some("interrupted_download"));
        assert_eq!(billing_risk.as_deref(), Some("none"));
        assert!(error.unwrap().contains("无需重新调用生成 API"));
    }

    #[tokio::test]
    async fn test_recovery_scenario_5_idempotent_duplicate_app_launches() {
        let dir = std::env::temp_dir().join(format!("aiwb-test-rec5-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let pool = crate::db::open(&dir.join("test.db")).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();

        let now = now_iso();
        // 插入混合场景
        sqlx::query("INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, input_json, parameters_json, created_at) VALUES ('t1', 'video', 'p', 'm', 'queued', '{}', '{}', ?)")
            .bind(&now).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, input_json, parameters_json, created_at) VALUES ('t2', 'video', 'p', 'm', 'submitting', '{}', '{}', ?)")
            .bind(&now).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, remote_job_id, input_json, parameters_json, created_at) VALUES ('t3', 'video', 'p', 'm', 'running', 'r_123', '{}', '{}', ?)")
            .bind(&now).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, input_json, parameters_json, created_at) VALUES ('t4', 'video', 'p', 'm', 'completed', '{}', '{}', ?)")
            .bind(&now).execute(&pool).await.unwrap();

        // 第一次启动恢复
        let report1 = recover_tasks_on_startup(&pool).await.unwrap();
        assert_eq!(report1.total_recovered, 3);

        // 第二次重复打开应用
        let report2 = recover_tasks_on_startup(&pool).await.unwrap();
        // 所有遗留任务都已收敛到终态，重复恢复不应再修改任务。
        assert_eq!(report2.total_recovered, 0);
        assert_eq!(report2.unknown_submissions, 0);
        assert_eq!(report2.interrupted_with_remote_id, 0);
        assert_eq!(report2.interrupted_downloads, 0);
        assert_eq!(report2.interrupted_unrecoverable, 0);
        // 重复调用幂等，终态绝不回退
        let t2_status: String =
            sqlx::query_scalar("SELECT status FROM generation_tasks WHERE id = 't2'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(t2_status, "unknown_submission");
        let t3_status: String =
            sqlx::query_scalar("SELECT status FROM generation_tasks WHERE id = 't3'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(t3_status, "interrupted");
        let t4_status: String =
            sqlx::query_scalar("SELECT status FROM generation_tasks WHERE id = 't4'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(t4_status, "completed");
    }

    #[tokio::test]
    async fn test_recovery_scenario_6_completed_task_remains_intact() {
        let dir = std::env::temp_dir().join(format!("aiwb-test-rec6-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let pool = crate::db::open(&dir.join("test.db")).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();

        let now = now_iso();
        // 已完成并落盘
        sqlx::query(
            "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, input_json, parameters_json, queue_output_json, created_at, completed_at)
             VALUES ('task_done', 'video', 'p', 'm', 'completed', '{}', '{}', '{\"videos\":[{\"path\":\"v.mp4\"}]}', ?, ?)"
        )
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let _ = recover_tasks_on_startup(&pool).await.unwrap();

        // 终态不可逆：不能重复写入或修改
        let row = sqlx::query("SELECT status, queue_output_json, completed_at FROM generation_tasks WHERE id = 'task_done'")
            .fetch_one(&pool).await.unwrap();
        let status: String = row.get("status");
        let output: Option<String> = row.get("queue_output_json");
        let completed_at: Option<String> = row.get("completed_at");

        assert_eq!(status, "completed");
        assert!(output.unwrap().contains("v.mp4"));
        assert!(completed_at.is_some());
    }

    #[tokio::test]
    async fn test_recovery_scenario_7_cancelled_task_remains_cancelled() {
        let dir = std::env::temp_dir().join(format!("aiwb-test-rec7-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let pool = crate::db::open(&dir.join("test.db")).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();

        let now = now_iso();
        // 任务在退出前已取消
        sqlx::query(
            "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, error, input_json, parameters_json, created_at, completed_at)
             VALUES ('task_cancel', 'video', 'p', 'm', 'cancelled', '任务已取消', '{}', '{}', ?, ?)"
        )
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let _ = recover_tasks_on_startup(&pool).await.unwrap();

        // 终态保持 cancelled，不回退
        let status: String =
            sqlx::query_scalar("SELECT status FROM generation_tasks WHERE id = 'task_cancel'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "cancelled");
    }

    #[tokio::test]
    async fn test_record_remote_job_updates_database() {
        let dir = std::env::temp_dir().join(format!("aiwb-test-rec8-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let pool = crate::db::open(&dir.join("test.db")).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();

        let now = now_iso();
        sqlx::query(
            "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, input_json, parameters_json, created_at)
             VALUES ('task_test_rec', 'video', 'p', 'm', 'running', '{}', '{}', ?)"
        )
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        record_remote_job(
            &pool,
            "task_test_rec",
            "remote_xyz_777",
            Some("https://example.com/poll/777"),
        )
        .await
        .unwrap();

        let row = sqlx::query("SELECT remote_job_id, remote_poll_url, execution_phase, billing_risk FROM generation_tasks WHERE id = 'task_test_rec'")
            .fetch_one(&pool).await.unwrap();
        let remote_id: Option<String> = row.get("remote_job_id");
        let poll_url: Option<String> = row.get("remote_poll_url");
        let phase: Option<String> = row.get("execution_phase");
        let risk: Option<String> = row.get("billing_risk");

        assert_eq!(remote_id.as_deref(), Some("remote_xyz_777"));
        assert_eq!(poll_url.as_deref(), Some("https://example.com/poll/777"));
        assert_eq!(phase.as_deref(), Some("running"));
        assert_eq!(risk.as_deref(), Some("none"));
    }

    #[test]
    fn test_valid_task_transitions() {
        assert!(TaskStatus::Queued.can_transition_to(TaskStatus::Submitting));
        assert!(TaskStatus::Submitting.can_transition_to(TaskStatus::Running));
        assert!(TaskStatus::Submitting.can_transition_to(TaskStatus::UnknownSubmission));
        assert!(TaskStatus::Running.can_transition_to(TaskStatus::Downloading));
        assert!(TaskStatus::Downloading.can_transition_to(TaskStatus::Completed));
        assert!(TaskStatus::Running.can_transition_to(TaskStatus::Cancelled));
        assert!(TaskStatus::Running.can_transition_to(TaskStatus::Failed));
    }

    #[test]
    fn test_invalid_task_transitions() {
        // 终态不可回到运行态或其它非终态
        assert!(!TaskStatus::Completed.can_transition_to(TaskStatus::Running));
        assert!(!TaskStatus::Failed.can_transition_to(TaskStatus::Running));
        assert!(!TaskStatus::Cancelled.can_transition_to(TaskStatus::Submitting));
        assert!(!TaskStatus::Interrupted.can_transition_to(TaskStatus::Downloading));
        assert!(!TaskStatus::UnknownSubmission.can_transition_to(TaskStatus::Running));
        assert!(!TaskStatus::UnknownSubmission.can_transition_to(TaskStatus::Queued));
    }

    #[tokio::test]
    async fn cancellation_interrupts_a_waiting_scheduler_acquire() {
        let scheduler = Arc::new(TaskScheduler::new());
        let mut held = Vec::new();
        for _ in 0..3 {
            held.push(scheduler.acquire(Modality::Audio, false).await.unwrap());
        }

        let cancel = Arc::new(AtomicBool::new(false));
        let waiting_scheduler = Arc::clone(&scheduler);
        let waiting_cancel = Arc::clone(&cancel);
        let waiting = tokio::spawn(async move {
            waiting_scheduler
                .acquire_cancellable(Modality::Audio, false, Some(&waiting_cancel))
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        cancel.store(true, Ordering::Relaxed);

        let result = tokio::time::timeout(std::time::Duration::from_millis(250), waiting)
            .await
            .expect("cancellation should release the waiting task")
            .expect("waiting task should not panic");
        let err = match result {
            Err(err) => err,
            Ok(_) => panic!("cancelled acquisition must not obtain a permit"),
        };
        assert_eq!(err.code, "cancelled");
        drop(held);
    }

    #[tokio::test]
    async fn comfyui_waiters_do_not_exhaust_modality_capacity() {
        let scheduler = Arc::new(TaskScheduler::new());
        let _comfy_slot = scheduler.acquire(Modality::Audio, true).await.unwrap();

        let mut waiting = Vec::new();
        for _ in 0..3 {
            let scheduler = Arc::clone(&scheduler);
            waiting.push(tokio::spawn(async move {
                scheduler.acquire(Modality::Image, true).await
            }));
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;

        let regular_image = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            scheduler.acquire(Modality::Image, false),
        )
        .await
        .expect("ComfyUI waiters must not hold image permits")
        .expect("regular image task should acquire an image permit");
        drop(regular_image);
        drop(_comfy_slot);

        for waiter in waiting {
            let permit = tokio::time::timeout(std::time::Duration::from_millis(250), waiter)
                .await
                .expect("waiting task should resume")
                .expect("waiting task should not panic")
                .expect("ComfyUI permit should become available");
            drop(permit);
        }
    }
}
