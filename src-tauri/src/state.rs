use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use reqwest::Client;
use sqlx::SqlitePool;

use crate::credentials::{KeyringSecretStore, SecretStore};
use crate::db;
use crate::error::AppError;
use crate::services::network;

pub struct AppState {
    pub db: SqlitePool,
    http: RwLock<Client>,
    pub secrets: Arc<dyn SecretStore>,
    pub cancels: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    pub scheduler: Arc<crate::services::task_scheduler::TaskScheduler>,
    #[allow(dead_code)]
    pub breakers: Arc<crate::resilience::CircuitBreakerRegistry>,
    pub logs: Arc<crate::logging::LogBuffer>,
    pub data_dir: PathBuf,
    pub job_queue: Arc<crate::services::job_queue::JobQueue>,
}

impl AppState {
    pub async fn initialize(dir: &Path) -> Result<Self, AppError> {
        crate::storage_util::ensure_dir_writable(dir)?;
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|err| AppError::storage(format!("无法创建数据目录: {err}")))?;
        for name in ["images", "videos", "audio", "files"] {
            tokio::fs::create_dir_all(dir.join(name))
                .await
                .map_err(|err| AppError::storage(format!("无法创建数据目录: {err}")))?;
        }
        let db = db::open(&dir.join("app.db")).await?;
        db::migrate(&db).await?;

        // Crash Recovery: 启动时恢复未完成的任务，避免上次异常退出导致任务永久处于运行中状态
        crate::services::task_scheduler::recover_tasks_on_startup(&db).await?;

        let proxy_settings = network::get_proxy_settings(&db).await.unwrap_or_default();
        let http = network::build_client_with_proxy(&proxy_settings).unwrap_or_else(|_| {
            Client::builder()
                .user_agent("ai-workbench/0.1")
                .redirect(network::same_origin_redirect_policy())
                .build()
                .unwrap_or_default()
        });
        let breakers = crate::resilience::global_breakers();
        let logs = crate::logging::global_logger();
        let cancels = Arc::new(Mutex::new(HashMap::new()));
        let scheduler = Arc::new(crate::services::task_scheduler::TaskScheduler::new());
        let secrets: Arc<dyn SecretStore> = Arc::new(KeyringSecretStore);

        let job_queue = crate::services::job_queue::JobQueue::new(
            db.clone(),
            http.clone(),
            Arc::clone(&secrets),
            dir.to_path_buf(),
            Arc::clone(&scheduler),
            Arc::clone(&cancels),
            None,
        );
        job_queue.hydrate_recent_tasks().await?;

        Ok(Self {
            db,
            http: RwLock::new(http),
            secrets,
            cancels,
            scheduler,
            breakers,
            logs,
            data_dir: dir.to_path_buf(),
            job_queue,
        })
    }

    pub fn http(&self) -> Client {
        self.http
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    pub fn update_http(&self, client: Client) {
        let mut guard = self.http.write().unwrap_or_else(|err| err.into_inner());
        *guard = client;
    }

    pub fn register_cancel(&self, id: &str) -> Result<Arc<AtomicBool>, AppError> {
        let flag = Arc::new(AtomicBool::new(false));
        let mut map = self.cancels.lock().unwrap_or_else(|err| err.into_inner());
        if map.contains_key(id) {
            return Err(AppError::conflict(format!("任务 ID '{id}' 已有活动任务")));
        }
        map.insert(id.to_string(), Arc::clone(&flag));
        Ok(flag)
    }

    pub fn trigger_cancel(&self, id: &str) -> bool {
        let map = self.cancels.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(flag) = map.get(id) {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    pub fn unregister_cancel(&self, id: &str, flag: &Arc<AtomicBool>) {
        let mut map = self.cancels.lock().unwrap_or_else(|err| err.into_inner());
        if map
            .get(id)
            .is_some_and(|current| Arc::ptr_eq(current, flag))
        {
            map.remove(id);
        }
    }
}
