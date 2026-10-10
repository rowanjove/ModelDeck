use std::fmt;

use serde::Serialize;
use serde_json::Value;

pub const AUTH: &str = "AuthenticationError";
pub const NETWORK: &str = "NetworkError";
pub const TIMEOUT: &str = "TimeoutError";
pub const RATE_LIMIT: &str = "RateLimitError";
pub const INVALID_REQUEST: &str = "InvalidRequestError";
pub const UNSUPPORTED: &str = "UnsupportedCapabilityError";
pub const PROVIDER: &str = "ProviderError";
pub const VALIDATION: &str = "ValidationError";
pub const STORAGE: &str = "StorageError";
pub const NOT_FOUND: &str = "NotFoundError";
pub const CONFLICT: &str = "ConflictError";

#[derive(Debug, Clone)]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
    pub status: Option<u16>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub raw: Option<Box<Value>>,
}

impl AppError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            status: None,
            provider: None,
            model: None,
            raw: None,
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(VALIDATION, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(CONFLICT, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(UNSUPPORTED, message)
    }

    pub fn storage(message: impl Into<String>) -> Self {
        Self::new(STORAGE, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(NOT_FOUND, message)
    }

    pub fn db(err: sqlx::Error) -> Self {
        Self::storage(format!("数据库错误: {err}"))
    }

    pub fn network(message: impl Into<String>, provider: &str, model: Option<&str>) -> Self {
        Self::new(NETWORK, message).context(provider, model)
    }

    pub fn timeout(provider: &str, model: Option<&str>) -> Self {
        Self::new(TIMEOUT, "请求超时").context(provider, model)
    }

    pub fn auth(message: impl Into<String>, provider: &str, model: Option<&str>) -> Self {
        Self::new(AUTH, message).context(provider, model)
    }

    pub fn provider(
        message: impl Into<String>,
        status: u16,
        provider: &str,
        model: Option<&str>,
        raw: Option<Value>,
    ) -> Self {
        let mut error = Self::new(PROVIDER, message).context(provider, model);
        error.status = Some(status);
        error.raw = raw.map(Box::new);
        error
    }

    pub fn status(
        code: &'static str,
        message: impl Into<String>,
        status: u16,
        provider: &str,
        model: Option<&str>,
        raw: Option<Value>,
    ) -> Self {
        let mut error = Self::new(code, message).context(provider, model);
        error.status = Some(status);
        error.raw = raw.map(Box::new);
        error
    }

    pub fn context(mut self, provider: &str, model: Option<&str>) -> Self {
        self.provider = Some(provider.to_string());
        self.model = model.map(ToOwned::to_owned);
        self
    }

    pub fn annotate(mut self, provider: &str, model: &str) -> Self {
        if self.provider.is_none() {
            self.provider = Some(provider.to_string());
        }
        if self.model.is_none() && !model.is_empty() {
            self.model = Some(model.to_string());
        }
        self
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
    status: Option<u16>,
    provider: Option<&'a str>,
    model: Option<&'a str>,
    raw: Option<&'a Value>,
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ErrorBody {
            code: self.code,
            message: &self.message,
            status: self.status,
            provider: self.provider.as_deref(),
            model: self.model.as_deref(),
            raw: self.raw.as_deref(),
        }
        .serialize(serializer)
    }
}
