use std::fs::File;
use std::io::Write;
use std::path::Path;
use uuid::Uuid;

use crate::error::AppError;

pub const MIN_DISK_SPACE_BYTES: u64 = 500 * 1024 * 1024; // 500 MB 安全底线

#[cfg(windows)]
mod platform {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    extern "system" {
        fn GetDiskFreeSpaceExW(
            lpDirectoryName: *const u16,
            lpFreeBytesAvailableToCaller: *mut u64,
            lpTotalNumberOfBytes: *mut u64,
            lpTotalNumberOfFreeBytes: *mut u64,
        ) -> i32;

        fn MoveFileExW(
            lpExistingFileName: *const u16,
            lpNewFileName: *const u16,
            dwFlags: u32,
        ) -> i32;
    }

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x00000001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x00000008;

    fn to_wide(s: &OsStr) -> Vec<u16> {
        let mut wide: Vec<u16> = s.encode_wide().collect();
        wide.push(0);
        wide
    }

    pub fn free_space_bytes(path: &Path) -> Option<u64> {
        // 获取有效存在的祖先目录
        let check_dir = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        let wide = to_wide(check_dir.as_os_str());
        let mut free_bytes: u64 = 0;
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free_bytes,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if ok != 0 {
            Some(free_bytes)
        } else {
            None
        }
    }

    pub fn atomic_replace(src: &Path, dst: &Path) -> std::io::Result<()> {
        let src_wide = to_wide(src.as_os_str());
        let dst_wide = to_wide(dst.as_os_str());
        let ok = unsafe {
            MoveFileExW(
                src_wide.as_ptr(),
                dst_wide.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if ok != 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::Path;

    pub fn free_space_bytes(_path: &Path) -> Option<u64> {
        None
    }

    pub fn atomic_replace(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::fs::rename(src, dst)
    }
}

/// 获取磁盘剩余空间字节数
pub fn platform_free_space(target: &Path) -> Option<u64> {
    platform::free_space_bytes(target)
}

/// 检查目标路径所在磁盘的剩余空间
pub fn check_disk_space(target: &Path, additional_bytes: u64) -> Result<(), AppError> {
    if let Some(free_bytes) = platform_free_space(target) {
        let needed = additional_bytes.saturating_add(MIN_DISK_SPACE_BYTES);
        if free_bytes < needed {
            return Err(AppError::storage(format!(
                "磁盘空间不足: 剩余 {} MB，低于安全阈值 ({} MB)",
                free_bytes / (1024 * 1024),
                needed / (1024 * 1024)
            )));
        }
    }
    Ok(())
}

/// 同步原子写入：先写入临时文件，flush + sync_all 后原子重命名替换目标文件
pub fn atomic_write(target: &Path, content: &[u8]) -> Result<(), AppError> {
    let parent = target
        .parent()
        .ok_or_else(|| AppError::storage("目标路径无效"))?;
    if !parent.exists() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::storage(format!("无法创建目录 {}: {e}", parent.display())))?;
    }

    // 检查磁盘空间
    check_disk_space(target, content.len() as u64)?;

    let tmp_name = format!(
        ".{}.tmp.{}",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file"),
        Uuid::new_v4()
    );
    let tmp_path = parent.join(tmp_name);

    let write_res = (|| -> std::io::Result<()> {
        let mut file = File::create(&tmp_path)?;
        file.write_all(content)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        platform::atomic_replace(&tmp_path, target)?;
        Ok(())
    })();

    if let Err(err) = write_res {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(AppError::storage(format!(
            "原子写入文件失败 {}: {err}",
            target.display()
        )));
    }

    Ok(())
}

/// 异步原子写入
pub async fn atomic_write_async(target: &Path, content: &[u8]) -> Result<(), AppError> {
    let target = target.to_path_buf();
    let content = content.to_vec();
    tokio::task::spawn_blocking(move || atomic_write(&target, &content))
        .await
        .map_err(|e| AppError::storage(format!("原子写入任务执行失败: {e}")))?
}

/// 校验任务 ID 是否为合法的 UUID 格式，禁止路径遍历符号与非法字符
pub fn validate_task_id(task_id: &str) -> Result<String, AppError> {
    let trimmed = task_id.trim();
    if trimmed.is_empty() {
        return Err(AppError::validation("任务 ID 不能为空"));
    }
    // 强制验证为有效 UUID，杜绝 ../、绝对路径等逃逸攻击
    Uuid::parse_str(trimmed).map_err(|_| {
        AppError::validation(format!(
            "任务 ID 非法: 必须是合法的 UUID 格式，接收到: '{trimmed}'"
        ))
    })?;
    Ok(trimmed.to_string())
}

pub fn safe_canonicalize(path: &Path) -> std::io::Result<std::path::PathBuf> {
    let p = std::fs::canonicalize(path)?;
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return Ok(std::path::PathBuf::from(stripped));
        }
    }
    Ok(p)
}

/// 确保 target 路径在 base_dir 内部，且规范化后没有逃逸
pub fn ensure_within_dir(base_dir: &Path, target: &Path) -> Result<(), AppError> {
    let base_canonical = safe_canonicalize(base_dir)
        .or_else(|_| {
            std::fs::create_dir_all(base_dir)?;
            safe_canonicalize(base_dir)
        })
        .map_err(|e| AppError::storage(format!("无法解析基础目录 {}: {e}", base_dir.display())))?;

    let parent = target.parent().unwrap_or(target);
    let parent_canonical = safe_canonicalize(parent)
        .or_else(|_| {
            std::fs::create_dir_all(parent)?;
            safe_canonicalize(parent)
        })
        .map_err(|e| AppError::storage(format!("无法解析目标目录 {}: {e}", parent.display())))?;

    if !parent_canonical.starts_with(&base_canonical) {
        return Err(AppError::validation(format!(
            "路径逃逸风险: 目标路径 {} 超出基础目录 {}",
            target.display(),
            base_dir.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_task_id() {
        let valid_uuid = Uuid::new_v4().to_string();
        assert_eq!(validate_task_id(&valid_uuid).unwrap(), valid_uuid);

        // 非法路径或字符
        assert!(validate_task_id("../escape").is_err());
        assert!(validate_task_id("..\\escape").is_err());
        assert!(validate_task_id("/etc/passwd").is_err());
        assert!(validate_task_id("C:\\Windows\\System32").is_err());
        assert!(validate_task_id("regular-string").is_err());
        assert!(validate_task_id("   ").is_err());
    }

    #[test]
    fn test_ensure_within_dir() {
        let base = std::env::temp_dir().join(format!("ai_wb_base_{}", Uuid::new_v4()));
        let safe_file = base.join("subdir").join("test.txt");
        assert!(ensure_within_dir(&base, &safe_file).is_ok());

        let outside_file = std::env::temp_dir().join("outside.txt");
        assert!(ensure_within_dir(&base, &outside_file).is_err());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_atomic_write_creates_and_overwrites() {
        let dir = std::env::temp_dir().join(format!("ai_wb_test_{}", Uuid::new_v4()));
        let file = dir.join("config.json");
        atomic_write(&file, b"{\"version\": 1}").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{\"version\": 1}");

        // 再次原子写入覆盖
        atomic_write(&file, b"{\"version\": 2}").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{\"version\": 2}");

        // 清理
        let _ = std::fs::remove_dir_all(&dir);
    }
}
