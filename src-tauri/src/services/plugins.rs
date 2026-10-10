use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;

use crate::credentials::SecretStore;
use crate::domain::{Modality, ModelInput, Provider, ProviderInput, ProviderType};
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginModelTemplate {
    pub id: String,
    pub name: String,
    pub model_id: String,
    pub modality: Modality,
    pub capabilities: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub protocol: String,
    pub default_base_url: String,
    pub default_headers: BTreeMap<String, String>,
    pub recommended_models: Vec<PluginModelTemplate>,
}

pub fn list(data_dir: &Path) -> Result<Vec<ProviderPlugin>, AppError> {
    let dir = data_dir.join("plugins");
    if !dir.exists() {
        return Ok(default_plugins());
    }
    let mut plugins = Vec::new();
    let entries = std::fs::read_dir(&dir)
        .map_err(|e| AppError::validation(format!("无法读取插件目录: {e}")))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(plugin) = serde_json::from_slice::<ProviderPlugin>(&bytes) {
                    plugins.push(plugin);
                }
            }
        }
    }
    if plugins.is_empty() {
        plugins = default_plugins();
    }
    Ok(plugins)
}

fn validate_plugin_id(id: &str) -> Result<&str, AppError> {
    let trimmed = id.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err(AppError::validation("插件 ID 长度必须在 1 到 64 之间"));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(AppError::validation(
            "插件 ID 只能包含字母、数字、下划线和连字符",
        ));
    }
    Ok(trimmed)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginValidationReport {
    pub valid: bool,
    pub plugin_id: Option<String>,
    pub plugin_name: Option<String>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub security_passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestAdapterResult {
    pub success: bool,
    pub extracted_value: Option<String>,
    pub resolved_status: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkTemplateInfo {
    pub schema_version: String,
    pub manifest_template: String,
    pub generic_http_template: String,
    pub documentation: String,
    pub supported_protocols: Vec<String>,
}

pub fn validate_manifest(manifest_json: &str) -> PluginValidationReport {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();
    let mut security_passed = true;

    let parsed_val: Result<Value, _> = serde_json::from_str(manifest_json);
    let Ok(val) = parsed_val else {
        return PluginValidationReport {
            valid: false,
            plugin_id: None,
            plugin_name: None,
            warnings: vec![],
            errors: vec!["JSON 语法格式不合法".to_string()],
            security_passed: false,
        };
    };

    let plugin_id = val.get("id").and_then(Value::as_str).map(|s| s.to_string());
    let plugin_name = val
        .get("name")
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    if let Some(ref id) = plugin_id {
        if let Err(e) = validate_plugin_id(id) {
            errors.push(e.message.clone());
            security_passed = false;
        }
    } else {
        errors.push("缺少插件 ID (id)".to_string());
    }

    if let Some(ref name) = plugin_name {
        if name.trim().is_empty() {
            errors.push("插件名称不能为空".to_string());
        }
    } else {
        errors.push("缺少插件名称 (name)".to_string());
    }

    let protocol = val.get("protocol").and_then(Value::as_str).unwrap_or("");
    let supported_protocols = ["openai-compatible", "anthropic", "gemini", "generic-http"];
    if !supported_protocols.contains(&protocol) {
        errors.push(format!(
            "不受支持的协议: '{protocol}'。仅支持 {:?}",
            supported_protocols
        ));
    }

    if let Some(base_url) = val.get("defaultBaseUrl").and_then(Value::as_str) {
        let trimmed_url = base_url.trim().to_lowercase();
        if !trimmed_url.starts_with("http://") && !trimmed_url.starts_with("https://") {
            errors.push("Base URL 必须是以 http:// 或 https:// 开头的网络地址，禁止使用本地文件或其他危险协议".to_string());
            security_passed = false;
        }
        if trimmed_url.contains("..") {
            errors.push("Base URL 中禁止包含路径穿越字符 '..' ".to_string());
            security_passed = false;
        }
    } else {
        warnings.push("未指定 defaultBaseUrl".to_string());
    }

    if let Some(headers) = val.get("defaultHeaders").and_then(Value::as_object) {
        for (k, v) in headers {
            if let Some(v_str) = v.as_str() {
                let v_lower = v_str.to_lowercase();
                if (k.to_lowercase() == "authorization" || k.to_lowercase() == "x-api-key")
                    && !v_lower.contains("{{")
                    && (v_lower.contains("sk-") || v_str.len() > 24)
                {
                    warnings.push(format!(
                        "请求头 '{}' 中可能包含硬编码凭据，建议通过 Workbench 密钥管理器安全注入",
                        k
                    ));
                }
            }
        }
    }

    if let Some(models) = val.get("recommendedModels").and_then(Value::as_array) {
        for (idx, m) in models.iter().enumerate() {
            if m.get("modelId").and_then(Value::as_str).is_none() {
                errors.push(format!("模型列表第 {} 项缺少 modelId", idx + 1));
            }
        }
    } else {
        warnings.push("建议在 recommendedModels 中推荐至少一个默认模型".to_string());
    }

    let valid = errors.is_empty();
    PluginValidationReport {
        valid,
        plugin_id,
        plugin_name,
        warnings,
        errors,
        security_passed: security_passed && valid,
    }
}

pub fn install(data_dir: &Path, manifest_json: &str) -> Result<ProviderPlugin, AppError> {
    let report = validate_manifest(manifest_json);
    if !report.valid || !report.security_passed {
        let msg = report.errors.join("; ");
        return Err(AppError::validation(format!("插件安全校验未通过: {msg}")));
    }
    let plugin: ProviderPlugin = serde_json::from_str(manifest_json)
        .map_err(|e| AppError::validation(format!("插件格式错误: {e}")))?;
    let safe_id = validate_plugin_id(&plugin.id)?;
    let dir = data_dir.join("plugins");
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::validation(format!("创建插件目录失败: {e}")))?;
    let target = dir.join(format!("{safe_id}.json"));
    std::fs::write(&target, manifest_json.as_bytes())
        .map_err(|e| AppError::validation(format!("保存插件文件失败: {e}")))?;
    Ok(plugin)
}

pub fn uninstall(data_dir: &Path, plugin_id: &str) -> Result<(), AppError> {
    let safe_id = validate_plugin_id(plugin_id)?;
    let file = data_dir.join("plugins").join(format!("{safe_id}.json"));
    if file.exists() {
        std::fs::remove_file(file)
            .map_err(|e| AppError::validation(format!("删除插件失败: {e}")))?;
    }
    Ok(())
}

fn at_eval<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let clean_path = path.trim();
    if clean_path.starts_with('/') {
        return value.pointer(clean_path);
    }
    let mut current = value;
    let mut rest = clean_path.strip_prefix('$').unwrap_or(clean_path);
    if rest.is_empty() {
        return Some(current);
    }
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix('.') {
            let end = stripped.find(['.', '[']).unwrap_or(stripped.len());
            if end == 0 {
                return None;
            }
            current = current.get(&stripped[..end])?;
            rest = &stripped[end..];
        } else if let Some(stripped) = rest.strip_prefix('[') {
            let end = stripped.find(']')?;
            let index: usize = stripped[..end].parse().ok()?;
            current = current.get(index)?;
            rest = &stripped[end + 1..];
        } else {
            let end = rest.find(['.', '[']).unwrap_or(rest.len());
            current = current.get(&rest[..end])?;
            rest = &rest[end..];
        }
    }
    Some(current)
}

