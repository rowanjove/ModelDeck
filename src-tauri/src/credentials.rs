use std::fmt;

#[cfg(test)]
use std::collections::BTreeMap;
#[cfg(test)]
use std::sync::Mutex;

use keyring::Entry;

use crate::error::AppError;

const SERVICE: &str = "local.aiworkbench";

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
        Entry::new(SERVICE, id)
            .map_err(|err| AppError::storage(format!("无法访问系统凭据库: {err}")))?
            .set_password(secret)
            .map_err(|err| AppError::storage(format!("无法写入系统凭据库: {err}")))
    }

    fn get(&self, id: &str) -> Result<Option<SecretString>, AppError> {
        let entry = Entry::new(SERVICE, id)
            .map_err(|err| AppError::storage(format!("无法访问系统凭据库: {err}")))?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(SecretString::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(AppError::storage(format!("无法读取系统凭据库: {err}"))),
        }
    }

    fn delete(&self, id: &str) -> Result<(), AppError> {
        let entry = Entry::new(SERVICE, id)
            .map_err(|err| AppError::storage(format!("无法访问系统凭据库: {err}")))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(AppError::storage(format!("无法删除系统凭据: {err}"))),
        }
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct MemorySecretStore {
    inner: Mutex<BTreeMap<String, String>>,
}

#[cfg(test)]
impl SecretStore for MemorySecretStore {
    fn set(&self, id: &str, secret: &str) -> Result<(), AppError> {
        self.lock()?.insert(id.to_string(), secret.to_string());
        Ok(())
    }

    fn get(&self, id: &str) -> Result<Option<SecretString>, AppError> {
        Ok(self.lock()?.get(id).cloned().map(SecretString::new))
    }

    fn delete(&self, id: &str) -> Result<(), AppError> {
        self.lock()?.remove(id);
        Ok(())
    }
}

#[cfg(test)]
impl MemorySecretStore {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, BTreeMap<String, String>>, AppError> {
        self.inner
            .lock()
            .map_err(|_| AppError::storage("凭据缓存锁已损坏"))
    }
}
