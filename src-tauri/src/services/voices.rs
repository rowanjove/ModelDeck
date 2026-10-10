use std::path::{Path, PathBuf};

use sqlx::SqlitePool;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use crate::domain::{now_iso, Voice, VoiceInput, VoiceUpdate};
use crate::error::AppError;
use crate::media;

pub async fn list(pool: &SqlitePool) -> Result<Vec<Voice>, AppError> {
    let rows = sqlx::query(
        "SELECT id, name, file_path, transcript, tags, language, notes, reference_files_json, created_at, updated_at
         FROM voices ORDER BY name, id",
    )
    .fetch_all(pool)
    .await
    .map_err(AppError::db)?;
    rows.iter().map(voice_from_row).collect()
}

pub async fn create(
    pool: &SqlitePool,
    data_dir: &Path,
    input: VoiceInput,
) -> Result<Voice, AppError> {
    let name = clean_name(&input.name)?;
    let transcript = clean_transcript(&input.transcript)?;
    let tags = clean_tags(&input.tags)?;
    let language = clean_language(input.language.as_deref())?;
    let notes = clean_notes(input.notes.as_deref())?;
    let id = Uuid::new_v4().to_string();
    let file_path = copy_audio(data_dir, &id, &input.source_path).await?;
    let reference_files = if let Some(refs) = &input.reference_paths {
        match copy_reference_audios(data_dir, &id, refs).await {
            Ok(paths) => paths,
            Err(err) => {
                remove_owned(data_dir, &file_path).await;
                return Err(err);
            }
        }
    } else {
        Vec::new()
    };
    let reference_files_json =
        serde_json::to_string(&reference_files).unwrap_or_else(|_| "[]".to_string());
    let now = now_iso();
    let res = sqlx::query(
        "INSERT INTO voices (id, name, file_path, transcript, tags, language, notes, reference_files_json, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&file_path)
    .bind(&transcript)
    .bind(&tags)
    .bind(&language)
    .bind(&notes)
    .bind(&reference_files_json)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await;

    if let Err(err) = res {
        remove_owned(data_dir, &file_path).await;
        for rf in &reference_files {
            remove_owned(data_dir, rf).await;
        }
        return Err(AppError::db(err));
    }
    Ok(Voice {
        id,
        name,
        file_path,
        transcript,
        tags,
        language,
        notes,
        reference_files,
        created_at: now.clone(),
        updated_at: now,
    })
}

pub async fn update(
    pool: &SqlitePool,
    data_dir: &Path,
    id: &str,
    input: VoiceUpdate,
) -> Result<Voice, AppError> {
    let current = get(pool, id).await?;
    let name = clean_name(&input.name)?;
    let transcript = clean_transcript(&input.transcript)?;
    let tags = clean_tags(&input.tags)?;
    let language = clean_language(input.language.as_deref())?;
    let notes = clean_notes(input.notes.as_deref())?;

    let mut newly_created_files: Vec<String> = Vec::new();

    let file_path = if let Some(source) = input
        .source_path
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        if source == current.file_path {
            current.file_path.clone()
        } else {
            let next = copy_audio(data_dir, id, source).await?;
            newly_created_files.push(next.clone());
            next
        }
    } else {
        current.file_path.clone()
    };

    let reference_files = if let Some(refs) = &input.reference_paths {
        let copied = match copy_reference_audios(data_dir, id, refs).await {
            Ok(paths) => paths,
            Err(err) => {
                for file in &newly_created_files {
                    remove_owned(data_dir, file).await;
                }
                if file_path != current.file_path {
                    remove_owned(data_dir, &file_path).await;
                }
                return Err(err);
            }
        };
        for p in &copied {
            if !current.reference_files.contains(p) {
                newly_created_files.push(p.clone());
            }
        }
        copied
    } else {
        current.reference_files.clone()
    };

    let reference_files_json =
        serde_json::to_string(&reference_files).unwrap_or_else(|_| "[]".to_string());
    let now = now_iso();

    let db_update = sqlx::query(
        "UPDATE voices SET name = ?, file_path = ?, transcript = ?, tags = ?, language = ?, notes = ?, reference_files_json = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&name)
    .bind(&file_path)
    .bind(&transcript)
    .bind(&tags)
    .bind(&language)
    .bind(&notes)
    .bind(&reference_files_json)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await;

    if let Err(err) = db_update {
        for f in newly_created_files {
            remove_owned(data_dir, &f).await;
        }
        return Err(AppError::db(err));
    }

    if file_path != current.file_path {
        remove_owned_if_unreferenced(pool, data_dir, &current.file_path, &file_path).await;
    }
    for old in &current.reference_files {
        if !reference_files.contains(old) {
            remove_owned_if_unreferenced(pool, data_dir, old, "").await;
        }
    }
    Ok(Voice {
        id: id.to_string(),
        name,
        file_path,
        transcript,
        tags,
        language,
        notes,
        reference_files,
        created_at: current.created_at,
        updated_at: now,
    })
}

pub async fn delete(pool: &SqlitePool, data_dir: &Path, id: &str) -> Result<(), AppError> {
    let current = get(pool, id).await?;
    sqlx::query("DELETE FROM voices WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(AppError::db)?;
    remove_owned_if_unreferenced(pool, data_dir, &current.file_path, "").await;
    for extra in &current.reference_files {
        remove_owned_if_unreferenced(pool, data_dir, extra, "").await;
    }
    Ok(())
}

async fn get(pool: &SqlitePool, id: &str) -> Result<Voice, AppError> {
    let row = sqlx::query(
        "SELECT id, name, file_path, transcript, tags, language, notes, reference_files_json, created_at, updated_at FROM voices WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::db)?
    .ok_or_else(|| AppError::not_found("找不到这个声音"))?;
    voice_from_row(&row)
}

fn voice_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Voice, AppError> {
    use sqlx::Row;
    let reference_files_raw: String = row
        .try_get("reference_files_json")
        .unwrap_or_else(|_| "[]".to_string());
    let reference_files: Vec<String> =
        serde_json::from_str(&reference_files_raw).unwrap_or_default();
    Ok(Voice {
        id: row.try_get("id").map_err(AppError::db)?,
        name: row.try_get("name").map_err(AppError::db)?,
        file_path: row.try_get("file_path").map_err(AppError::db)?,
        transcript: row.try_get("transcript").map_err(AppError::db)?,
        tags: row.try_get("tags").map_err(AppError::db)?,
        language: row.try_get("language").unwrap_or_default(),
        notes: row.try_get("notes").unwrap_or_default(),
        reference_files,
        created_at: row.try_get("created_at").map_err(AppError::db)?,
        updated_at: row.try_get("updated_at").map_err(AppError::db)?,
    })
}

fn clean_name(value: &str) -> Result<String, AppError> {
    let name = value.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::validation("声音名称需要 1 到 80 个字符"));
    }
    Ok(name.to_string())
}

fn clean_transcript(value: &str) -> Result<String, AppError> {
    let text = value.trim();
    if text.chars().count() > 20_000 {
        return Err(AppError::validation("参考文本过长"));
    }
    Ok(text.to_string())
}

fn clean_tags(value: &str) -> Result<String, AppError> {
    let text = value.trim();
    if text.chars().count() > 200 {
        return Err(AppError::validation("标签过长"));
    }
    Ok(text.to_string())
}

fn clean_language(value: Option<&str>) -> Result<String, AppError> {
    let text = value.unwrap_or("").trim();
    if text.chars().count() > 50 {
        return Err(AppError::validation("语言标识过长"));
    }
    Ok(text.to_string())
}

fn clean_notes(value: Option<&str>) -> Result<String, AppError> {
    let text = value.unwrap_or("").trim();
    if text.chars().count() > 2000 {
        return Err(AppError::validation("备注过长"));
    }
    Ok(text.to_string())
}

async fn copy_audio(data_dir: &Path, id: &str, source: &str) -> Result<String, AppError> {
    let file = Path::new(source.trim());
    if !file.is_file() {
        return Err(AppError::validation("找不到参考音频"));
    }
    let bytes = read_audio_file(file, "参考音频").await?;
    let mime = media::describe_audio(&bytes).map_err(AppError::validation)?;
    let dir = data_dir.join("audio").join("voices");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|err| AppError::storage(format!("无法创建声音库目录: {err}")))?;
    let path = dir.join(format!(
        "{id}_{}.{}",
        Uuid::new_v4().simple(),
        media::extension_for(&mime)
    ));
    crate::storage_util::atomic_write_async(&path, &bytes)
        .await
        .map_err(|err| AppError::storage(format!("无法保存参考音频: {err}")))?;
    Ok(path.to_string_lossy().to_string())
}

