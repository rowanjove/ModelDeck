use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::future::join_all;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::SqlitePool;
use tokio::sync::Semaphore;

use crate::credentials::SecretStore;
use crate::domain::{GenerateImageInput, GenerateTextInput};
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchGenerateInput {
    pub modality: String,
    pub model_id: String,
    pub prompts: Vec<String>,
    pub system_prompt: Option<String>,
    pub seeds: Option<Vec<u64>>,
    pub parameters: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchItemResult {
    pub index: usize,
    pub prompt: String,
    pub seed: Option<u64>,
    pub output: Option<String>,
    pub latency_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchGenerateResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub items: Vec<BatchItemResult>,
}

pub async fn execute(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    data_dir: &Path,
    input: BatchGenerateInput,
    cancel_flag: Option<Arc<AtomicBool>>,
    scheduler: Option<Arc<crate::services::task_scheduler::TaskScheduler>>,
) -> Result<BatchGenerateResult, AppError> {
    if let Some(ref cf) = cancel_flag {
        if cf.load(Ordering::Relaxed) {
            return Err(AppError::new("cancelled", "批量任务已被用户取消"));
        }
    }

    let mut tasks = Vec::new();
    let seeds = input.seeds.unwrap_or_default();

    for p in &input.prompts {
        let p_trimmed = p.trim().to_string();
        if p_trimmed.is_empty() {
            continue;
        }
        if seeds.is_empty() {
            tasks.push((tasks.len(), p_trimmed, None));
        } else {
            for &s in &seeds {
                tasks.push((tasks.len(), p_trimmed.clone(), Some(s)));
            }
        }
    }

    if tasks.is_empty() {
        return Err(AppError::validation("没有有效的批量生成任务"));
    }
    if tasks.len() > 30 {
        return Err(AppError::validation("单次批量生成任务不能超过 30 条"));
    }

    let model = crate::db::get_model(pool, &input.model_id).await?;
    let has_seed = model.parameters.iter().any(|param| param.key == "seed");
    let is_comfyui = crate::db::is_comfyui_model(pool, &input.model_id).await;

    let semaphore = Arc::new(Semaphore::new(2));
    let mut futures = Vec::new();

    for (idx, p, s) in tasks {
        let sem = semaphore.clone();
        let modality = input.modality.clone();
        let mid = input.model_id.clone();
        let sys = input.system_prompt.clone();
        let mut params = input.parameters.clone();
        let cancel_sub = cancel_flag.clone();
        let sched = scheduler.clone();
        if let Some(seed_val) = s {
            if has_seed {
                params.insert("seed".to_string(), json!(seed_val));
            }
        }

        futures.push(async move {
            let permit = if let Some(cf) = &cancel_sub {
                let acquire = sem.acquire();
                tokio::pin!(acquire);
                loop {
                    if cf.load(Ordering::Relaxed) {
                        return BatchItemResult {
                            index: idx,
                            prompt: p,
                            seed: s,
                            output: None,
                            latency_ms: 0,
                            error: Some("任务已取消".to_string()),
                        };
                    }
                    tokio::select! {
                        permit = &mut acquire => break permit,
                        _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
                    }
                }
            } else {
                sem.acquire().await
            };
            let _permit = match permit {
                Ok(permit) => permit,
                Err(_) => {
                    return BatchItemResult {
                        index: idx,
                        prompt: p,
                        seed: s,
                        output: None,
                        latency_ms: 0,
                        error: Some("批量任务调度已关闭".to_string()),
                    };
                }
            };

            let _sched_permit = if let Some(scheduler) = &sched {
                let mod_enum = if modality == "image" {
                    crate::domain::Modality::Image
                } else {
                    crate::domain::Modality::Text
                };
                match scheduler
                    .acquire_cancellable(mod_enum, is_comfyui, cancel_sub.as_deref())
                    .await
                {
                    Ok(p) => Some(p),
                    Err(e) => {
                        return BatchItemResult {
                            index: idx,
                            prompt: p,
                            seed: s,
                            output: None,
                            latency_ms: 0,
                            error: Some(e.message),
                        };
                    }
                }
            } else {
                None
            };

            if cancel_sub
                .as_ref()
                .is_some_and(|cf| cf.load(Ordering::Relaxed))
            {
                return BatchItemResult {
                    index: idx,
                    prompt: p,
                    seed: s,
                    output: None,
                    latency_ms: 0,
                    error: Some("任务已取消".to_string()),
                };
            }

            if modality == "image" {
                let img_input = GenerateImageInput {
                    model_definition_id: mid,
                    prompt: p.clone(),
                    negative_prompt: None,
                    source_image: None,
                    reference_images: vec![],
                    mask_base64: None,
                    mask_path: None,
                    parameters: params,
                    extra: None,
                };
                match crate::services::image::execute_with_cancel(
                    pool, http, secrets, data_dir, img_input, None, cancel_sub,
                )
                .await
                {
                    Ok(res) => BatchItemResult {
                        index: idx,
                        prompt: p,
                        seed: s,
                        output: res.images.first().map(|img| img.path.clone()),
                        latency_ms: res.latency_ms,
                        error: None,
                    },
                    Err(e) => BatchItemResult {
                        index: idx,
                        prompt: p,
                        seed: s,
                        output: None,
                        latency_ms: 0,
                        error: Some(e.message),
                    },
                }
            } else {
                let txt_input = GenerateTextInput {
                    model_definition_id: mid,
                    prompt: p.clone(),
                    system_prompt: sys,
                    parameters: params,
                    extra: None,
                    images: vec![],
                };
                match crate::services::generate::execute_with_cancel(
                    pool, http, secrets, data_dir, txt_input, cancel_sub,
                )
                .await
                {
                    Ok(res) => BatchItemResult {
                        index: idx,
                        prompt: p,
                        seed: s,
                        output: Some(res.text),
                        latency_ms: res.latency_ms,
                        error: None,
                    },
                    Err(e) => BatchItemResult {
                        index: idx,
                        prompt: p,
                        seed: s,
                        output: None,
                        latency_ms: 0,
                        error: Some(e.message),
                    },
                }
            }
        });
    }

    let items: Vec<BatchItemResult> = join_all(futures).await;
    if let Some(ref cf) = cancel_flag {
        if cf.load(Ordering::Relaxed) {
            return Err(AppError::new("cancelled", "批量任务已被用户取消"));
        }
    }
    let total = items.len();
    let succeeded = items.iter().filter(|i| i.error.is_none()).count();
    let failed = total - succeeded;

    Ok(BatchGenerateResult {
        total,
        succeeded,
        failed,
        items,
    })
}
