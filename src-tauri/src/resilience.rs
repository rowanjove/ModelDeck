use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::AppError;

pub const DEFAULT_MAX_RETRIES: u32 = 2; // 最多重试2次（共3次执行机会）
pub const CIRCUIT_FAILURE_THRESHOLD: u32 = 5; // 连续失败5次触发熔断
pub const CIRCUIT_COOLDOWN: Duration = Duration::from_secs(30); // 熔断冷却期30秒

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

struct ProviderCircuit {
    consecutive_failures: u32,
    last_failure_at: Option<Instant>,
    state: CircuitState,
}

impl Default for ProviderCircuit {
    fn default() -> Self {
        Self {
            consecutive_failures: 0,
            last_failure_at: None,
            state: CircuitState::Closed,
        }
    }
}

pub struct CircuitBreakerRegistry {
    breakers: Mutex<HashMap<String, ProviderCircuit>>,
}

impl Default for CircuitBreakerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

static GLOBAL_BREAKERS: std::sync::OnceLock<std::sync::Arc<CircuitBreakerRegistry>> =
    std::sync::OnceLock::new();

pub fn init_global_breakers(breakers: std::sync::Arc<CircuitBreakerRegistry>) {
    let _ = GLOBAL_BREAKERS.set(breakers);
}

pub fn global_breakers() -> std::sync::Arc<CircuitBreakerRegistry> {
    GLOBAL_BREAKERS
        .get_or_init(|| std::sync::Arc::new(CircuitBreakerRegistry::new()))
        .clone()
}

impl CircuitBreakerRegistry {
    pub fn new() -> Self {
        Self {
            breakers: Mutex::new(HashMap::new()),
        }
    }

    /// 重置所有熔断器状态（测试或重置时使用）
    pub fn reset(&self) {
        let mut map = self.breakers.lock().unwrap_or_else(|e| e.into_inner());
        map.clear();
    }

    /// 检查 Provider 是否允许请求通过
    pub fn check_permission(&self, provider_id: &str, provider_name: &str) -> Result<(), AppError> {
        let mut map = self.breakers.lock().unwrap_or_else(|e| e.into_inner());
        let circuit = map.entry(provider_id.to_string()).or_default();

        match circuit.state {
            CircuitState::Closed => Ok(()),
            CircuitState::Open => {
                if let Some(last_fail) = circuit.last_failure_at {
                    if last_fail.elapsed() >= CIRCUIT_COOLDOWN {
                        // 冷却结束，转为半开试探
                        circuit.state = CircuitState::HalfOpen;
                        return Ok(());
                    }
                }
                Err(AppError::provider(
                    format!(
                        "Provider「{}」连续失败过多，熔断保护中。请 30 秒后再试",
                        provider_name
                    ),
                    503,
                    provider_name,
                    None,
                    None,
                ))
            }
            CircuitState::HalfOpen => {
                // 半开状态允许放行试探
                Ok(())
            }
        }
    }

    /// 记录请求结果
    pub fn record_result(&self, provider_id: &str, success: bool) {
        let mut map = self.breakers.lock().unwrap_or_else(|e| e.into_inner());
        let circuit = map.entry(provider_id.to_string()).or_default();

        if success {
            circuit.consecutive_failures = 0;
            circuit.last_failure_at = None;
            circuit.state = CircuitState::Closed;
        } else {
            circuit.consecutive_failures = circuit.consecutive_failures.saturating_add(1);
            circuit.last_failure_at = Some(Instant::now());
            if circuit.consecutive_failures >= CIRCUIT_FAILURE_THRESHOLD {
                circuit.state = CircuitState::Open;
            }
        }
    }
}

pub fn is_retryable_status(status: u16) -> bool {
    matches!(status, 429 | 502 | 503 | 504)
}

/// 判断 HTTP 状态或错误类型是否支持安全重试
pub fn is_retryable(err: &AppError) -> bool {
    if let Some(status) = err.status {
        // 429 RateLimit, 502/503/504 Gateway/Service Unavailable 属于临时性网络或压力故障，可重试
        if is_retryable_status(status) {
            return true;
        }
        // 400, 401, 403, 404, 422 等客户端/认证/参数错误坚决不重试
        if (400..500).contains(&status) {
            return false;
        }
    }
    // 超时与网络中断属于临时故障，可重试
    err.code == crate::error::TIMEOUT || err.code == crate::error::NETWORK
}

/// 指数退避等待延迟计算（含轻度 jitter）
pub fn backoff_delay(attempt: u32) -> Duration {
    let base_ms = match attempt {
        0 => 600,
        1 => 1200,
        _ => 2400,
    };
    // 简易伪随机扰动 (0 ~ 100ms)
    let jitter_ms = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_millis()
        % 100) as u64;
    Duration::from_millis(base_ms + jitter_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retryable_classification() {
        let e429 = AppError::provider("Too Many Requests", 429, "OpenAI", None, None);
        assert!(is_retryable(&e429));

        let e502 = AppError::provider("Bad Gateway", 502, "OpenAI", None, None);
        assert!(is_retryable(&e502));

        let e401 = AppError::auth("Unauthorized", "OpenAI", None);
        assert!(!is_retryable(&e401));

        let e400 = AppError::validation("Bad parameter");
        assert!(!is_retryable(&e400));
    }

    #[test]
    fn test_circuit_breaker_transitions() {
        let registry = CircuitBreakerRegistry::new();
        let pid = "p1";
        let pname = "TestProvider";

        assert!(registry.check_permission(pid, pname).is_ok());

        // 连续记录4次失败，还未达到阈值5
        for _ in 0..4 {
            registry.record_result(pid, false);
        }
        assert!(registry.check_permission(pid, pname).is_ok());

        // 第5次失败触发熔断 Open
        registry.record_result(pid, false);
        assert!(registry.check_permission(pid, pname).is_err());

        // 恢复成功一次后重置为 Closed
        registry.record_result(pid, true);
        assert!(registry.check_permission(pid, pname).is_ok());
    }
}