async fn copy_reference_audios(
    data_dir: &Path,
    id: &str,
    sources: &[String],
) -> Result<Vec<String>, AppError> {
    let dir = data_dir.join("audio").join("voices");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|err| AppError::storage(format!("无法创建声音库目录: {err}")))?;
    let mut paths: Vec<String> = Vec::new();
    for src in sources {
        if src.trim().is_empty() {
            continue;
        }
        let file = Path::new(src.trim());
        if !file.is_file() {
            continue;
        }
        // 无论输入文件位于何处（包括声音库自身），始终为该声音复制独立文件，防止共享引用导致误删
        let bytes = match read_audio_file(file, "附加参考音频").await {
            Ok(bytes) => bytes,
            Err(err) => {
                for path in &paths {
                    remove_owned(data_dir, path).await;
                }
                return Err(err);
            }
        };
        let mime = match media::describe_audio(&bytes) {
            Ok(mime) => mime,
            Err(err) => {
                for path in &paths {
                    remove_owned(data_dir, path).await;
                }
                return Err(AppError::validation(err));
            }
        };
        let path = dir.join(format!(
            "{id}_ref_{}.{}",
            Uuid::new_v4().simple(),
            media::extension_for(&mime)
        ));
        if let Err(err) = crate::storage_util::atomic_write_async(&path, &bytes).await {
            for existing in &paths {
                remove_owned(data_dir, existing).await;
            }
            return Err(AppError::storage(format!("无法保存附加参考音频: {err}")));
        }
        paths.push(path.to_string_lossy().to_string());
    }
    Ok(paths)
}

