use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::credentials::SecretString;
use crate::domain::{MediaRef, ModelDefinition, ProviderRecord, ReferenceImage};

#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolLog {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub result: String,
    pub ok: bool,
}

#[derive(Debug, Clone)]
pub struct TextTurn {
    pub role: String,
    pub text: String,
    pub images: Vec<MediaRef>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_call_id: Option<String>,
    pub name: Option<String>,
}

impl TextTurn {
    pub fn plain(role: impl Into<String>, text: impl Into<String>, images: Vec<MediaRef>) -> Self {
        Self {
            role: role.into(),
            text: text.into(),
            images,
            tool_calls: Vec::new(),
            tool_call_id: None,
            name: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TextPayload {
    pub system_prompt: Option<String>,
    pub turns: Vec<TextTurn>,
    pub parameters: Map<String, Value>,
    pub extra: Option<Value>,
    pub tools: Vec<ToolSpec>,
}

pub fn tool_schema(tools: &[ToolSpec]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    }
                })
            })
            .collect(),
    )
}

#[derive(Debug, Clone)]
pub enum LiveDelta {
    Text(String),
    Reasoning(String),
}

#[derive(Debug)]
pub struct ImagePayload {
    pub prompt: String,
    pub negative_prompt: Option<String>,
    pub source_image: Option<MediaRef>,
    pub reference_images: Vec<ReferenceImage>,
    pub mask: Option<MediaRef>,
    pub parameters: Map<String, Value>,
    pub extra: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct ImageBinary {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedImage {
    pub path: String,
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageResult {
    pub images: Vec<GeneratedImage>,
    pub latency_ms: u64,
    pub provider_id: String,
    pub provider_name: String,
    pub model_definition_id: String,
    pub model_name: String,
    pub model_id: String,
    pub raw_request: Value,
    pub raw_response: Value,
    pub status: u16,
    #[serde(skip)]
    pub binaries: Vec<ImageBinary>,
}

#[derive(Debug)]
pub struct VideoPayload {
    pub prompt: String,
    pub negative_prompt: Option<String>,
    pub first_frame: Option<MediaRef>,
    pub last_frame: Option<MediaRef>,
    pub reference_images: Vec<ReferenceImage>,
    pub reference_videos: Vec<MediaRef>,
    pub reference_audios: Vec<MediaRef>,
    pub parameters: Map<String, Value>,
    pub extra: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct VideoBinary {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedVideo {
    pub path: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoResult {
    pub videos: Vec<GeneratedVideo>,
    pub latency_ms: u64,
    pub provider_id: String,
    pub provider_name: String,
    pub model_definition_id: String,
    pub model_name: String,
    pub model_id: String,
    pub raw_request: Value,
    pub raw_response: Value,
    pub status: u16,
    #[serde(skip)]
    pub binaries: Vec<VideoBinary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioTask {
    Tts,
    Stt,
    Sts,
    Clone,
}

#[derive(Debug)]
pub struct AudioPayload {
    pub task: AudioTask,
    pub text: String,
    pub voice: Option<String>,
    pub source_audio: Option<MediaRef>,
    pub reference_audio: Option<MediaRef>,
    pub reference_transcript: Option<String>,
    pub parameters: Map<String, Value>,
    pub extra: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct AudioBinary {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedAudio {
    pub path: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioResult {
    pub audios: Vec<GeneratedAudio>,
    pub latency_ms: u64,
    pub provider_id: String,
    pub provider_name: String,
    pub model_definition_id: String,
    pub model_name: String,
    pub model_id: String,
    pub raw_request: Value,
    pub raw_response: Value,
    pub status: u16,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub transcript: String,
    #[serde(skip)]
    pub transcript_path: Option<String>,
    #[serde(skip)]
    pub binaries: Vec<AudioBinary>,
}

#[derive(Debug)]
pub enum CanonicalRequest {
    Text(TextPayload),
    Image(ImagePayload),
    Video(VideoPayload),
    Audio(AudioPayload),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextResult {
    pub text: String,
    pub usage: Option<Usage>,
    pub latency_ms: u64,
    pub provider_id: String,
    pub provider_name: String,
    pub model_definition_id: String,
    pub model_name: String,
    pub model_id: String,
    pub raw_request: Value,
    pub raw_response: Value,
    pub status: u16,
    pub stopped: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reasoning: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interrupted: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug)]
pub enum CanonicalResponse {
    Text(TextResult),
    Image(ImageResult),
    Video(VideoResult),
    Audio(AudioResult),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionReport {
    pub latency_ms: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteModel {
    pub id: String,
    pub name: String,
}

pub struct AdapterContext {
    pub http: Client,
    pub provider: ProviderRecord,
    pub model: Option<ModelDefinition>,
    pub api_key: Option<SecretString>,
    pub chat_path: Option<String>,
    pub models_path: Option<String>,
    pub cancelled: Arc<AtomicBool>,
    pub request_id: String,
    pub task_id: Option<String>,
    pub breakers: Option<Arc<crate::resilience::CircuitBreakerRegistry>>,
    pub logs: Option<Arc<crate::logging::LogBuffer>>,
}

impl AdapterContext {
    pub fn new(
        http: Client,
        provider: ProviderRecord,
        model: Option<ModelDefinition>,
        api_key: Option<SecretString>,
        chat_path: Option<String>,
        models_path: Option<String>,
    ) -> Self {
        Self {
            http,
            provider,
            model,
            api_key,
            chat_path,
            models_path,
            cancelled: Arc::new(AtomicBool::new(false)),
            request_id: format!("req_{}", uuid::Uuid::new_v4().simple()),
            task_id: None,
            breakers: Some(crate::resilience::global_breakers()),
            logs: Some(crate::logging::global_logger()),
        }
    }

    pub fn with_cancelled(mut self, cancelled: Arc<AtomicBool>) -> Self {
        self.cancelled = cancelled;
        self
    }

    #[allow(dead_code)]
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = request_id.into();
        self
    }

    #[allow(dead_code)]
    pub fn with_task_id(mut self, task_id: Option<String>) -> Self {
        self.task_id = task_id;
        self
    }

    #[allow(dead_code)]
    pub fn with_breakers(
        mut self,
        breakers: Arc<crate::resilience::CircuitBreakerRegistry>,
    ) -> Self {
        self.breakers = Some(breakers);
        self
    }

    #[allow(dead_code)]
    pub fn with_logs(mut self, logs: Arc<crate::logging::LogBuffer>) -> Self {
        self.logs = Some(logs);
        self
    }
}