pub fn test_recipe(
    recipe: &crate::domain::HttpRecipe,
    sample_response: &str,
) -> Result<TestAdapterResult, AppError> {
    let parsed: Value = serde_json::from_str(sample_response)
        .map_err(|e| AppError::validation(format!("测试响应 JSON 格式错误: {e}")))?;

    let mut resolved_status = None;
    if let Some(ref poll) = recipe.poll {
        if let Some(val) = at_eval(&parsed, &poll.status_path) {
            let status_str = match val {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => val.to_string(),
            };
            resolved_status = Some(status_str);
        }
    }

    let extracted = at_eval(&parsed, &recipe.result_path);
    match extracted {
        Some(val) => {
            let text = match val {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => val.to_string(),
            };
            Ok(TestAdapterResult {
                success: true,
                extracted_value: Some(text),
                resolved_status,
                error: None,
            })
        }
        None => Ok(TestAdapterResult {
            success: false,
            extracted_value: None,
            resolved_status,
            error: Some(format!(
                "未在响应中通过路径 '{}' 找到匹配字段",
                recipe.result_path
            )),
        }),
    }
}

pub fn get_sdk_template() -> SdkTemplateInfo {
    let manifest_template = r#"{
  "id": "my-custom-provider",
  "name": "My Custom Provider",
  "version": "1.0.0",
  "description": "自定义第三方 AI 推理服务插件，支持文本/绘图等能力",
  "protocol": "openai-compatible",
  "defaultBaseUrl": "https://api.example.com/v1",
  "defaultHeaders": {
    "User-Agent": "AI-Workbench-Plugin/1.0"
  },
  "recommendedModels": [
    {
      "id": "custom-model-chat",
      "name": "Custom Model (Chat)",
      "modelId": "custom-v1",
      "modality": "text",
      "capabilities": {
        "streaming": true,
        "tools": false
      }
    }
  ]
}"#;

    let generic_http_template = r#"{
  "method": "POST",
  "path": "/v1/images/generations",
  "headers": [
    { "name": "Content-Type", "value": "application/json" }
  ],
  "body": {
    "prompt": "{{prompt}}",
    "negative_prompt": "{{negativePrompt}}",
    "width": 1024,
    "height": 1024
  },
  "resultPath": "$.data[0].url",
  "resultKind": "url"
}"#;

    let documentation = r#"# AI Workbench Plugin & Adapter SDK

