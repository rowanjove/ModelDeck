use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::domain::{now_iso, Modality};
use crate::error::AppError;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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
            _ => None,
        }
    }

    pub const ALL: [TaskStatus; 9] = [
        Self::Queued,
        Self::Submitting,
        Self::Running,
        Self::Downloading,
        Self::Completed,
        Self::Failed,
        Self::Cancelled,
        Self::Detached,
        Self::Interrupted,
    ];

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }

    pub fn can_transition_to(&self, next: TaskStatus) -> bool {
        if *self == next {
            return true;
        }
        // 终态不可逆流转回进行状态
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

/// 启动时崩溃恢复：原子标记未完成的任务为中断
pub async fn recover_tasks_on_startup(pool: &SqlitePool) -> Result<usize, AppError> {
    let now = now_iso();
    let result = sqlx::query(
        "UPDATE generation_tasks
         SET status = 'interrupted',
             error = '应用异常退出，未完成的任务已标记中断',
             completed_at = ?
         WHERE status IN ('claimed', 'queued', 'submitting', 'running', 'downloading')",
    )
    .bind(&now)
    .execute(pool)
    .await
    .map_err(AppError::db)?;

    Ok(result.rows_affected() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn test_valid_task_transitions() {
        assert!(TaskStatus::Queued.can_transition_to(TaskStatus::Submitting));
        assert!(TaskStatus::Submitting.can_transition_to(TaskStatus::Running));
        assert!(TaskStatus::Running.can_transition_to(TaskStatus::Downloading));
        assert!(TaskStatus::Downloading.can_transition_to(TaskStatus::Completed));
        assert!(TaskStatus::Running.can_transition_to(TaskStatus::Cancelled));
        assert!(TaskStatus::Running.can_transition_to(TaskStatus::Failed));
    }

    #[test]
    fn test_invalid_task_transitions() {
        // 终态不可回到运行态
        assert!(!TaskStatus::Completed.can_transition_to(TaskStatus::Running));
        assert!(!TaskStatus::Failed.can_transition_to(TaskStatus::Running));
        assert!(!TaskStatus::Cancelled.can_transition_to(TaskStatus::Submitting));
        assert!(!TaskStatus::Interrupted.can_transition_to(TaskStatus::Downloading));
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
