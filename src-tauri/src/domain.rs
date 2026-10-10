use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderType {
    Openai,
    OpenaiCompatible,
    Ollama,
    Comfyui,
    GenericHttp,
    Anthropic,
    Gemini,
    Replicate,
    Fal,
}

impl ProviderType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::OpenaiCompatible => "openai-compatible",
            Self::Ollama => "ollama",
            Self::Comfyui => "comfyui",
            Self::GenericHttp => "generic-http",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::Replicate => "replicate",
            Self::Fal => "fal",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "openai" => Some(Self::Openai),
            "openai-compatible" => Some(Self::OpenaiCompatible),
            "ollama" => Some(Self::Ollama),
            "comfyui" => Some(Self::Comfyui),
            "generic-http" => Some(Self::GenericHttp),
            "anthropic" => Some(Self::Anthropic),
            "gemini" => Some(Self::Gemini),
            "replicate" => Some(Self::Replicate),
            "fal" => Some(Self::Fal),
            _ => None,
        }
    }

    pub fn requires_api_key(self) -> bool {
        matches!(
            self,
            Self::Openai | Self::Anthropic | Self::Gemini | Self::Replicate | Self::Fal
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modality {
    Text,
    Image,
    Video,
    Audio,
}

impl Modality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "text" => Some(Self::Text),
            "image" => Some(Self::Image),
            "video" => Some(Self::Video),
            "audio" => Some(Self::Audio),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParameterType {
    Boolean,
    Number,
    Range,
    Select,
    Text,
    Textarea,
    Image,
    Images,
    Video,
    Videos,
    Audio,
    Audios,
}

impl ParameterType {
    pub fn is_media(self) -> bool {
        matches!(
            self,
            Self::Image | Self::Images | Self::Video | Self::Videos | Self::Audio | Self::Audios
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SelectOption {
    pub label: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ParameterDefinition {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: ParameterType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<SelectOption>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advanced: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ConstraintDefinition {
    MutuallyExclusive {
        fields: Vec<String>,
    },
    Requires {
        field: String,
        required_fields: Vec<String>,
    },
    ConditionalVisibility {
        field: String,
        depends_on: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        equals: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        not_equals: Option<Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowBinding {
    pub key: String,
    pub node: String,
    pub input: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct TransportDefinition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_edit_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speech_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bindings: Option<Vec<WorkflowBinding>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpRecipe>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HeaderTemplate {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HttpPoll {
    pub method: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    pub status_path: String,
    pub success: String,
    pub failure: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HttpRecipe {
    pub method: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<HeaderTemplate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll: Option<HttpPoll>,
    pub result_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_kind: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProviderRecord {
    pub id: String,
    pub name: String,
    pub provider_type: ProviderType,
    pub base_url: String,
    pub credential_id: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub timeout_ms: u64,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    pub base_url: String,
    pub has_credential: bool,
    pub headers: BTreeMap<String, String>,
    pub timeout_ms: u64,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl ProviderRecord {
    pub fn to_dto(&self) -> Provider {
        Provider {
            id: self.id.clone(),
            name: self.name.clone(),
            provider_type: self.provider_type,
            base_url: self.base_url.clone(),
            has_credential: self.credential_id.is_some(),
            headers: self.headers.clone(),
            timeout_ms: self.timeout_ms,
            enabled: self.enabled,
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
        }
    }
}

pub fn default_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelDefinition {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub model_id: String,
    pub modality: Modality,
    pub capabilities: Value,
    pub parameters: Vec<ParameterDefinition>,
    pub constraints: Vec<ConstraintDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<TransportDefinition>,
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    pub name: String,
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub clear_api_key: bool,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub timeout_ms: u64,
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInput {
    pub name: String,
    pub provider_id: String,
    pub model_id: String,
    pub modality: Modality,
    pub capabilities: Value,
    #[serde(default)]
    pub parameters: Vec<ParameterDefinition>,
    #[serde(default)]
    pub constraints: Vec<ConstraintDefinition>,
    #[serde(default)]
    pub transport: Option<TransportDefinition>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct MediaRef {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

impl MediaRef {
    pub fn from_path(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            role: None,
            weight: None,
            description: None,
            enabled: Some(true),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceImage {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateImageInput {
    pub model_definition_id: String,
    pub prompt: String,
    #[serde(default)]
    pub negative_prompt: Option<String>,
    #[serde(default)]
    pub source_image: Option<MediaRef>,
    #[serde(default)]
    pub reference_images: Vec<ReferenceImage>,
    #[serde(default)]
    pub mask_base64: Option<String>,
    #[serde(default)]
    pub mask_path: Option<String>,
    #[serde(default)]
    pub parameters: Map<String, Value>,
    #[serde(default)]
    pub extra: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateAudioInput {
    pub model_definition_id: String,
    #[serde(default)]
    pub task: Option<String>,
    pub text: String,
    #[serde(default)]
    pub voice: Option<String>,
    #[serde(default)]
    pub source_audio: Option<MediaRef>,
    #[serde(default)]
    pub reference_audio: Option<MediaRef>,
    #[serde(default)]
    pub reference_transcript: Option<String>,
    #[serde(default)]
    pub parameters: Map<String, Value>,
    #[serde(default)]
    pub extra: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
    pub id: String,
    pub name: String,
    pub file_path: String,
    pub transcript: String,
    pub tags: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub reference_files: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceInput {
    pub name: String,
    pub source_path: String,
    #[serde(default)]
    pub transcript: String,
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub reference_paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceUpdate {
    pub name: String,
    #[serde(default)]
    pub transcript: String,
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub reference_paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateVideoInput {
    pub model_definition_id: String,
    pub prompt: String,
    #[serde(default)]
    pub negative_prompt: Option<String>,
    #[serde(default)]
    pub first_frame: Option<MediaRef>,
    #[serde(default)]
    pub last_frame: Option<MediaRef>,
    #[serde(default)]
    pub reference_images: Vec<ReferenceImage>,
    #[serde(default)]
    pub reference_videos: Vec<MediaRef>,
    #[serde(default)]
    pub reference_audios: Vec<MediaRef>,
    #[serde(default)]
    pub parameters: Map<String, Value>,
    #[serde(default)]
    pub extra: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextInput {
    pub model_definition_id: String,
    #[serde(default)]
    pub system_prompt: Option<String>,
    pub prompt: String,
    #[serde(default)]
    pub parameters: Map<String, Value>,
    #[serde(default)]
    pub extra: Option<Value>,
    #[serde(default)]
    pub images: Vec<MediaRef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSendInput {
    #[serde(default)]
    pub conversation_id: Option<String>,
    pub model_definition_id: String,
    #[serde(default)]
    pub system_prompt: Option<String>,
    pub context_strategy: String,
    #[serde(default)]
    pub context_limit: u32,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub images: Vec<MediaRef>,
    #[serde(default)]
    pub parameters: Map<String, Value>,
    #[serde(default)]
    pub extra: Option<Value>,
    #[serde(default)]
    pub replace_from_message_id: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub confirmed_tools: Vec<String>,
    #[serde(default)]
    pub max_steps: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationUpdate {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub model_definition_id: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub context_strategy: Option<String>,
    #[serde(default)]
    pub context_limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_template: Option<String>,
    pub parameters: Map<String, Value>,
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetInput {
    pub name: String,
    pub model_id: String,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub user_template: Option<String>,
    #[serde(default)]
    pub parameters: Map<String, Value>,
}

pub fn now_iso() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

pub fn capability_enabled(capabilities: &Value, key: &str) -> bool {
    match capabilities.get(key) {
        Some(Value::Bool(enabled)) => *enabled,
        Some(Value::Object(map)) => map.get("enabled").and_then(Value::as_bool).unwrap_or(false),
        _ => false,
    }
}

pub fn capability_limit(capabilities: &Value, key: &str, fallback: usize) -> usize {
    capabilities
        .get(key)
        .and_then(|value| value.get("max"))
        .and_then(Value::as_u64)
        .and_then(|max| usize::try_from(max).ok())
        .unwrap_or(fallback)
}
