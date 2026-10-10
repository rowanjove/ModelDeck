use std::fmt;

#[cfg(test)]
use std::collections::BTreeMap;
#[cfg(test)]
use std::sync::Mutex;

use keyring::Entry;

use crate::error::AppError;

pub const PRIMARY_SERVICE: &str = "local.modeldeck";
pub const LEGACY_SERVICE: &str = "local.aiworkbench";

pub struct SecretString(String);

impl SecretString {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([redacted])")
    }
}

pub trait SecretStore: Send + Sync {
    fn set(&self, id: &str, secret: &str) -> Result<(), AppError>;
    fn get(&self, id: &str) -> Result<Option<SecretString>, AppError>;
    fn delete(&self, id: &str) -> Result<(), AppError>;
}

pub struct KeyringSecretStore;

impl SecretStore for KeyringSecretStore {
    fn set(&self, id: &str, secret: &str) -> Result<(), AppError> {
        Entry::new(PRIMARY_SERVICE, id)
            .map_err(|err| AppError::storage(format!("无法访问系统凭据库: {err}")))?
            .set_password(secret)
            .map_err(|err| AppError::storage(format!("无法写入系统凭据库: {err}")))
    }

    fn get(&self, id: &str) -> Result<Option<SecretString>, AppError> {
        // 1. 优先从首选服务标识 local.modeldeck 读取
        let primary_entry = Entry::new(PRIMARY_SERVICE, id)
            .map_err(|err| AppError::storage(format!("无法访问系统凭据库: {err}")))?;
        match primary_entry.get_password() {
            Ok(secret) => Ok(Some(SecretString::new(secret))),
            Err(keyring::Error::NoEntry) => {
                // 2. 回退尝试读取旧版历史服务标识 local.aiworkbench (D08 平滑兼容)
                let legacy_entry = Entry::new(LEGACY_SERVICE, id)
                    .map_err(|err| AppError::storage(format!("无法访问旧凭据库: {err}")))?;
                match legacy_entry.get_password() {
                    Ok(secret) => {
                        // 3. 透明无感迁移到 PRIMARY_SERVICE，保留 LEGACY 凭据防止用户回退至旧版本时失效
                        if let Err(e) = primary_entry.set_password(&secret) {
                            crate::logging::global_logger().warn(
                                "credentials",
                                None,
                                None,
                                None,
                                None,
                                "credential_migration_write_failed",
                                &e.to_string(),
                            );
                        } else {
                            crate::logging::global_logger().info(
                                "credentials",
                                None,
                                None,
                                None,
                                None,
                                "credential_migrated_from_legacy",
                                &format!("已成功将凭据 [{id}] 从 local.aiworkbench 迁移至 local.modeldeck"),
                            );
                        }
                        Ok(Some(SecretString::new(secret)))
                    }
                    Err(keyring::Error::NoEntry) => Ok(None),
                    Err(err) => Err(AppError::storage(format!("无法读取旧系统凭据: {err}"))),
                }
            }
            Err(err) => Err(AppError::storage(format!("无法读取系统凭据库: {err}"))),
        }
    }

    fn delete(&self, id: &str) -> Result<(), AppError> {
        // 删除时执行双清：彻底抹除 PRIMARY 与 LEGACY 中的敏感凭据
        let mut first_err = None;
        if let Ok(entry) = Entry::new(PRIMARY_SERVICE, id) {
            if let Err(err) = entry.delete_credential() {
                if !matches!(err, keyring::Error::NoEntry) {
                    first_err = Some(AppError::storage(format!("无法删除系统凭据: {err}")));
                }
            }
        }
        if let Ok(entry) = Entry::new(LEGACY_SERVICE, id) {
            if let Err(err) = entry.delete_credential() {
                if !matches!(err, keyring::Error::NoEntry) && first_err.is_none() {
                    first_err = Some(AppError::storage(format!("无法删除旧系统凭据: {err}")));
                }
            }
        }
        if let Some(err) = first_err {
            Err(err)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct MemorySecretStore {
    primary: Mutex<BTreeMap<String, String>>,
    legacy: Mutex<BTreeMap<String, String>>,
}

#[cfg(test)]
impl MemorySecretStore {
    pub fn set_legacy(&self, id: &str, secret: &str) -> Result<(), AppError> {
        self.legacy
            .lock()
            .map_err(|_| AppError::storage("旧凭据缓存锁损坏"))?
            .insert(id.to_string(), secret.to_string());
        Ok(())
    }

    pub fn has_primary(&self, id: &str) -> bool {
        self.primary
            .lock()
            .map(|m| m.contains_key(id))
            .unwrap_or(false)
    }

    pub fn has_legacy(&self, id: &str) -> bool {
        self.legacy
            .lock()
            .map(|m| m.contains_key(id))
            .unwrap_or(false)
    }
}

#[cfg(test)]
impl SecretStore for MemorySecretStore {
    fn set(&self, id: &str, secret: &str) -> Result<(), AppError> {
        self.primary
            .lock()
            .map_err(|_| AppError::storage("凭据缓存锁已损坏"))?
            .insert(id.to_string(), secret.to_string());
        Ok(())
    }

    fn get(&self, id: &str) -> Result<Option<SecretString>, AppError> {
        // 1. 查 primary
        if let Some(secret) = self
            .primary
            .lock()
            .map_err(|_| AppError::storage("凭据缓存锁已损坏"))?
            .get(id)
            .cloned()
        {
            return Ok(Some(SecretString::new(secret)));
        }
        // 2. 回退 legacy
        if let Some(secret) = self
            .legacy
            .lock()
            .map_err(|_| AppError::storage("凭据缓存锁已损坏"))?
            .get(id)
            .cloned()
        {
            // 透明写入 primary
            self.primary
                .lock()
                .map_err(|_| AppError::storage("凭据缓存锁已损坏"))?
                .insert(id.to_string(), secret.clone());
            return Ok(Some(SecretString::new(secret)));
        }
        Ok(None)
    }

    fn delete(&self, id: &str) -> Result<(), AppError> {
        self.primary
            .lock()
            .map_err(|_| AppError::storage("凭据缓存锁已损坏"))?
            .remove(id);
        self.legacy
            .lock()
            .map_err(|_| AppError::storage("凭据缓存锁已损坏"))?
            .remove(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_constants() {
        assert_eq!(PRIMARY_SERVICE, "local.modeldeck");
        assert_eq!(LEGACY_SERVICE, "local.aiworkbench");
    }

    #[test]
    fn test_legacy_credential_transparent_migration() {
        let store = MemorySecretStore::default();
        let cred_id = "cred-legacy-123";
        let api_key = "sk-legacy-token-abc";

        // 模拟旧版本用户已存有凭据在 legacy 服务
        store.set_legacy(cred_id, api_key).unwrap();
        assert!(!store.has_primary(cred_id));
        assert!(store.has_legacy(cred_id));

        // 首次读取：触发自动透明迁移
        let retrieved = store.get(cred_id).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().expose(), api_key);

        // 验证已迁移至 primary，且 legacy 仍然保留
        assert!(store.has_primary(cred_id));
        assert!(store.has_legacy(cred_id));

        // 删除凭据：触发双清
        store.delete(cred_id).unwrap();
        assert!(!store.has_primary(cred_id));
        assert!(!store.has_legacy(cred_id));
        assert!(store.get(cred_id).unwrap().is_none());
    }
}
