use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::domain::now_iso;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,  // "info" | "warn" | "error"
    pub module: String, // "text" | "image" | "video" | "audio" | "exchange" | "task" | "db"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub event: String,
    pub message: String,
}

pub struct LogBuffer {
    max_entries: usize,
    entries: Mutex<VecDeque<LogEntry>>,
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::new(1000)
    }
}

#[allow(clippy::too_many_arguments)]
impl LogBuffer {
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries,
            entries: Mutex::new(VecDeque::with_capacity(max_entries.min(1024))),
        }
    }

    pub fn record(
        &self,
        level: &str,
        module: &str,
        request_id: Option<&str>,
        task_id: Option<&str>,
        provider_id: Option<&str>,
        model_id: Option<&str>,
        event: &str,
        message: &str,
    ) {
        let entry = LogEntry {
            timestamp: now_iso(),
            level: level.to_string(),
            module: module.to_string(),
            request_id: request_id.map(|s| s.to_string()),
            task_id: task_id.map(|s| s.to_string()),
            provider_id: provider_id.map(|s| s.to_string()),
            model_id: model_id.map(|s| s.to_string()),
            event: event.to_string(),
            // Ensure message is sanitized without destroying useful diagnostic information
            message: crate::redact::mask_embedded(message),
        };

        let mut queue = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if queue.len() >= self.max_entries {
            queue.pop_front();
        }
        queue.push_back(entry);
    }

    pub fn info(
        &self,
        module: &str,
        request_id: Option<&str>,
        task_id: Option<&str>,
        provider_id: Option<&str>,
        model_id: Option<&str>,
        event: &str,
        message: &str,
    ) {
        self.record(
            "info",
            module,
            request_id,
            task_id,
            provider_id,
            model_id,
            event,
            message,
        );
    }

    pub fn warn(
        &self,
        module: &str,
        request_id: Option<&str>,
        task_id: Option<&str>,
        provider_id: Option<&str>,
        model_id: Option<&str>,
        event: &str,
        message: &str,
    ) {
        self.record(
            "warn",
            module,
            request_id,
            task_id,
            provider_id,
            model_id,
            event,
            message,
        );
    }

    pub fn error(
        &self,
        module: &str,
        request_id: Option<&str>,
        task_id: Option<&str>,
        provider_id: Option<&str>,
        model_id: Option<&str>,
        event: &str,
        message: &str,
    ) {
        self.record(
            "error",
            module,
            request_id,
            task_id,
            provider_id,
            model_id,
            event,
            message,
        );
    }

    pub fn recent(&self, limit: usize) -> Vec<LogEntry> {
        let queue = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let count = queue.len().min(limit);
        queue.iter().rev().take(count).cloned().collect()
    }

    pub fn clear(&self) {
        let mut queue = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        queue.clear();
    }
}

static GLOBAL_LOGS: std::sync::OnceLock<Arc<LogBuffer>> = std::sync::OnceLock::new();

pub fn init_global_logger(buffer: Arc<LogBuffer>) {
    let _ = GLOBAL_LOGS.set(buffer);
}

pub fn global_logger() -> Arc<LogBuffer> {
    GLOBAL_LOGS
        .get_or_init(|| Arc::new(LogBuffer::default()))
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_buffer_bounds_and_sanitization() {
        let buffer = LogBuffer::new(5);
        for i in 0..10 {
            buffer.info(
                "exchange",
                Some(&format!("req_{i}")),
                None,
                Some("openai"),
                None,
                "request_sent",
                &format!("Secret sk-1234567890abcdef message {i}"),
            );
        }
        let recent = buffer.recent(10);
        assert_eq!(recent.len(), 5);
        // Ensure newest is first
        assert_eq!(recent[0].request_id.as_deref(), Some("req_9"));
        // Ensure sensitive token is masked
        assert!(recent[0].message.contains("••••"));
        assert!(!recent[0].message.contains("sk-1234567890abcdef"));
    }
}
