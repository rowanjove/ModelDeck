use std::collections::BTreeMap;
use std::time::Instant;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::db;
use crate::domain::{
    now_iso, Modality, ModelDefinition, ParameterDefinition, ParameterType, Provider,
    ProviderRecord, ProviderType,
};
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredService {
    pub id: String,
    pub name: String,
    pub service_type: String,
    pub base_url: String,
    pub port: u16,
    pub online: bool,
    pub latency_ms: Option<u64>,
    pub already_added: bool,
    pub details: Option<String>,
    pub version: Option<String>,
    pub detected_models: Vec<String>,
    pub web_ui_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProbeTarget {
    pub id: &'static str,
    pub name: &'static str,
    pub service_type: &'static str,
    pub base_url: &'static str,
    pub probe_url: &'static str,
    pub port: u16,
    pub web_ui_url: &'static str,
}

pub const TARGETS: &[ProbeTarget] = &[
    ProbeTarget {
        id: "ollama",
        name: "Ollama (本地)",
        service_type: "ollama",
        base_url: "http://127.0.0.1:11434",
        probe_url: "http://127.0.0.1:11434/api/tags",
        port: 11434,
        web_ui_url: "http://127.0.0.1:11434",
    },
    ProbeTarget {
        id: "lmstudio",
        name: "LM Studio (本地)",
        service_type: "openai-compatible",
        base_url: "http://127.0.0.1:1234/v1",
        probe_url: "http://127.0.0.1:1234/v1/models",
        port: 1234,
        web_ui_url: "http://127.0.0.1:1234",
    },
    ProbeTarget {
        id: "llamacpp",
        name: "llama.cpp (本地)",
        service_type: "openai-compatible",
        base_url: "http://127.0.0.1:8080/v1",
        probe_url: "http://127.0.0.1:8080/v1/models",
        port: 8080,
        web_ui_url: "http://127.0.0.1:8080",
    },
    ProbeTarget {
        id: "localai",
        name: "LocalAI / vLLM (8000端口)",
        service_type: "openai-compatible",
        base_url: "http://127.0.0.1:8000/v1",
        probe_url: "http://127.0.0.1:8000/v1/models",
        port: 8000,
        web_ui_url: "http://127.0.0.1:8000",
    },
    ProbeTarget {
        id: "comfyui",
        name: "ComfyUI (本地)",
        service_type: "comfyui",
        base_url: "http://127.0.0.1:8188",
        probe_url: "http://127.0.0.1:8188/system_stats",
        port: 8188,
        web_ui_url: "http://127.0.0.1:8188",
    },
    ProbeTarget {
        id: "f5tts",
        name: "F5-TTS (本地语音)",
        service_type: "generic-http",
        base_url: "http://127.0.0.1:9998",
        probe_url: "http://127.0.0.1:9998/health",
        port: 9998,
        web_ui_url: "http://127.0.0.1:9998",
    },
    ProbeTarget {
        id: "cosyvoice",
        name: "CosyVoice (本地语音)",
        service_type: "generic-http",
        base_url: "http://127.0.0.1:50000",
        probe_url: "http://127.0.0.1:50000/api/v1/health",
        port: 50000,
        web_ui_url: "http://127.0.0.1:50000",
    },
];

pub async fn discover_local_services(
    pool: &SqlitePool,
    client: &Client,
) -> Result<Vec<DiscoveredService>, AppError> {
    let existing_providers = db::list_providers(pool).await?;
    let mut results = Vec::new();

    for target in TARGETS {
        let port_needle = format!(":{}", target.port);
        let already_added = existing_providers
            .iter()
            .any(|p| p.base_url.contains(&port_needle) || p.base_url.starts_with(target.base_url));

        let start = Instant::now();
        let probe_resp = client
            .get(target.probe_url)
            .timeout(std::time::Duration::from_millis(600))
            .send()
            .await;

        let (online, latency_ms, details, version, detected_models) = match probe_resp {
            Ok(res) => {
                let ms = start.elapsed().as_millis() as u64;
                let status = res.status();
                if status.is_success() || status.as_u16() == 404 || status.as_u16() == 401 {
                    let mut ver = None;
                    let mut models = Vec::new();

                    if status.is_success() {
                        if let Ok(json_val) = res.json::<serde_json::Value>().await {
                            // 1. Extract models
                            if let Some(arr) = json_val.get("models").and_then(|v| v.as_array()) {
                                for item in arr {
                                    if let Some(name) = item.get("name").and_then(|n| n.as_str()) {
                                        models.push(name.to_string());
                                    }
                                }
                            } else if let Some(arr) =
                                json_val.get("data").and_then(|v| v.as_array())
                            {
                                for item in arr {
                                    if let Some(id) = item.get("id").and_then(|i| i.as_str()) {
                                        models.push(id.to_string());
                                    }
                                }
                            }

                            // 2. Extract version
                            if let Some(v_str) = json_val.get("version").and_then(|v| v.as_str()) {
                                ver = Some(v_str.to_string());
                            } else if let Some(sys) =
                                json_val.get("system").and_then(|v| v.as_object())
                            {
                                if let Some(py) = sys.get("python_version").and_then(|v| v.as_str())
                                {
                                    ver = Some(format!("Python {}", py));
                                }
                            }
                        }
                    }

                    // For Ollama, if version not extracted, try a quick /api/version
                    if target.id == "ollama" && ver.is_none() {
                        if let Ok(v_resp) = client
                            .get("http://127.0.0.1:11434/api/version")
                            .timeout(std::time::Duration::from_millis(300))
                            .send()
                            .await
                        {
                            if let Ok(v_json) = v_resp.json::<serde_json::Value>().await {
                                if let Some(v_str) = v_json.get("version").and_then(|v| v.as_str())
                                {
                                    ver = Some(v_str.to_string());
                                }
                            }
                        }
                    }

                    (
                        true,
                        Some(ms),
                        Some(format!("HTTP {}", status.as_u16())),
                        ver,
                        models,
                    )
                } else {
                    (
                        false,
                        None,
                        Some(format!("HTTP {}", status.as_u16())),
                        None,
                        Vec::new(),
                    )
                }
            }
            Err(e) => (false, None, Some(e.to_string()), None, Vec::new()),
        };

        results.push(DiscoveredService {
            id: target.id.to_string(),
            name: target.name.to_string(),
            service_type: target.service_type.to_string(),
            base_url: target.base_url.to_string(),
            port: target.port,
            online,
            latency_ms,
            already_added,
            details,
            version,
            detected_models,
            web_ui_url: Some(target.web_ui_url.to_string()),
        });
    }

    Ok(results)
}

pub async fn add_discovered_service(
    pool: &SqlitePool,
    service_id: &str,
) -> Result<Provider, AppError> {
    let target = TARGETS
        .iter()
        .find(|t| t.id == service_id)
        .ok_or_else(|| AppError::not_found(format!("未知服务类型: {}", service_id)))?;

    let normalized = target.service_type.replace('_', "-");
    let p_type: ProviderType = ProviderType::from_db(&normalized)
        .or_else(|| serde_json::from_value(serde_json::Value::String(normalized.clone())).ok())
        .ok_or_else(|| {
            AppError::validation(format!("无效Provider类型: {}", target.service_type))
        })?;

    let provider_id = Uuid::new_v4().to_string();
    let now = now_iso();

    let record = ProviderRecord {
        id: provider_id.clone(),
        name: target.name.to_string(),
        provider_type: p_type,
        base_url: target.base_url.to_string(),
        credential_id: None,
        headers: BTreeMap::new(),
        timeout_ms: 30000,
        enabled: true,
        created_at: now.clone(),
        updated_at: now.clone(),
    };

    db::insert_provider(pool, &record).await?;

    // Attempt to probe detected models from the running local service
    let client = Client::new();
    let mut detected = Vec::new();
    if let Ok(res) = client
        .get(target.probe_url)
        .timeout(std::time::Duration::from_millis(1000))
        .send()
        .await
    {
        if res.status().is_success() {
            if let Ok(json_val) = res.json::<serde_json::Value>().await {
                if let Some(arr) = json_val.get("models").and_then(|v| v.as_array()) {
                    for item in arr {
                        if let Some(name) = item.get("name").and_then(|n| n.as_str()) {
                            detected.push(name.to_string());
                        }
                    }
                } else if let Some(arr) = json_val.get("data").and_then(|v| v.as_array()) {
                    for item in arr {
                        if let Some(id) = item.get("id").and_then(|i| i.as_str()) {
                            detected.push(id.to_string());
                        }
                    }
                }
            }
        }
    }

    if !detected.is_empty() {
        for m_name in detected {
            let (modality, caps) = match target.id {
                "comfyui" => (
                    Modality::Image,
                    serde_json::json!({ "image_generate": true, "image_edit": true }),
                ),
                "f5tts" | "cosyvoice" => (Modality::Audio, serde_json::json!({ "tts": true })),
                _ => (
                    Modality::Text,
                    serde_json::json!({
                        "streaming": true,
                        "chat": true,
                        "text_generate": true,
                    }),
                ),
            };

            let model_record = ModelDefinition {
                id: Uuid::new_v4().to_string(),
                provider_id: provider_id.clone(),
                name: format!("{} ({})", m_name, target.name),
                model_id: m_name.clone(),
                modality,
                capabilities: caps,
                parameters: vec![ParameterDefinition {
                    key: "temperature".to_string(),
                    label: "Temperature".to_string(),
                    kind: ParameterType::Number,
                    default: Some(serde_json::json!(0.7)),
                    min: Some(0.0),
                    max: Some(2.0),
                    step: Some(0.1),
                    options: None,
                    required: Some(false),
                    advanced: Some(false),
                    group: None,
                }],
                constraints: vec![],
                transport: None,
                schema_version: 1,
                enabled: true,
                created_at: now.clone(),
                updated_at: now.clone(),
            };
            let _ = db::insert_model(pool, &model_record).await;
        }
    } else {
        // Fallback to recommended default model for this provider
        let (model_id_str, model_name, modality, caps) = match target.id {
            "ollama" => (
                "llama3",
                "Llama 3 (Ollama)",
                Modality::Text,
                serde_json::json!({ "streaming": true, "chat": true, "text_generate": true }),
            ),
            "lmstudio" => (
                "local-model",
                "LM Studio Model",
                Modality::Text,
                serde_json::json!({ "streaming": true, "chat": true, "text_generate": true }),
            ),
            "llamacpp" => (
                "local-gguf",
                "llama.cpp Model",
                Modality::Text,
                serde_json::json!({ "streaming": true, "chat": true, "text_generate": true }),
            ),
            "localai" => (
                "default-local",
                "LocalAI Model",
                Modality::Text,
                serde_json::json!({ "streaming": true, "chat": true, "text_generate": true }),
            ),
            "comfyui" => (
                "sdxl-base",
                "SDXL Workflow (ComfyUI)",
                Modality::Image,
                serde_json::json!({ "image_generate": true, "image_edit": true }),
            ),
            "f5tts" => (
                "f5-tts-base",
                "F5-TTS Voice Model",
                Modality::Audio,
                serde_json::json!({ "tts": true }),
            ),
            "cosyvoice" => (
                "cosyvoice-base",
                "CosyVoice Model",
                Modality::Audio,
                serde_json::json!({ "tts": true }),
            ),
            _ => (
                "default-local",
                "Local Default Model",
                Modality::Text,
                serde_json::json!({ "streaming": true, "chat": true, "text_generate": true }),
            ),
        };

        let model_record = ModelDefinition {
            id: Uuid::new_v4().to_string(),
            provider_id: provider_id.clone(),
            name: model_name.to_string(),
            model_id: model_id_str.to_string(),
            modality,
            capabilities: caps,
            parameters: vec![ParameterDefinition {
                key: "temperature".to_string(),
                label: "Temperature".to_string(),
                kind: ParameterType::Number,
                default: Some(serde_json::json!(0.7)),
                min: Some(0.0),
                max: Some(2.0),
                step: Some(0.1),
                options: None,
                required: Some(false),
                advanced: Some(false),
                group: None,
            }],
            constraints: vec![],
            transport: None,
            schema_version: 1,
            enabled: true,
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        let _ = db::insert_model(pool, &model_record).await;
    }

    Ok(record.to_dto())
}