## 1. 插件清单 (Manifest) 规范
第三方可通过创建 `JSON` 清单接入自定义 Provider：
- `id`: 唯一标识符（只允许小写英文字母、数字、下划线与中划线）。
- `name`: 显示名称。
- `version`: 语义化版本号。
- `protocol`: 协议类型，支持：
  - `openai-compatible`: 标准 OpenAI 兼容接口（含 Chat, Embeddings, TTS 等）。
  - `anthropic`: Anthropic Messages 原生格式。
  - `gemini`: Google Gemini 原生格式。
  - `generic-http`: 自定义声明式 HTTP 适配器。
- `defaultBaseUrl`: 必须以 `http://` 或 `https://` 开头。
- `recommendedModels`: 预置推荐的模型列表。

## 2. 声明式适配器 (Generic HTTP Recipe)
针对非标接口，可通过 `HttpRecipe` 进行参数填充与响应提取：
- `resultPath`: 支持 JSONPath 或 Pointer (例如 `$.data[0].url` 或 `/choices/0/text`)。
- `body`: 支持模板占位符，如 `{{prompt}}`, `{{negativePrompt}}`, `{{firstFrame}}`, `{{model}}`。

## 3. 安全沙箱保障
- 插件仅限于声明式网络交互，禁止执行任何操作系统本地 Shell / 二进制指令。
- 绝不泄露宿主密钥；凭据统一由 Workbench 内存中 SecretStore 注入，禁止导出或文件明文留存。
"#;

    SdkTemplateInfo {
        schema_version: "1.0.0".to_string(),
        manifest_template: manifest_template.to_string(),
        generic_http_template: generic_http_template.to_string(),
        documentation: documentation.to_string(),
        supported_protocols: vec![
            "openai-compatible".to_string(),
            "anthropic".to_string(),
            "gemini".to_string(),
            "generic-http".to_string(),
        ],
    }
}