async fn read_audio_file(file: &Path, label: &str) -> Result<Vec<u8>, AppError> {
    let metadata = tokio::fs::metadata(file)
        .await
        .map_err(|err| AppError::validation(format!("无法读取{label}: {err}")))?;
    if metadata.len() > media::MAX_AUDIO_BYTES as u64 {
        return Err(AppError::validation("音频超过 40MB"));
    }

    let mut handle = tokio::fs::File::open(file)
        .await
        .map_err(|err| AppError::validation(format!("无法读取{label}: {err}")))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    let mut limited = (&mut handle).take(media::MAX_AUDIO_BYTES as u64 + 1);
    limited
        .read_to_end(&mut bytes)
        .await
        .map_err(|err| AppError::validation(format!("无法读取{label}: {err}")))?;
    if bytes.len() > media::MAX_AUDIO_BYTES {
        return Err(AppError::validation("音频超过 40MB"));
    }
    Ok(bytes)
}

async fn remove_owned(data_dir: &Path, stored: &str) {
    if stored.trim().is_empty() {
        return;
    }
    if let Some(path) = owned_path(data_dir, stored) {
        let _ = tokio::fs::remove_file(path).await;
    }
}

async fn remove_owned_if_unreferenced(
    pool: &SqlitePool,
    data_dir: &Path,
    stored: &str,
    keep: &str,
) {
    if stored == keep || stored.trim().is_empty() {
        return;
    }
    if owned_path(data_dir, stored).is_some() {
        // Use the same parsed, canonical path comparison as history cleanup.
        let referenced = crate::services::history::is_file_referenced(pool, stored)
            .await
            .unwrap_or(true); // 查询失败默认视为受保护，绝不误删
        if !referenced {
            remove_owned(data_dir, stored).await;
        }
    }
}

fn owned_path(data_dir: &Path, stored: &str) -> Option<PathBuf> {
    let path = PathBuf::from(stored.trim());
    let root = std::fs::canonicalize(data_dir.join("audio").join("voices")).ok()?;
    let canonical = std::fs::canonicalize(&path).ok()?;
    if canonical.starts_with(&root) && canonical != root {
        Some(canonical)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn oversized_audio_is_rejected_before_reading_into_memory() {
        let path =
            std::env::temp_dir().join(format!("ai-workbench-voice-limit-{}.audio", Uuid::new_v4()));
        let file = tokio::fs::File::create(&path).await.unwrap();
        file.set_len(media::MAX_AUDIO_BYTES as u64 + 1)
            .await
            .unwrap();
        drop(file);

        let result = read_audio_file(&path, "参考音频").await;
        let _ = tokio::fs::remove_file(&path).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("40MB"));
    }
}
