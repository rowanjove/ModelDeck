use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::{Row, SqlitePool};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use crate::domain::{now_iso, MediaRef, ReferenceImage};
use crate::error::AppError;

const MODALITIES: [&str; 6] = ["text", "image", "video", "audio", "batch", "pipeline"];
const STATUSES: [&str; 9] = [
    "queued",
    "submitting",
    "running",
    "downloading",
    "completed",
    "failed",
    "cancelled",
    "interrupted",
    "unknown_submission",
];
const OUTPUT_LIMIT: u64 = 5_000_000;

pub async fn mark_task_cancelled(pool: &SqlitePool, task_id: &str) -> Result<bool, AppError> {
    let now = now_iso();
    let res = sqlx::query(
        "UPDATE generation_tasks 
         SET status = 'cancelled', error = '任务已取消', completed_at = ? 
         WHERE id = ? AND status NOT IN ('completed', 'cancelled', 'failed', 'interrupted', 'unknown_submission')",
    )
    .bind(now)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(AppError::db)?;
    Ok(res.rows_affected() > 0)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub id: String,
    pub modality: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_name: String,
    pub status: String,
    pub prompt_preview: String,
    pub error: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub items: Vec<HistoryItem>,
    pub total: u32,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub task_id: String,
    #[serde(rename = "type")]
    pub asset_type: String,
    pub path: Option<String>,
    pub url: Option<String>,
    pub mime_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u32>,
    pub metadata: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryDetail {
    pub id: String,
    pub modality: String,
    pub kind: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_name: String,
    pub model_api_id: String,
    pub status: String,
    pub prompt: String,
    pub system_prompt: Option<String>,
    pub parameters: Value,
    pub extra: Option<Value>,
    pub images: Vec<MediaRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_image: Option<MediaRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mask: Option<MediaRef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reference_images: Vec<ReferenceImage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reference_videos: Vec<MediaRef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reference_audios: Vec<MediaRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_frame: Option<MediaRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_frame: Option<MediaRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_transcript: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_audio: Option<MediaRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_audio: Option<MediaRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_task: Option<String>,
    pub conversation_id: Option<String>,
    pub raw_request: Value,
    pub raw_response: Value,
    pub output: String,
    pub error: Option<String>,
    pub assets: Vec<Asset>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryQuery {
    #[serde(default)]
    pub modality: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub date_from: Option<String>,
    #[serde(default)]
    pub date_to: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub offset: Option<u32>,
}

pub struct NewTextTask {
    pub kind: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_name: String,
    pub model_api_id: String,
    pub prompt: String,
    pub system_prompt: Option<String>,
    pub images: Vec<MediaRef>,
    pub extra: Option<Value>,
    pub conversation_id: Option<String>,
    pub parameters: Map<String, Value>,
    pub status: String,
    pub output_text: Option<String>,
    pub raw_request: Option<Value>,
    pub raw_response: Option<Value>,
    pub error: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskInput {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    images: Vec<MediaRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    negative_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_image: Option<MediaRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mask: Option<MediaRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    reference_images: Vec<ReferenceImage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    reference_videos: Vec<MediaRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    reference_audios: Vec<MediaRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    first_frame: Option<MediaRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_frame: Option<MediaRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    voice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reference_transcript: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reference_audio: Option<MediaRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_audio: Option<MediaRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    audio_task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extra: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    conversation_id: Option<String>,
    #[serde(default)]
    provider_name: String,
    #[serde(default)]
    model_name: String,
    #[serde(default)]
    model_api_id: String,
}

/// Return stable identities for a stored path. Windows may expose the same
/// file as a normal absolute path or with the `\\?\` extended prefix, and
/// path separators can differ between JSON and SQLite columns.
pub(crate) fn path_keys(path: &str) -> HashSet<String> {
    fn key(path: &Path) -> String {
        let mut value = path.to_string_lossy().replace('\\', "/");
        if let Some(rest) = value.strip_prefix("//?/UNC/") {
            value = format!("//{rest}");
        } else if let Some(rest) = value.strip_prefix("//?/") {
            value = rest.to_string();
        }
        if cfg!(windows) {
            value.make_ascii_lowercase();
        }
        value
    }

    let mut keys = HashSet::new();
    keys.insert(key(Path::new(path)));
    if let Ok(canonical) = std::fs::canonicalize(path) {
        keys.insert(key(&canonical));
    }
    keys
}

fn path_matches(left: &str, right: &HashSet<String>) -> bool {
    path_keys(left).iter().any(|key| right.contains(key))
}

fn json_contains_path(value: &str, target: &HashSet<String>) -> bool {
    let Ok(parsed) = serde_json::from_str::<Value>(value) else {
        return false;
    };
    fn visit(value: &Value, target: &HashSet<String>) -> bool {
        match value {
            Value::String(text) => path_matches(text, target),
            Value::Array(items) => items.iter().any(|item| visit(item, target)),
            Value::Object(map) => map.values().any(|item| visit(item, target)),
            _ => false,
        }
    }
    visit(&parsed, target)
}

fn task_input_from_json(raw: &str, modality: &str) -> Result<TaskInput, AppError> {
    let mut input = serde_json::from_str::<TaskInput>(raw)
        .map_err(|err| AppError::storage(format!("历史输入已损坏: {err}")))?;
    if input.kind.trim().is_empty() {
        input.kind = modality.to_string();
    }
    Ok(input)
}

async fn ensure_task_not_deleted(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let deleted: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM deleted_generation_tasks WHERE id = ? LIMIT 1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(AppError::db)?;
    if deleted.is_some() {
        return Err(AppError::conflict("任务已被删除，忽略迟到的生成结果"));
    }
    Ok(())
}

pub async fn record_text_task(
    pool: &SqlitePool,
    data_dir: &Path,
    task: NewTextTask,
) -> Result<String, AppError> {
    if !STATUSES.contains(&task.status.as_str()) {
        return Err(AppError::validation("状态无效"));
    }
    let id = Uuid::new_v4().to_string();
    let now = now_iso();
    let output_text = task.output_text.filter(|text| !text.is_empty());
    let output_path = match &output_text {
        Some(text) => Some(write_output(data_dir, &id, text).await?),
        None => None,
    };
    let input = TaskInput {
        kind: task.kind,
        prompt: task.prompt.clone(),
        system_prompt: task.system_prompt.filter(|text| !text.trim().is_empty()),
        images: task.images,
        negative_prompt: None,
        source_image: None,
        mask: None,
        reference_images: Vec::new(),
        reference_videos: Vec::new(),
        reference_audios: Vec::new(),
        first_frame: None,
        last_frame: None,
        voice: None,
        reference_transcript: None,
        reference_audio: None,
        source_audio: None,
        audio_task: None,
        extra: task.extra.filter(|value| !value.is_null()),
        conversation_id: task.conversation_id.filter(|text| !text.is_empty()),
        provider_name: task.provider_name,
        model_name: task.model_name,
        model_api_id: task.model_api_id,
    };
    let is_terminal = matches!(
        task.status.as_str(),
        "completed" | "failed" | "cancelled" | "interrupted"
    );
    let completed_at = if is_terminal {
        Some(now.as_str())
    } else {
        None
    };
    let mut attempts = 0;
    let (task_res, output_path) = loop {
        attempts += 1;
        let mut conn = match pool.acquire().await {
            Ok(c) => c,
            Err(e) => {
                if attempts < 5 {
                    tokio::time::sleep(std::time::Duration::from_millis(50 * attempts)).await;
                    continue;
                }
                return Err(AppError::db(e));
            }
        };

        if let Err(e) = sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await {
            if attempts < 5 && e.to_string().contains("locked") {
                tokio::time::sleep(std::time::Duration::from_millis(50 * attempts)).await;
                continue;
            }
            return Err(AppError::db(e));
        }

        let res: Result<String, AppError> = async {
            sqlx::query(
                "INSERT INTO generation_tasks
                 (id, modality, provider_id, model_id, status, input_json, parameters_json, raw_request_json, raw_response_json, error, created_at, completed_at)
                 VALUES (?, 'text', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(id) DO UPDATE SET
                   status = CASE 
                     WHEN generation_tasks.status IN ('cancelled', 'interrupted') THEN generation_tasks.status 
                     ELSE excluded.status 
                   END,
                   raw_request_json = COALESCE(excluded.raw_request_json, generation_tasks.raw_request_json),
                   raw_response_json = COALESCE(excluded.raw_response_json, generation_tasks.raw_response_json),
                   error = CASE 
                     WHEN generation_tasks.status IN ('cancelled', 'interrupted') THEN generation_tasks.error 
                     ELSE excluded.error 
                   END,
                   completed_at = CASE 
                     WHEN generation_tasks.status IN ('cancelled', 'interrupted') THEN generation_tasks.completed_at 
                     ELSE excluded.completed_at 
                   END",
            )
            .bind(&id)
            .bind(&task.provider_id)
            .bind(&task.model_id)
            .bind(&task.status)
            .bind(json_text(&input)?)
            .bind(json_text(&task.parameters)?)
            .bind(task.raw_request.as_ref().map(json_text).transpose()?)
            .bind(task.raw_response.as_ref().map(json_text).transpose()?)
            .bind(task.error.as_ref().map(|text| truncate_chars(text, 2000)))
            .bind(&now)
            .bind(completed_at)
            .execute(&mut *conn)
            .await
            .map_err(AppError::db)?;

            for image in &input.images {
                if image.path.trim().is_empty() {
                    continue;
                }
                insert_asset(
                    &mut conn,
                    &AssetInsert {
                        task_id: &id,
                        asset_type: "image",
                        path: Some(&image.path),
                        mime_type: None,
                        width: None,
                        height: None,
                        metadata: &json!({ "role": "input" }),
                        created_at: &now,
                    },
                )
                .await?;
            }

            if let (Some(path), Some(text)) = (&output_path, &output_text) {
                let metadata = json!({ "role": "output", "bytes": text.len() });
                let path_text = path.to_string_lossy().to_string();
                insert_asset(
                    &mut conn,
                    &AssetInsert {
                        task_id: &id,
                        asset_type: "file",
                        path: Some(&path_text),
                        mime_type: Some("text/plain"),
                        width: None,
                        height: None,
                        metadata: &metadata,
                        created_at: &now,
                    },
                )
                .await?;
            }

            sqlx::query("COMMIT")
                .execute(&mut *conn)
                .await
                .map_err(AppError::db)?;

            let _ = crate::db::sync_fts_entry(pool, &id, "task", "text", &task.prompt).await;
            Ok(id.clone())
        }
        .await;

        match res {
            Ok(ret_id) => break (Ok(ret_id), output_path),
            Err(e) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                if attempts < 5 && e.message.contains("database is locked") {
                    tokio::time::sleep(std::time::Duration::from_millis(50 * attempts)).await;
                    continue;
                }
                break (Err(e), output_path);
            }
        }
    };

    if task_res.is_err() {
        if let Some(path) = &output_path {
            let _ = tokio::fs::remove_file(path).await;
        }
    }
    task_res
}

pub struct StoredOutput {
    pub path: String,
    pub mime_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

pub struct NewImageTask {
    pub id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_name: String,
    pub model_api_id: String,
    pub prompt: String,
    pub negative_prompt: Option<String>,
    pub source_image: Option<MediaRef>,
    pub mask: Option<MediaRef>,
    pub reference_images: Vec<ReferenceImage>,
    pub extra: Option<Value>,
    pub parameters: Map<String, Value>,
    pub status: String,
    pub outputs: Vec<StoredOutput>,
    pub raw_request: Option<Value>,
    pub raw_response: Option<Value>,
    pub error: Option<String>,
}

pub async fn record_image_task(pool: &SqlitePool, task: NewImageTask) -> Result<String, AppError> {
    if !STATUSES.contains(&task.status.as_str()) {
        return Err(AppError::validation("状态无效"));
    }
    ensure_task_not_deleted(pool, &task.id).await?;
    let now = now_iso();
    let input = TaskInput {
        kind: "image".to_string(),
        prompt: task.prompt,
        system_prompt: None,
        images: Vec::new(),
        negative_prompt: task.negative_prompt.filter(|text| !text.trim().is_empty()),
        source_image: task
            .source_image
            .filter(|item| !item.path.trim().is_empty()),
        mask: task.mask.filter(|item| !item.path.trim().is_empty()),
        reference_images: task
            .reference_images
            .into_iter()
            .filter(|item| !item.path.trim().is_empty())
            .collect(),
        reference_videos: Vec::new(),
        reference_audios: Vec::new(),
        first_frame: None,
        last_frame: None,
        voice: None,
        reference_transcript: None,
        reference_audio: None,
        source_audio: None,
        audio_task: None,
        extra: task.extra.filter(|value| !value.is_null()),
        conversation_id: None,
        provider_name: task.provider_name,
        model_name: task.model_name,
        model_api_id: task.model_api_id,
    };
    let is_terminal = matches!(
        task.status.as_str(),
        "completed" | "failed" | "cancelled" | "interrupted"
    );
    let completed_at = if is_terminal {
        Some(now.as_str())
    } else {
        None
    };
    let mut tx = pool.begin().await.map_err(AppError::db)?;
    sqlx::query(
        "INSERT INTO generation_tasks
         (id, modality, provider_id, model_id, status, input_json, parameters_json, raw_request_json, raw_response_json, error, created_at, completed_at)
         VALUES (?, 'image', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
           status = CASE 
             WHEN generation_tasks.status IN ('cancelled', 'interrupted') THEN generation_tasks.status 
             ELSE excluded.status 
           END,
           raw_request_json = COALESCE(excluded.raw_request_json, generation_tasks.raw_request_json),
           raw_response_json = COALESCE(excluded.raw_response_json, generation_tasks.raw_response_json),
           error = CASE 
             WHEN generation_tasks.status IN ('cancelled', 'interrupted') THEN generation_tasks.error 
             ELSE excluded.error 
           END,
           completed_at = CASE 
             WHEN generation_tasks.status IN ('cancelled', 'interrupted') THEN generation_tasks.completed_at 
             ELSE excluded.completed_at 
           END",
    )
    .bind(&task.id)
    .bind(&task.provider_id)
    .bind(&task.model_id)
    .bind(&task.status)
    .bind(json_text(&input)?)
    .bind(json_text(&task.parameters)?)
    .bind(task.raw_request.as_ref().map(json_text).transpose()?)
    .bind(task.raw_response.as_ref().map(json_text).transpose()?)
    .bind(task.error.as_ref().map(|text| truncate_chars(text, 2000)))
    .bind(&now)
    .bind(completed_at)
    .execute(&mut *tx)
    .await
    .map_err(AppError::db)?;
    if let Some(source) = &input.source_image {
        insert_role(&mut tx, &task.id, &source.path, "source", None, None, &now).await?;
    }
    for image in &input.reference_images {
        insert_role(
            &mut tx,
            &task.id,
            &image.path,
            "reference",
            None,
            None,
            &now,
        )
        .await?;
    }
    if let Some(mask) = &input.mask {
        insert_role(&mut tx, &task.id, &mask.path, "mask", None, None, &now).await?;
    }
    for output in &task.outputs {
        insert_role(
            &mut tx,
            &task.id,
            &output.path,
            "output",
            Some(&output.mime_type),
            Some((output.width, output.height)),
            &now,
        )
        .await?;
    }
    tx.commit().await.map_err(AppError::db)?;
    Ok(task.id)
}

pub struct NewVideoTask {
    pub id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_name: String,
    pub model_api_id: String,
    pub prompt: String,
    pub negative_prompt: Option<String>,
    pub first_frame: Option<MediaRef>,
    pub last_frame: Option<MediaRef>,
    pub reference_images: Vec<ReferenceImage>,
    pub reference_videos: Vec<MediaRef>,
    pub reference_audios: Vec<MediaRef>,
    pub extra: Option<Value>,
    pub parameters: Map<String, Value>,
    pub status: String,
    pub outputs: Vec<StoredOutput>,
    pub raw_request: Option<Value>,
    pub raw_response: Option<Value>,
    pub error: Option<String>,
}

pub async fn record_video_task(pool: &SqlitePool, task: NewVideoTask) -> Result<String, AppError> {
    if !STATUSES.contains(&task.status.as_str()) {
        return Err(AppError::validation("状态无效"));
    }
    ensure_task_not_deleted(pool, &task.id).await?;
    let now = now_iso();
    let input = TaskInput {
        kind: "video".to_string(),
        prompt: task.prompt,
        system_prompt: None,
        images: Vec::new(),
        negative_prompt: task.negative_prompt.filter(|text| !text.trim().is_empty()),
        source_image: None,
        mask: None,
        reference_images: task
            .reference_images
            .into_iter()
            .filter(|item| !item.path.trim().is_empty())
            .collect(),
        reference_videos: task
            .reference_videos
            .into_iter()
            .filter(|item| !item.path.trim().is_empty())
            .collect(),
        reference_audios: task
            .reference_audios
            .into_iter()
            .filter(|item| !item.path.trim().is_empty())
            .collect(),
        first_frame: task.first_frame.filter(|item| !item.path.trim().is_empty()),
        last_frame: task.last_frame.filter(|item| !item.path.trim().is_empty()),
        voice: None,
        reference_transcript: None,
        reference_audio: None,
        source_audio: None,
        audio_task: None,
        extra: task.extra.filter(|value| !value.is_null()),
        conversation_id: None,
        provider_name: task.provider_name,
        model_name: task.model_name,
        model_api_id: task.model_api_id,
    };
    let is_terminal = matches!(
        task.status.as_str(),
        "completed" | "failed" | "cancelled" | "interrupted"
    );
    let completed_at = if is_terminal {
        Some(now.as_str())
    } else {
        None
    };
    let mut tx = pool.begin().await.map_err(AppError::db)?;
    sqlx::query(
        "INSERT INTO generation_tasks
         (id, modality, provider_id, model_id, status, input_json, parameters_json, raw_request_json, raw_response_json, error, created_at, completed_at)
         VALUES (?, 'video', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
           status = CASE 
             WHEN generation_tasks.status IN ('cancelled', 'interrupted') THEN generation_tasks.status 
             ELSE excluded.status 
           END,
           raw_request_json = COALESCE(excluded.raw_request_json, generation_tasks.raw_request_json),
           raw_response_json = COALESCE(excluded.raw_response_json, generation_tasks.raw_response_json),
           error = CASE 
             WHEN generation_tasks.status IN ('cancelled', 'interrupted') THEN generation_tasks.error 
             ELSE excluded.error 
           END,
           completed_at = CASE 
             WHEN generation_tasks.status IN ('cancelled', 'interrupted') THEN generation_tasks.completed_at 
             ELSE excluded.completed_at 
           END",
    )
    .bind(&task.id)
    .bind(&task.provider_id)
    .bind(&task.model_id)
    .bind(&task.status)
    .bind(json_text(&input)?)
    .bind(json_text(&task.parameters)?)
    .bind(task.raw_request.as_ref().map(json_text).transpose()?)
    .bind(task.raw_response.as_ref().map(json_text).transpose()?)
    .bind(task.error.as_ref().map(|text| truncate_chars(text, 2000)))
    .bind(&now)
    .bind(completed_at)
    .execute(&mut *tx)
    .await
    .map_err(AppError::db)?;
    if let Some(frame) = &input.first_frame {
        insert_role(
            &mut tx,
            &task.id,
            &frame.path,
            "firstFrame",
            None,
            None,
            &now,
        )
        .await?;
    }
    if let Some(frame) = &input.last_frame {
        insert_role(
            &mut tx,
            &task.id,
            &frame.path,
            "lastFrame",
            None,
            None,
            &now,
        )
        .await?;
    }
    for image in &input.reference_images {
        insert_role(
            &mut tx,
            &task.id,
            &image.path,
            "reference",
            None,
            None,
            &now,
        )
        .await?;
    }
    for item in &input.reference_videos {
        let metadata = json!({ "role": "reference" });
        insert_asset(
            &mut tx,
            &AssetInsert {
                task_id: &task.id,
                asset_type: "video",
                path: Some(&item.path),
                mime_type: None,
                width: None,
                height: None,
                metadata: &metadata,
                created_at: &now,
            },
        )
        .await?;
    }
    for item in &input.reference_audios {
        let metadata = json!({ "role": "reference" });
        insert_asset(
            &mut tx,
            &AssetInsert {
                task_id: &task.id,
                asset_type: "audio",
                path: Some(&item.path),
                mime_type: None,
                width: None,
                height: None,
                metadata: &metadata,
                created_at: &now,
            },
        )
        .await?;
    }
    for output in &task.outputs {
        let metadata = json!({ "role": "output" });
        insert_asset(
            &mut tx,
            &AssetInsert {
                task_id: &task.id,
                asset_type: "video",
                path: Some(&output.path),
                mime_type: Some(&output.mime_type),
                width: output.width,
                height: output.height,
                metadata: &metadata,
                created_at: &now,
            },
        )
        .await?;
    }
    tx.commit().await.map_err(AppError::db)?;
    Ok(task.id)
}

pub struct NewAudioTask {
    pub id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_name: String,
    pub model_api_id: String,
    pub text: String,
    pub voice: Option<String>,
    pub reference_audio: Option<MediaRef>,
    pub reference_transcript: Option<String>,
    pub source_audio: Option<MediaRef>,
    pub audio_task: String,
    pub transcript_path: Option<String>,
    pub extra: Option<Value>,
    pub parameters: Map<String, Value>,
    pub status: String,
    pub outputs: Vec<StoredOutput>,
    pub raw_request: Option<Value>,
    pub raw_response: Option<Value>,
    pub error: Option<String>,
}

pub async fn record_audio_task(pool: &SqlitePool, task: NewAudioTask) -> Result<String, AppError> {
    if !STATUSES.contains(&task.status.as_str()) {
        return Err(AppError::validation("状态无效"));
    }
    ensure_task_not_deleted(pool, &task.id).await?;
    let now = now_iso();
    let input = TaskInput {
        kind: "audio".to_string(),
        prompt: task.text,
        system_prompt: None,
        images: Vec::new(),
        negative_prompt: None,
        source_image: None,
        mask: None,
        reference_images: Vec::new(),
        reference_videos: Vec::new(),
        reference_audios: Vec::new(),
        first_frame: None,
        last_frame: None,
        voice: task.voice.filter(|text| !text.trim().is_empty()),
        reference_transcript: task
            .reference_transcript
            .filter(|text| !text.trim().is_empty()),
        reference_audio: task
            .reference_audio
            .filter(|item| !item.path.trim().is_empty()),
        source_audio: task
            .source_audio
            .filter(|item| !item.path.trim().is_empty()),
        audio_task: Some(task.audio_task).filter(|text| !text.trim().is_empty()),
        extra: task.extra.filter(|value| !value.is_null()),
        conversation_id: None,
        provider_name: task.provider_name,
        model_name: task.model_name,
        model_api_id: task.model_api_id,
    };
    let is_terminal = matches!(
        task.status.as_str(),
        "completed" | "failed" | "cancelled" | "interrupted"
    );
    let completed_at = if is_terminal {
        Some(now.as_str())
    } else {
        None
    };
    let mut tx = pool.begin().await.map_err(AppError::db)?;
    sqlx::query(
        "INSERT INTO generation_tasks
         (id, modality, provider_id, model_id, status, input_json, parameters_json, raw_request_json, raw_response_json, error, created_at, completed_at)
         VALUES (?, 'audio', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
           status = CASE 
             WHEN generation_tasks.status IN ('cancelled', 'interrupted') THEN generation_tasks.status 
             ELSE excluded.status 
           END,
           raw_request_json = COALESCE(excluded.raw_request_json, generation_tasks.raw_request_json),
           raw_response_json = COALESCE(excluded.raw_response_json, generation_tasks.raw_response_json),
           error = CASE 
             WHEN generation_tasks.status IN ('cancelled', 'interrupted') THEN generation_tasks.error 
             ELSE excluded.error 
           END,
           completed_at = CASE 
             WHEN generation_tasks.status IN ('cancelled', 'interrupted') THEN generation_tasks.completed_at 
             ELSE excluded.completed_at 
           END",
    )
    .bind(&task.id)
    .bind(&task.provider_id)
    .bind(&task.model_id)
    .bind(&task.status)
    .bind(json_text(&input)?)
    .bind(json_text(&task.parameters)?)
    .bind(task.raw_request.as_ref().map(json_text).transpose()?)
    .bind(task.raw_response.as_ref().map(json_text).transpose()?)
    .bind(task.error.as_ref().map(|text| truncate_chars(text, 2000)))
    .bind(&now)
    .bind(completed_at)
    .execute(&mut *tx)
    .await
    .map_err(AppError::db)?;
    if let Some(audio) = &input.reference_audio {
        let metadata = json!({ "role": "reference" });
        insert_asset(
            &mut tx,
            &AssetInsert {
                task_id: &task.id,
                asset_type: "audio",
                path: Some(&audio.path),
                mime_type: None,
                width: None,
                height: None,
                metadata: &metadata,
                created_at: &now,
            },
        )
        .await?;
    }
    if let Some(audio) = &input.source_audio {
        let metadata = json!({ "role": "source" });
        insert_asset(
            &mut tx,
            &AssetInsert {
                task_id: &task.id,
                asset_type: "audio",
                path: Some(&audio.path),
                mime_type: None,
                width: None,
                height: None,
                metadata: &metadata,
                created_at: &now,
            },
        )
        .await?;
    }
    if let Some(path) = &task.transcript_path {
        let metadata = json!({ "role": "output" });
        insert_asset(
            &mut tx,
            &AssetInsert {
                task_id: &task.id,
                asset_type: "file",
                path: Some(path),
                mime_type: Some("text/plain"),
                width: None,
                height: None,
                metadata: &metadata,
                created_at: &now,
            },
        )
        .await?;
    }
    for output in &task.outputs {
        let metadata = json!({ "role": "output" });
        insert_asset(
            &mut tx,
            &AssetInsert {
                task_id: &task.id,
                asset_type: "audio",
                path: Some(&output.path),
                mime_type: Some(&output.mime_type),
                width: None,
                height: None,
                metadata: &metadata,
                created_at: &now,
            },
        )
        .await?;
    }
    tx.commit().await.map_err(AppError::db)?;
    Ok(task.id)
}

async fn insert_role(
    conn: &mut sqlx::SqliteConnection,
    task_id: &str,
    path: &str,
    role: &str,
    mime_type: Option<&str>,
    size: Option<(Option<u32>, Option<u32>)>,
    created_at: &str,
) -> Result<(), AppError> {
    let (width, height) = size.unwrap_or((None, None));
    let metadata = json!({ "role": role });
    insert_asset(
        conn,
        &AssetInsert {
            task_id,
            asset_type: "image",
            path: Some(path),
            mime_type,
            width,
            height,
            metadata: &metadata,
            created_at,
        },
    )
    .await
}

pub async fn list(pool: &SqlitePool, query: HistoryQuery) -> Result<HistoryPage, AppError> {
    let limit = query.limit.unwrap_or(30).clamp(1, 100);
    let offset = query.offset.unwrap_or(0);
    let (where_sql, binds) = filters(&query)?;
    let count_sql = format!(
        "SELECT COUNT(*) FROM generation_tasks t
         LEFT JOIN providers p ON p.id = t.provider_id
         LEFT JOIN models m ON m.id = t.model_id
         {where_sql}"
    );
    let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
    for value in &binds {
        count_query = count_query.bind(value);
    }
    let total = count_query.fetch_one(pool).await.map_err(AppError::db)?;
    let list_sql = format!(
        "SELECT t.id, t.modality, t.provider_id, t.model_id, t.status, t.input_json, t.error, t.created_at, t.completed_at,
                p.name AS provider_name, m.name AS model_name
         FROM generation_tasks t
         LEFT JOIN providers p ON p.id = t.provider_id
         LEFT JOIN models m ON m.id = t.model_id
         {where_sql}
         ORDER BY t.created_at DESC, t.id DESC
         LIMIT ? OFFSET ?"
    );
    let mut list_query = sqlx::query(&list_sql);
    for value in &binds {
        list_query = list_query.bind(value);
    }
    let rows = list_query
        .bind(i64::from(limit))
        .bind(i64::from(offset))
        .fetch_all(pool)
        .await
        .map_err(AppError::db)?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        let input = task_input_from_json(
            row.get::<String, _>("input_json").as_str(),
            row.get::<String, _>("modality").as_str(),
        )
        .unwrap_or_default();
        let provider_id: String = row.try_get("provider_id").map_err(AppError::db)?;
        let model_id: String = row.try_get("model_id").map_err(AppError::db)?;
        let provider_name = joined_name(row, "provider_name", &input.provider_name, &provider_id)?;
        let model_name = joined_name(row, "model_name", &input.model_name, &model_id)?;
        items.push(HistoryItem {
            id: row.try_get("id").map_err(AppError::db)?,
            modality: row.try_get("modality").map_err(AppError::db)?,
            provider_id,
            provider_name,
            model_id,
            model_name,
            status: row.try_get("status").map_err(AppError::db)?,
            prompt_preview: preview(&input.prompt),
            error: row.try_get("error").map_err(AppError::db)?,
            created_at: row.try_get("created_at").map_err(AppError::db)?,
            completed_at: row.try_get("completed_at").map_err(AppError::db)?,
        });
    }
    Ok(HistoryPage {
        items,
        total: u32::try_from(total).unwrap_or(u32::MAX),
        limit,
        offset,
    })
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<HistoryDetail, AppError> {
    let row = sqlx::query(
        "SELECT t.id, t.modality, t.provider_id, t.model_id, t.status, t.input_json, t.parameters_json,
                t.raw_request_json, t.raw_response_json, t.error, t.created_at, t.completed_at,
                p.name AS provider_name, m.name AS model_name
         FROM generation_tasks t
         LEFT JOIN providers p ON p.id = t.provider_id
         LEFT JOIN models m ON m.id = t.model_id
         WHERE t.id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::db)?
    .ok_or_else(|| AppError::not_found("找不到这条历史"))?;
    let input = task_input_from_json(
        row.get::<String, _>("input_json").as_str(),
        row.get::<String, _>("modality").as_str(),
    )?;
    let parameters = serde_json::from_str(row.get::<String, _>("parameters_json").as_str())
        .unwrap_or_else(|_| json!({}));
    let provider_id: String = row.try_get("provider_id").map_err(AppError::db)?;
    let model_id: String = row.try_get("model_id").map_err(AppError::db)?;
    let provider_name = joined_name(&row, "provider_name", &input.provider_name, &provider_id)?;
    let model_name = joined_name(&row, "model_name", &input.model_name, &model_id)?;
    let assets = assets_for(pool, id).await?;
    let output = read_output(&assets).await;
    Ok(HistoryDetail {
        id: row.try_get("id").map_err(AppError::db)?,
        modality: row.try_get("modality").map_err(AppError::db)?,
        kind: input.kind,
        provider_id,
        provider_name,
        model_id,
        model_name,
        model_api_id: input.model_api_id,
        status: row.try_get("status").map_err(AppError::db)?,
        prompt: input.prompt,
        system_prompt: input.system_prompt,
        parameters,
        extra: input.extra,
        images: input.images,
        negative_prompt: input.negative_prompt,
        source_image: input.source_image,
        mask: input.mask,
        reference_images: input.reference_images,
        reference_videos: input.reference_videos,
        reference_audios: input.reference_audios,
        first_frame: input.first_frame,
        last_frame: input.last_frame,
        voice: input.voice,
        reference_transcript: input.reference_transcript,
        reference_audio: input.reference_audio,
        source_audio: input.source_audio,
        audio_task: input.audio_task,
        conversation_id: input.conversation_id,
        raw_request: parse_json_value(row.try_get("raw_request_json").map_err(AppError::db)?),
        raw_response: parse_json_value(row.try_get("raw_response_json").map_err(AppError::db)?),
        output,
        error: row.try_get("error").map_err(AppError::db)?,
        assets,
        created_at: row.try_get("created_at").map_err(AppError::db)?,
        completed_at: row.try_get("completed_at").map_err(AppError::db)?,
    })
}

fn filters(query: &HistoryQuery) -> Result<(String, Vec<String>), AppError> {
    let mut clauses = Vec::new();
    let mut binds = Vec::new();
    if let Some(value) = present(&query.modality) {
        require_choice(&value, &MODALITIES, "模态")?;
        clauses.push("t.modality = ?");
        binds.push(value);
    }
    if let Some(value) = present(&query.provider_id) {
        require_short(&value)?;
        clauses.push("t.provider_id = ?");
        binds.push(value);
    }
    if let Some(value) = present(&query.model_id) {
        require_short(&value)?;
        clauses.push("t.model_id = ?");
        binds.push(value);
    }
    if let Some(value) = present(&query.status) {
        require_choice(&value, &STATUSES, "状态")?;
        clauses.push("t.status = ?");
        binds.push(value);
    }
    if let Some(value) = present(&query.date_from) {
        require_date(&value)?;
        clauses.push("t.created_at >= ?");
        binds.push(value);
    }
    if let Some(value) = present(&query.date_to) {
        require_date(&value)?;
        clauses.push("t.created_at <= ?");
        binds.push(format!("{value}T23:59:59.999Z"));
    }
    if let Some(value) = present(&query.query) {
        let fts_q = crate::services::search::format_fts5_query(&value);
        if !fts_q.is_empty() {
            clauses.push("(t.id IN (SELECT entity_id FROM fts_entries WHERE entity_type = 'task' AND (search_text MATCH ? OR body MATCH ?)) OR t.input_json LIKE ? OR p.name LIKE ? OR m.name LIKE ? OR t.error LIKE ?)");
            binds.push(fts_q.clone());
            binds.push(fts_q);
            let pattern = format!("%{value}%");
            binds.push(pattern.clone());
            binds.push(pattern.clone());
            binds.push(pattern.clone());
            binds.push(pattern);
        } else {
            clauses
                .push("(t.input_json LIKE ? OR p.name LIKE ? OR m.name LIKE ? OR t.error LIKE ?)");
            let pattern = format!("%{value}%");
            binds.push(pattern.clone());
            binds.push(pattern.clone());
            binds.push(pattern.clone());
            binds.push(pattern);
        }
    }
    let sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    Ok((sql, binds))
}

fn present(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn require_choice(value: &str, allowed: &[&str], label: &str) -> Result<(), AppError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(AppError::validation(format!("{label}无效")))
    }
}

fn require_short(value: &str) -> Result<(), AppError> {
    if value.chars().count() > 80 {
        Err(AppError::validation("筛选条件过长"))
    } else {
        Ok(())
    }
}

fn require_date(value: &str) -> Result<(), AppError> {
    let invalid = || AppError::validation("日期格式应为 YYYY-MM-DD");
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(invalid());
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return Err(invalid());
    }
    let month: u32 = value[5..7].parse().map_err(|_| invalid())?;
    let day: u32 = value[8..10].parse().map_err(|_| invalid())?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(invalid());
    }
    Ok(())
}

fn preview(prompt: &str) -> String {
    prompt
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(80)
        .collect()
}

fn truncate_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn joined_name(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
    snapshot: &str,
    fallback: &str,
) -> Result<String, AppError> {
    let joined: Option<String> = row.try_get(column).map_err(AppError::db)?;
    Ok(joined
        .filter(|name| !name.is_empty())
        .or_else(|| {
            if snapshot.is_empty() {
                None
            } else {
                Some(snapshot.to_string())
            }
        })
        .unwrap_or_else(|| fallback.to_string()))
}

fn json_text<T: Serialize>(value: &T) -> Result<String, AppError> {
    serde_json::to_string(value).map_err(|err| AppError::storage(format!("无法保存 JSON: {err}")))
}

fn parse_json_value(value: Option<String>) -> Value {
    value
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

async fn write_output(data_dir: &Path, id: &str, text: &str) -> Result<PathBuf, AppError> {
    let dir = data_dir.join("files");
    let path = dir.join(format!("{id}.txt"));
    crate::storage_util::atomic_write_async(&path, text.as_bytes()).await?;
    Ok(path)
}

struct AssetInsert<'a> {
    task_id: &'a str,
    asset_type: &'a str,
    path: Option<&'a str>,
    mime_type: Option<&'a str>,
    width: Option<u32>,
    height: Option<u32>,
    metadata: &'a Value,
    created_at: &'a str,
}

async fn insert_asset(
    conn: &mut sqlx::SqliteConnection,
    asset: &AssetInsert<'_>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO assets
         (id, task_id, type, path, url, mime_type, width, height, duration_ms, metadata_json, created_at)
         VALUES (?, ?, ?, ?, NULL, ?, ?, ?, NULL, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(asset.task_id)
    .bind(asset.asset_type)
    .bind(asset.path)
    .bind(asset.mime_type)
    .bind(asset.width.map(i64::from))
    .bind(asset.height.map(i64::from))
    .bind(json_text(asset.metadata)?)
    .bind(asset.created_at)
    .execute(&mut *conn)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

async fn assets_for(pool: &SqlitePool, task_id: &str) -> Result<Vec<Asset>, AppError> {
    let rows = sqlx::query(
        "SELECT id, task_id, type, path, url, mime_type, width, height, duration_ms, metadata_json, created_at
         FROM assets WHERE task_id = ? ORDER BY created_at, id",
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::db)?;
    rows.iter().map(asset_from_row).collect()
}

fn asset_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Asset, AppError> {
    let metadata = row
        .try_get::<String, _>("metadata_json")
        .map_err(AppError::db)?;
    Ok(Asset {
        id: row.try_get("id").map_err(AppError::db)?,
        task_id: row.try_get("task_id").map_err(AppError::db)?,
        asset_type: row.try_get("type").map_err(AppError::db)?,
        path: row.try_get("path").map_err(AppError::db)?,
        url: row.try_get("url").map_err(AppError::db)?,
        mime_type: row.try_get("mime_type").map_err(AppError::db)?,
        width: opt_u32(row, "width")?,
        height: opt_u32(row, "height")?,
        duration_ms: opt_u32(row, "duration_ms")?,
        metadata: serde_json::from_str(&metadata).unwrap_or_else(|_| json!({})),
        created_at: row.try_get("created_at").map_err(AppError::db)?,
    })
}

fn opt_u32(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Option<u32>, AppError> {
    let value: Option<i64> = row.try_get(column).map_err(AppError::db)?;
    Ok(value.and_then(|number| u32::try_from(number).ok()))
}

async fn read_output(assets: &[Asset]) -> String {
    let Some(path) = assets.iter().find_map(|asset| {
        let role = asset.metadata.get("role").and_then(Value::as_str);
        if asset.asset_type == "file" && role == Some("output") {
            asset.path.clone()
        } else {
            None
        }
    }) else {
        return String::new();
    };
    let Ok(file) = tokio::fs::File::open(&path).await else {
        return String::new();
    };
    let mut limited = file.take(OUTPUT_LIMIT);
    let mut bytes = Vec::new();
    if limited.read_to_end(&mut bytes).await.is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

pub async fn delete_task(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    delete_tasks_batch(pool, &[id.to_string()]).await
}

pub async fn delete_tasks_batch(pool: &SqlitePool, ids: &[String]) -> Result<(), AppError> {
    if ids.is_empty() {
        return Ok(());
    }

    let mut files_to_delete = Vec::new();
    let mut tx = pool.begin().await.map_err(AppError::db)?;
    for id in ids {
        // This write obtains SQLite's writer lock before paths are collected.
        // A concurrent generation write must either commit first (so its
        // output is included below) or run after the tombstone and be rejected
        // by the migration trigger.
        sqlx::query(
            "INSERT INTO deleted_generation_tasks (id, deleted_at) VALUES (?, ?) ON CONFLICT(id) DO UPDATE SET deleted_at = excluded.deleted_at",
        )
        .bind(id)
        .bind(now_iso())
        .execute(&mut *tx)
        .await
        .map_err(AppError::db)?;

        let rows = sqlx::query(
            "SELECT id, task_id, type, path, url, mime_type, width, height, duration_ms, metadata_json, created_at
             FROM assets WHERE task_id = ? ORDER BY created_at, id",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await
        .map_err(AppError::db)?;
        for row in &rows {
            let asset = asset_from_row(row)?;
            if asset.metadata.get("role").and_then(Value::as_str) == Some("output") {
                if let Some(path) = asset.path {
                    files_to_delete.push(path);
                }
            }
        }

        sqlx::query("DELETE FROM assets WHERE task_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::db)?;

        sqlx::query("DELETE FROM generation_tasks WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::db)?;
    }
    tx.commit().await.map_err(AppError::db)?;

    // Delete captured files only after commit and only when no other task or
    // voice still refers to them.
    for path in files_to_delete {
        if !is_file_referenced(pool, &path).await? {
            let _ = tokio::fs::remove_file(&path).await;
        }
    }

    Ok(())
}

pub async fn is_file_referenced(pool: &SqlitePool, path: &str) -> Result<bool, AppError> {
    let target = path_keys(path);

    // Compare parsed path values and canonical identities instead of matching
    // one serialized spelling. This handles Windows extended paths and JSON
    // escaping without treating a similarly named file as a reference.
    let asset_paths: Vec<String> =
        sqlx::query_scalar("SELECT path FROM assets WHERE path IS NOT NULL")
            .fetch_all(pool)
            .await
            .map_err(AppError::db)?;
    if asset_paths
        .iter()
        .any(|stored| path_matches(stored, &target))
    {
        return Ok(true);
    }

    let voice_rows: Vec<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT file_path, reference_files_json FROM voices")
            .fetch_all(pool)
            .await
            .map_err(AppError::db)?;
    for (file_path, references) in voice_rows {
        if file_path
            .as_deref()
            .is_some_and(|stored| path_matches(stored, &target))
        {
            return Ok(true);
        }
        if references
            .as_deref()
            .is_some_and(|raw| json_contains_path(raw, &target))
        {
            return Ok(true);
        }
    }

    let message_json: Vec<String> = sqlx::query_scalar("SELECT content_json FROM messages")
        .fetch_all(pool)
        .await
        .map_err(AppError::db)?;
    if message_json
        .iter()
        .any(|raw| json_contains_path(raw, &target))
    {
        return Ok(true);
    }

    let task_json: Vec<(String, String)> =
        sqlx::query_as("SELECT input_json, parameters_json FROM generation_tasks")
            .fetch_all(pool)
            .await
            .map_err(AppError::db)?;
    if task_json.iter().any(|(input, parameters)| {
        json_contains_path(input, &target) || json_contains_path(parameters, &target)
    }) {
        return Ok(true);
    }

    Ok(false)
}

/// Remove generated files left behind when their history transaction fails,
/// while preserving any path that another persisted object still references.
pub async fn remove_unreferenced_files(
    pool: &SqlitePool,
    paths: &[String],
) -> Result<(), AppError> {
    for path in paths {
        if is_file_referenced(pool, path).await? {
            continue;
        }
        match tokio::fs::remove_file(path).await {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(AppError::storage(format!(
                    "无法清理未登记的生成文件: {err}"
                )));
            }
        }
    }
    Ok(())
}

#[allow(dead_code)]
pub async fn delete_all_tasks(pool: &SqlitePool, modality: Option<String>) -> Result<(), AppError> {
    let ids: Vec<String> = match modality {
        Some(m) if !m.is_empty() => {
            sqlx::query_scalar("SELECT id FROM generation_tasks WHERE modality = ?")
                .bind(m)
                .fetch_all(pool)
                .await
                .map_err(AppError::db)?
        }
        _ => sqlx::query_scalar("SELECT id FROM generation_tasks")
            .fetch_all(pool)
            .await
            .map_err(AppError::db)?,
    };

    delete_tasks_batch(pool, &ids).await
}