pub async fn instantiate(
    pool: &SqlitePool,
    secrets: &dyn SecretStore,
    data_dir: &Path,
    plugin_id: &str,
    api_key: Option<String>,
) -> Result<Provider, AppError> {
    let plugins = list(data_dir)?;
    let plugin = plugins
        .into_iter()
        .find(|p| p.id == plugin_id)
        .ok_or_else(|| AppError::not_found("未找到对应的 Provider 插件"))?;

    let p_type = match plugin.protocol.as_str() {
        "anthropic" => ProviderType::Anthropic,
        "gemini" => ProviderType::Gemini,
        "generic-http" => ProviderType::GenericHttp,
        _ => ProviderType::OpenaiCompatible,
    };

    let p_input = ProviderInput {
        name: plugin.name.clone(),
        provider_type: p_type,
        base_url: plugin.default_base_url.clone(),
        api_key: api_key.filter(|s| !s.trim().is_empty()),
        clear_api_key: false,
        headers: plugin.default_headers.clone(),
        timeout_ms: 120_000,
        enabled: true,
    };

    let record = crate::services::providers::create(pool, secrets, p_input).await?;

    let mut created_models = Vec::new();
    for m in plugin.recommended_models {
        match crate::services::models::create(
            pool,
            ModelInput {
                name: m.name,
                provider_id: record.id.clone(),
                model_id: m.model_id,
                modality: m.modality,
                capabilities: m.capabilities,
                parameters: vec![],
                constraints: vec![],
                transport: None,
                enabled: true,
            },
        )
        .await
        {
            Ok(model) => created_models.push(model.id),
            Err(err) => {
                // Instantiation is one user action. Do not leave a provider
                // with only a subset of its advertised model set.
                for model_id in created_models {
                    let _ = crate::services::models::delete(pool, &model_id).await;
                }
                if let Err(cleanup_err) =
                    crate::services::providers::delete(pool, secrets, &record.id).await
                {
                    return Err(AppError::storage(format!(
                        "插件模型初始化失败，且回滚 Provider 失败: {}",
                        cleanup_err.message
                    )));
                }
                return Err(AppError::storage(format!(
                    "插件模型初始化失败，已回滚 Provider: {}",
                    err.message
                )));
            }
        }
    }

    Ok(record)
}

fn default_plugins() -> Vec<ProviderPlugin> {
    vec![
        ProviderPlugin {
            id: "groq".to_string(),
            name: "Groq Cloud".to_string(),
            version: "1.0.0".to_string(),
            description: "超高速 LPU 推理云，支持 Llama 3 与 Whisper".to_string(),
            protocol: "openai-compatible".to_string(),
            default_base_url: "https://api.groq.com/openai/v1".to_string(),
            default_headers: BTreeMap::new(),
            recommended_models: vec![
                PluginModelTemplate {
                    id: "groq-llama-3-3-70b".to_string(),
                    name: "Llama 3.3 70B (Groq)".to_string(),
                    model_id: "llama-3.3-70b-versatile".to_string(),
                    modality: Modality::Text,
                    capabilities: serde_json::json!({ "streaming": true, "tools": true }),
                },
                PluginModelTemplate {
                    id: "groq-whisper".to_string(),
                    name: "Whisper Large v3 (Groq)".to_string(),
                    model_id: "whisper-large-v3".to_string(),
                    modality: Modality::Audio,
                    capabilities: serde_json::json!({ "speechToText": true }),
                },
            ],
        },
        ProviderPlugin {
            id: "deepseek".to_string(),
            name: "DeepSeek 官方".to_string(),
            version: "1.0.0".to_string(),
            description: "DeepSeek 官方推理接口，支持 V3 与 R1 深度思考".to_string(),
            protocol: "openai-compatible".to_string(),
            default_base_url: "https://api.deepseek.com/v1".to_string(),
            default_headers: BTreeMap::new(),
            recommended_models: vec![
                PluginModelTemplate {
                    id: "deepseek-reasoner".to_string(),
                    name: "DeepSeek R1".to_string(),
                    model_id: "deepseek-reasoner".to_string(),
                    modality: Modality::Text,
                    capabilities: serde_json::json!({ "streaming": true, "thinking": true }),
                },
                PluginModelTemplate {
                    id: "deepseek-chat".to_string(),
                    name: "DeepSeek V3".to_string(),
                    model_id: "deepseek-chat".to_string(),
                    modality: Modality::Text,
                    capabilities: serde_json::json!({ "streaming": true, "tools": true }),
                },
            ],
        },
    ]
}
