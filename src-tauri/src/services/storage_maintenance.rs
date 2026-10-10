use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageReport {
    pub total_asset_records: usize,
    pub disk_files_count: usize,
    pub disk_total_bytes: u64,
    pub orphan_files: Vec<String>,
    pub missing_files: Vec<String>,
    pub temp_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanReport {
    pub cleaned_files_count: usize,
    pub freed_bytes: u64,
}

pub const AGE_PROTECTION_SECS: u64 = 1800; // 30 分钟保护窗口，防止清理正在写入或刚完成的文件

fn is_older_than(metadata: Option<&std::fs::Metadata>, secs: u64) -> bool {
    let Some(meta) = metadata else {
        return false;
    };
    if let Ok(modified) = meta.modified() {
        if let Ok(elapsed) = std::time::SystemTime::now().duration_since(modified) {
            return elapsed.as_secs() >= secs;
        }
    }
    false
}

fn normalize_path_set(path_str: &str, set: &mut HashSet<String>) {
    set.extend(crate::services::history::path_keys(path_str));
}

/// 扫描数据目录并比对数据库，生成存储健康报告
pub async fn inspect_storage(
    data_dir: &Path,
    pool: &SqlitePool,
) -> Result<StorageReport, AppError> {
    // 1. 查询数据库中所有登记的 asset path 以及声音库 path
    let mut db_paths: HashSet<String> = HashSet::new();
    let mut missing_files = Vec::new();

    let asset_rows =
        sqlx::query_scalar::<_, Option<String>>("SELECT path FROM assets WHERE path IS NOT NULL")
            .fetch_all(pool)
            .await
            .map_err(AppError::db)?;

    for row in asset_rows.into_iter().flatten() {
        let p = PathBuf::from(&row);
        if !p.exists() {
            missing_files.push(row.clone());
        }
        normalize_path_set(&row, &mut db_paths);
    }

    let voice_rows = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "SELECT file_path, reference_files_json FROM voices",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for (file_path, ref_json) in voice_rows {
        if let Some(fp) = file_path {
            if !fp.trim().is_empty() {
                normalize_path_set(&fp, &mut db_paths);
            }
        }
        if let Some(json_str) = ref_json {
            if let Ok(refs) = serde_json::from_str::<Vec<String>>(&json_str) {
                for r in refs {
                    if !r.trim().is_empty() {
                        normalize_path_set(&r, &mut db_paths);
                    }
                }
            }
        }
    }

    // 2. 遍历磁盘媒体目录
    let mut disk_files_count = 0;
    let mut disk_total_bytes = 0;
    let mut orphan_files = Vec::new();
    let mut temp_files = Vec::new();

    for sub_dir in ["images", "videos", "audio", "files"] {
        let target_dir = data_dir.join(sub_dir);
        if !target_dir.exists() {
            continue;
        }

        let mut file_paths = Vec::new();
        if let Ok(mut entries) = tokio::fs::read_dir(&target_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_file() {
                    file_paths.push(path);
                } else if path.is_dir() {
                    if let Ok(mut sub_entries) = tokio::fs::read_dir(&path).await {
                        while let Ok(Some(sub_entry)) = sub_entries.next_entry().await {
                            if sub_entry.path().is_file() {
                                file_paths.push(sub_entry.path());
                            }
                        }
                    }
                }
            }
        }

        for path in file_paths {
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();

            let metadata = std::fs::metadata(&path).ok();
            let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            let is_old_enough = is_older_than(metadata.as_ref(), AGE_PROTECTION_SECS);

            disk_files_count += 1;
            disk_total_bytes += size;

            // 临时碎片文件检查（需超出 30 分钟保护窗口）
            if file_name.starts_with('.') && file_name.contains(".tmp.") {
                if is_old_enough {
                    temp_files.push(path.to_string_lossy().to_string());
                }
                continue;
            }

            // 孤儿文件检查（非临时文件、未在数据库登记、且超出 30 分钟保护窗口）
            let disk_keys = crate::services::history::path_keys(&path.to_string_lossy());
            if disk_keys.is_disjoint(&db_paths) && is_old_enough {
                orphan_files.push(path.to_string_lossy().to_string());
            }
        }
    }

    Ok(StorageReport {
        total_asset_records: db_paths.len(),
        disk_files_count,
        disk_total_bytes,
        orphan_files,
        missing_files,
        temp_files,
    })
}

/// 清理所有临时碎片文件和选中的孤儿文件（严格限定在应用数据目录内的媒体子目录中）
pub async fn clean_storage(
    data_dir: &Path,
    pool: &SqlitePool,
    files_to_delete: &[String],
) -> Result<CleanReport, AppError> {
    let mut cleaned_files_count = 0;
    let mut freed_bytes = 0;

    let root = std::fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());
    let allowed_subdirs: Vec<PathBuf> = ["images", "videos", "audio", "files"]
        .iter()
        .map(|sub| root.join(sub))
        .collect();

    for path_str in files_to_delete {
        let p = Path::new(path_str);
        if let Ok(canon) = std::fs::canonicalize(p) {
            // 1. 严格沙箱校验：必须在合法媒体子目录内且不能是目录本身
            let in_sandbox = allowed_subdirs
                .iter()
                .any(|allowed| canon.starts_with(allowed) && &canon != allowed);
            if !in_sandbox || !canon.is_file() {
                continue;
            }

            // 2. 时间保护窗口：30分钟内刚写入的文件禁止清理，防止与活动任务并发竞态
            let meta = std::fs::metadata(&canon).ok();
            if !is_older_than(meta.as_ref(), AGE_PROTECTION_SECS) {
                continue;
            }

            // 3. 二次核实数据库引用：避免刚完成落库的任务素材被误删，查询失败默认视为受保护
            let canon_str = canon.to_string_lossy();
            if crate::services::history::is_file_referenced(pool, &canon_str)
                .await
                .unwrap_or(true)
            {
                continue;
            }

            let file_size = meta.map(|m| m.len()).unwrap_or(0);
            if std::fs::remove_file(&canon).is_ok() {
                freed_bytes += file_size;
                cleaned_files_count += 1;
            }
        }
    }

    Ok(CleanReport {
        cleaned_files_count,
        freed_bytes,
    })
}
