use std::path::Path;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::db;
use crate::domain::{
    ConstraintDefinition, Modality, ModelDefinition, ModelInput, ParameterDefinition,
    ParameterType, ProviderType, SelectOption, TransportDefinition, WorkflowBinding,
};
use crate::error::AppError;

const DEFAULT_REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/ai-workbench/registry/main/model-registry.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryProviderTemplate {
    pub id: String,
    pub name: String,
    pub provider_type: ProviderType,
    pub default_base_url: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryModelTemplate {
    pub id: String,
    pub name: String,
    pub provider_type: ProviderType,
    pub model_id: String,
    pub modality: Modality,
    pub capabilities: Value,
    #[serde(default)]
    pub parameters: Vec<ParameterDefinition>,
    #[serde(default)]
    pub constraints: Vec<ConstraintDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<TransportDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_defaults: Option<serde_json::Map<String, Value>>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryCatalog {
    pub version: String,
    pub updated_at: String,
    pub source_url: Option<String>,
    pub providers: Vec<RegistryProviderTemplate>,
    pub models: Vec<RegistryModelTemplate>,
    #[serde(default)]
    pub can_rollback: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryUpdateCheck {
    pub current_version: String,
    pub latest_version: String,
    pub remote_version: String,
    pub has_update: bool,
    pub remote_model_count: usize,
    pub release_notes: Option<String>,
}

pub fn default_catalog() -> RegistryCatalog {
    RegistryCatalog {
        version: "2026.10.1".to_string(),
        updated_at: "2026-10-01T00:00:00Z".to_string(),
        source_url: Some(DEFAULT_REGISTRY_URL.to_string()),
        can_rollback: false,
        backup_version: None,
        providers: vec![
            RegistryProviderTemplate {
                id: "openai".to_string(),
                name: "OpenAI".to_string(),
                provider_type: ProviderType::Openai,
                default_base_url: "https://api.openai.com/v1".to_string(),
                description: "OpenAI 官方云端接口（GPT / DALL-E / Whisper / TTS）".to_string(),
            },
            RegistryProviderTemplate {
                id: "anthropic".to_string(),
                name: "Anthropic".to_string(),
                provider_type: ProviderType::Anthropic,
                default_base_url: "https://api.anthropic.com".to_string(),
                description: "Claude 系列模型原生接口".to_string(),
            },
            RegistryProviderTemplate {
                id: "gemini".to_string(),
                name: "Google Gemini".to_string(),
                provider_type: ProviderType::Gemini,
                default_base_url: "https://generativelanguage.googleapis.com".to_string(),
                description: "Google 官方 Gemini 系列多模态接口".to_string(),
            },
            RegistryProviderTemplate {
                id: "deepseek".to_string(),
                name: "DeepSeek".to_string(),
                provider_type: ProviderType::OpenaiCompatible,
                default_base_url: "https://api.deepseek.com".to_string(),
                description: "DeepSeek 官方 API（V3 / R1 推理）".to_string(),
            },
            RegistryProviderTemplate {
                id: "ollama".to_string(),
                name: "Ollama 本地服务".to_string(),
                provider_type: ProviderType::Ollama,
                default_base_url: "http://127.0.0.1:11434".to_string(),
                description: "本地免部署模型运行环境".to_string(),
            },
            RegistryProviderTemplate {
                id: "comfyui".to_string(),
                name: "ComfyUI 本地服务".to_string(),
                provider_type: ProviderType::Comfyui,
                default_base_url: "http://127.0.0.1:8188".to_string(),
                description: "基于节点图的高性能生图与生视频本地引擎".to_string(),
            },
        ],
        models: vec![
            RegistryModelTemplate {
                id: "gpt-4o".to_string(),
                name: "GPT-4o".to_string(),
                provider_type: ProviderType::Openai,
                model_id: "gpt-4o".to_string(),
                modality: Modality::Text,
                capabilities: json!({ "streaming": true, "tools": true, "vision": true }),
                parameters: vec![
                    ParameterDefinition {
                        key: "temperature".to_string(),
                        label: "采样温度".to_string(),
                        kind: ParameterType::Range,
                        default: Some(json!(0.7)),
                        min: Some(0.0),
                        max: Some(2.0),
                        step: Some(0.05),
                        options: None,
                        required: None,
                        advanced: None,
                        group: Some("采样控制".to_string()),
                    },
                    ParameterDefinition {
                        key: "max_tokens".to_string(),
                        label: "最大生成长度".to_string(),
                        kind: ParameterType::Number,
                        default: Some(json!(4096)),
                        min: Some(1.0),
                        max: Some(16384.0),
                        step: Some(64.0),
                        options: None,
                        required: None,
                        advanced: None,
                        group: Some("生成参数".to_string()),
                    },
                ],
                constraints: vec![],
                transport: None,
                recommended_defaults: None,
                description: "OpenAI 旗舰全能多模态模型".to_string(),
            },
            RegistryModelTemplate {
                id: "claude-3-7-sonnet".to_string(),
                name: "Claude 3.7 Sonnet".to_string(),
                provider_type: ProviderType::Anthropic,
                model_id: "claude-3-7-sonnet-20250219".to_string(),
                modality: Modality::Text,
                capabilities: json!({ "streaming": true, "tools": true, "vision": true, "thinking": true }),
                parameters: vec![
                    ParameterDefinition {
                        key: "temperature".to_string(),
                        label: "采样温度".to_string(),
                        kind: ParameterType::Range,
                        default: Some(json!(1.0)),
                        min: Some(0.0),
                        max: Some(1.0),
                        step: Some(0.05),
                        options: None,
                        required: None,
                        advanced: None,
                        group: Some("采样控制".to_string()),
                    },
                    ParameterDefinition {
                        key: "max_tokens".to_string(),
                        label: "最大 Token 数".to_string(),
                        kind: ParameterType::Number,
                        default: Some(json!(8192)),
                        min: Some(1.0),
                        max: Some(64000.0),
                        step: Some(128.0),
                        options: None,
                        required: None,
                        advanced: None,
                        group: Some("生成参数".to_string()),
                    },
                ],
                constraints: vec![],
                transport: None,
                recommended_defaults: None,
                description: "Anthropic 强混合推理旗舰模型".to_string(),
            },
            RegistryModelTemplate {
                id: "deepseek-r1".to_string(),
                name: "DeepSeek R1".to_string(),
                provider_type: ProviderType::OpenaiCompatible,
                model_id: "deepseek-reasoner".to_string(),
                modality: Modality::Text,
                capabilities: json!({ "streaming": true, "thinking": true }),
                parameters: vec![ParameterDefinition {
                    key: "temperature".to_string(),
                    label: "采样温度".to_string(),
                    kind: ParameterType::Range,
                    default: Some(json!(0.6)),
                    min: Some(0.0),
                    max: Some(2.0),
                    step: Some(0.05),
                    options: None,
                    required: None,
                    advanced: None,
                    group: Some("采样控制".to_string()),
                }],
                constraints: vec![],
                transport: None,
                recommended_defaults: None,
                description: "深度思考与数学/代码推理旗舰模型".to_string(),
            },
            RegistryModelTemplate {
                id: "gemini-2.5-pro".to_string(),
                name: "Gemini 2.5 Pro".to_string(),
                provider_type: ProviderType::Gemini,
                model_id: "gemini-2.5-pro".to_string(),
                modality: Modality::Text,
                capabilities: json!({ "streaming": true, "tools": true, "vision": true }),
                parameters: vec![],
                constraints: vec![],
                transport: None,
                recommended_defaults: None,
                description: "Google 百万级超长上下文多模态推理模型".to_string(),
            },
            RegistryModelTemplate {
                id: "dall-e-3".to_string(),
                name: "DALL-E 3".to_string(),
                provider_type: ProviderType::Openai,
                model_id: "dall-e-3".to_string(),
                modality: Modality::Image,
                capabilities: json!({ "generate": true }),
                parameters: vec![
                    ParameterDefinition {
                        key: "size".to_string(),
                        label: "分辨率".to_string(),
                        kind: ParameterType::Select,
                        default: Some(json!("1024x1024")),
                        min: None,
                        max: None,
                        step: None,
                        options: Some(vec![
                            SelectOption {
                                label: "1024x1024 正方形".to_string(),
                                value: json!("1024x1024"),
                            },
                            SelectOption {
                                label: "1024x1792 竖版".to_string(),
                                value: json!("1024x1792"),
                            },
                            SelectOption {
                                label: "1792x1024 横版".to_string(),
                                value: json!("1792x1024"),
                            },
                        ]),
                        required: Some(true),
                        advanced: None,
                        group: Some("图像格式".to_string()),
                    },
                    ParameterDefinition {
                        key: "quality".to_string(),
                        label: "渲染质量".to_string(),
                        kind: ParameterType::Select,
                        default: Some(json!("standard")),
                        min: None,
                        max: None,
                        step: None,
                        options: Some(vec![
                            SelectOption {
                                label: "标准 standard".to_string(),
                                value: json!("standard"),
                            },
                            SelectOption {
                                label: "高清 hd".to_string(),
                                value: json!("hd"),
                            },
                        ]),
                        required: None,
                        advanced: None,
                        group: Some("图像格式".to_string()),
                    },
                ],
                constraints: vec![],
                transport: None,
                recommended_defaults: None,
                description: "OpenAI 官方高质量文生图模型".to_string(),
            },
            RegistryModelTemplate {
                id: "flux-1-schnell".to_string(),
                name: "FLUX.1 schnell".to_string(),
                provider_type: ProviderType::Comfyui,
                model_id: "flux1-schnell".to_string(),
                modality: Modality::Image,
                capabilities: json!({ "generate": true, "edit": true }),
                parameters: vec![
                    ParameterDefinition {
                        key: "steps".to_string(),
                        label: "迭代步数".to_string(),
                        kind: ParameterType::Range,
                        default: Some(json!(4)),
                        min: Some(1.0),
                        max: Some(8.0),
                        step: Some(1.0),
                        options: None,
                        required: Some(true),
                        advanced: None,
                        group: Some("采样控制".to_string()),
                    },
                    ParameterDefinition {
                        key: "guidance_scale".to_string(),
                        label: "引导强度 (CFG)".to_string(),
                        kind: ParameterType::Range,
                        default: Some(json!(3.5)),
                        min: Some(0.0),
                        max: Some(10.0),
                        step: Some(0.5),
                        options: None,
                        required: None,
                        advanced: None,
                        group: Some("采样控制".to_string()),
                    },
                    ParameterDefinition {
                        key: "enable_upscale".to_string(),
                        label: "开启高清修复".to_string(),
                        kind: ParameterType::Boolean,
                        default: Some(json!(false)),
                        min: None,
                        max: None,
                        step: None,
                        options: None,
                        required: None,
                        advanced: None,
                        group: Some("后处理".to_string()),
                    },
                    ParameterDefinition {
                        key: "upscale_factor".to_string(),
                        label: "放大倍数".to_string(),
                        kind: ParameterType::Range,
                        default: Some(json!(2.0)),
                        min: Some(1.0),
                        max: Some(4.0),
                        step: Some(0.5),
                        options: None,
                        required: Some(true),
                        advanced: None,
                        group: Some("后处理".to_string()),
                    },
                ],
                constraints: vec![
                    ConstraintDefinition::Requires {
                        field: "enable_upscale".to_string(),
                        required_fields: vec!["upscale_factor".to_string()],
                    },
                    ConstraintDefinition::ConditionalVisibility {
                        field: "upscale_factor".to_string(),
                        depends_on: "enable_upscale".to_string(),
                        equals: Some(json!(true)),
                        not_equals: None,
                    },
                ],
                transport: None,
                recommended_defaults: None,
                description: "开源 SOTA 极速画质生图模型（支持 4 步极速出图）".to_string(),
            },
            RegistryModelTemplate {
                id: "flux-1-dev-comfyui".to_string(),
                name: "FLUX.1 Dev (Workflow)".to_string(),
                provider_type: ProviderType::Comfyui,
                model_id: "flux1-dev-workflow".to_string(),
                modality: Modality::Image,
                capabilities: json!({ "generate": true, "edit": true, "variation": true }),
                parameters: vec![
                    ParameterDefinition {
                        key: "steps".to_string(),
                        label: "迭代步数".to_string(),
                        kind: ParameterType::Range,
                        default: Some(json!(25)),
                        min: Some(1.0),
                        max: Some(50.0),
                        step: Some(1.0),
                        options: None,
                        required: Some(true),
                        advanced: None,
                        group: Some("采样控制".to_string()),
                    },
                    ParameterDefinition {
                        key: "guidance_scale".to_string(),
                        label: "引导强度 (CFG)".to_string(),
                        kind: ParameterType::Range,
                        default: Some(json!(3.5)),
                        min: Some(1.0),
                        max: Some(10.0),
                        step: Some(0.5),
                        options: None,
                        required: None,
                        advanced: None,
                        group: Some("采样控制".to_string()),
                    },
                    ParameterDefinition {
                        key: "seed".to_string(),
                        label: "随机种子".to_string(),
                        kind: ParameterType::Number,
                        default: Some(json!(42)),
                        min: Some(0.0),
                        max: Some(9999999999.0),
                        step: Some(1.0),
                        options: None,
                        required: None,
                        advanced: None,
                        group: Some("采样控制".to_string()),
                    },
                ],
                constraints: vec![],
                transport: Some(TransportDefinition {
                    workflow: Some(json!({
                        "6": { "class_type": "CLIPTextEncode", "inputs": { "text": "high quality photo" } },
                        "8": { "class_type": "VAEDecode", "inputs": { "samples": ["13", 0], "vae": ["10", 0] } },
                        "9": { "class_type": "SaveImage", "inputs": { "filename_prefix": "FluxDev", "images": ["8", 0] } },
                        "10": { "class_type": "VAELoader", "inputs": { "vae_name": "ae.safetensors" } },
                        "12": { "class_type": "UNETLoader", "inputs": { "unet_name": "flux1-dev.safetensors" } },
                        "13": { "class_type": "KSampler", "inputs": { "cfg": 3.5, "denoise": 1.0, "latent_image": ["27", 0], "model": ["12", 0], "positive": ["6", 0], "sampler_name": "euler", "scheduler": "simple", "seed": 42, "steps": 25 } },
                        "27": { "class_type": "EmptySD3LatentImage", "inputs": { "batch_size": 1, "height": 1024, "width": 1024 } }
                    })),
                    bindings: Some(vec![
                        WorkflowBinding {
                            key: "prompt".to_string(),
                            node: "6".to_string(),
                            input: "text".to_string(),
                        },
                        WorkflowBinding {
                            key: "seed".to_string(),
                            node: "13".to_string(),
                            input: "seed".to_string(),
                        },
                        WorkflowBinding {
                            key: "steps".to_string(),
                            node: "13".to_string(),
                            input: "steps".to_string(),
                        },
                        WorkflowBinding {
                            key: "guidance_scale".to_string(),
                            node: "13".to_string(),
                            input: "cfg".to_string(),
                        },
                    ]),
                    ..Default::default()
                }),
                recommended_defaults: None,
                description: "ComfyUI 官方 FLUX.1 Dev 旗舰工作流模板".to_string(),
            },
            RegistryModelTemplate {
                id: "sdxl-base-comfyui".to_string(),
                name: "SDXL 1.0 (Workflow)".to_string(),
                provider_type: ProviderType::Comfyui,
                model_id: "sdxl-base-workflow".to_string(),
                modality: Modality::Image,
                capabilities: json!({ "generate": true, "edit": true }),
                parameters: vec![
                    ParameterDefinition {
                        key: "steps".to_string(),
                        label: "迭代步数".to_string(),
                        kind: ParameterType::Range,
                        default: Some(json!(25)),
                        min: Some(10.0),
                        max: Some(60.0),
                        step: Some(1.0),
                        options: None,
                        required: Some(true),
                        advanced: None,
                        group: Some("采样控制".to_string()),
                    },
                    ParameterDefinition {
                        key: "cfg".to_string(),
                        label: "提示词引导 (CFG)".to_string(),
                        kind: ParameterType::Range,
                        default: Some(json!(7.0)),
                        min: Some(1.0),
                        max: Some(15.0),
                        step: Some(0.5),
                        options: None,
                        required: None,
                        advanced: None,
                        group: Some("采样控制".to_string()),
                    },
                    ParameterDefinition {
                        key: "seed".to_string(),
                        label: "随机种子".to_string(),
                        kind: ParameterType::Number,
                        default: Some(json!(42)),
                        min: Some(0.0),
                        max: Some(9999999999.0),
                        step: Some(1.0),
                        options: None,
                        required: None,
                        advanced: None,
                        group: Some("采样控制".to_string()),
                    },
                ],
                constraints: vec![],
                transport: Some(TransportDefinition {
                    workflow: Some(json!({
                        "3": { "class_type": "KSampler", "inputs": { "cfg": 7.0, "denoise": 1.0, "latent_image": ["5", 0], "model": ["4", 0], "negative": ["7", 0], "positive": ["6", 0], "sampler_name": "euler", "scheduler": "normal", "seed": 42, "steps": 25 } },
                        "4": { "class_type": "CheckpointLoaderSimple", "inputs": { "ckpt_name": "sd_xl_base_1.0.safetensors" } },
                        "5": { "class_type": "EmptyLatentImage", "inputs": { "batch_size": 1, "height": 1024, "width": 1024 } },
                        "6": { "class_type": "CLIPTextEncode", "inputs": { "clip": ["4", 1], "text": "masterpiece" } },
                        "7": { "class_type": "CLIPTextEncode", "inputs": { "clip": ["4", 1], "text": "ugly, blurry" } },
                        "8": { "class_type": "VAEDecode", "inputs": { "samples": ["3", 0], "vae": ["4", 2] } },
                        "9": { "class_type": "SaveImage", "inputs": { "filename_prefix": "SDXL", "images": ["8", 0] } }
                    })),
                    bindings: Some(vec![
                        WorkflowBinding {
                            key: "prompt".to_string(),
                            node: "6".to_string(),
                            input: "text".to_string(),
                        },
                        WorkflowBinding {
                            key: "negativePrompt".to_string(),
                            node: "7".to_string(),
                            input: "text".to_string(),
                        },
                        WorkflowBinding {
                            key: "seed".to_string(),
                            node: "3".to_string(),
                            input: "seed".to_string(),
                        },
                        WorkflowBinding {
                            key: "steps".to_string(),
                            node: "3".to_string(),
                            input: "steps".to_string(),
                        },
                        WorkflowBinding {
                            key: "cfg".to_string(),
                            node: "3".to_string(),
                            input: "cfg".to_string(),
                        },
                    ]),
                    ..Default::default()
                }),
                recommended_defaults: None,
                description: "Stable Diffusion XL 1.0 经典基础生图工作流模板".to_string(),
            },
            RegistryModelTemplate {
                id: "wan-2-1-i2v-comfyui".to_string(),
                name: "Wan 2.1 I2V (Workflow)".to_string(),
                provider_type: ProviderType::Comfyui,
                model_id: "wan2.1-i2v-workflow".to_string(),
                modality: Modality::Video,
                capabilities: json!({ "textToVideo": true, "imageToVideo": true }),
                parameters: vec![
                    ParameterDefinition {
                        key: "frames".to_string(),
                        label: "总帧数".to_string(),
                        kind: ParameterType::Range,
                        default: Some(json!(81)),
                        min: Some(17.0),
                        max: Some(121.0),
                        step: Some(16.0),
                        options: None,
                        required: Some(true),
                        advanced: None,
                        group: Some("视频规格".to_string()),
                    },
                    ParameterDefinition {
                        key: "fps".to_string(),
                        label: "帧率 (FPS)".to_string(),
                        kind: ParameterType::Select,
                        default: Some(json!(16)),
                        min: None,
                        max: None,
                        step: None,
                        options: Some(vec![
                            SelectOption {
                                label: "16 帧/秒".to_string(),
                                value: json!(16),
                            },
                            SelectOption {
                                label: "24 帧/秒".to_string(),
                                value: json!(24),
                            },
                        ]),
                        required: None,
                        advanced: None,
                        group: Some("视频规格".to_string()),
                    },
                ],
                constraints: vec![],
                transport: Some(TransportDefinition {
                    workflow: Some(json!({
                        "3": { "class_type": "KSampler", "inputs": { "cfg": 6.0, "denoise": 1.0, "latent_image": ["10", 0], "model": ["4", 0], "negative": ["7", 0], "positive": ["6", 0], "sampler_name": "euler", "scheduler": "normal", "seed": 42, "steps": 30 } },
                        "4": { "class_type": "CheckpointLoaderSimple", "inputs": { "ckpt_name": "wan2.1_i2v_720p.safetensors" } },
                        "6": { "class_type": "CLIPTextEncode", "inputs": { "clip": ["4", 1], "text": "cinematic scene" } },
                        "7": { "class_type": "CLIPTextEncode", "inputs": { "clip": ["4", 1], "text": "static, watermark" } },
                        "10": { "class_type": "LoadImage", "inputs": { "image": "first_frame.png" } },
                        "12": { "class_type": "SaveAnimatedWEBP", "inputs": { "filename_prefix": "WanI2V", "fps": 16, "images": ["3", 0] } }
                    })),
                    bindings: Some(vec![
                        WorkflowBinding {
                            key: "prompt".to_string(),
                            node: "6".to_string(),
                            input: "text".to_string(),
                        },
                        WorkflowBinding {
                            key: "negativePrompt".to_string(),
                            node: "7".to_string(),
                            input: "text".to_string(),
                        },
                        WorkflowBinding {
                            key: "firstFrame".to_string(),
                            node: "10".to_string(),
                            input: "image".to_string(),
                        },
                        WorkflowBinding {
                            key: "seed".to_string(),
                            node: "3".to_string(),
                            input: "seed".to_string(),
                        },
                    ]),
                    ..Default::default()
                }),
                recommended_defaults: None,
                description: "Wan 2.1 图生视频 (Image-to-Video) ComfyUI 标准工作流模板".to_string(),
            },
            RegistryModelTemplate {
                id: "hunyuan-video-comfyui".to_string(),
                name: "Hunyuan Video (Workflow)".to_string(),
                provider_type: ProviderType::Comfyui,
                model_id: "hunyuan-video-workflow".to_string(),
                modality: Modality::Video,
                capabilities: json!({ "textToVideo": true }),
                parameters: vec![ParameterDefinition {
                    key: "frames".to_string(),
                    label: "总帧数".to_string(),
                    kind: ParameterType::Range,
                    default: Some(json!(65)),
                    min: Some(17.0),
                    max: Some(129.0),
                    step: Some(16.0),
                    options: None,
                    required: Some(true),
                    advanced: None,
                    group: Some("视频规格".to_string()),
                }],
                constraints: vec![],
                transport: Some(TransportDefinition {
                    workflow: Some(json!({
                        "3": { "class_type": "KSampler", "inputs": { "cfg": 6.0, "denoise": 1.0, "model": ["4", 0], "positive": ["6", 0], "sampler_name": "euler", "scheduler": "normal", "seed": 42, "steps": 30 } },
                        "4": { "class_type": "CheckpointLoaderSimple", "inputs": { "ckpt_name": "hunyuan_video.safetensors" } },
                        "6": { "class_type": "CLIPTextEncode", "inputs": { "clip": ["4", 1], "text": "high quality video" } },
                        "12": { "class_type": "SaveAnimatedWEBP", "inputs": { "filename_prefix": "HunyuanVideo", "fps": 24, "images": ["3", 0] } }
                    })),
                    bindings: Some(vec![
                        WorkflowBinding {
                            key: "prompt".to_string(),
                            node: "6".to_string(),
                            input: "text".to_string(),
                        },
                        WorkflowBinding {
                            key: "seed".to_string(),
                            node: "3".to_string(),
                            input: "seed".to_string(),
                        },
                    ]),
                    ..Default::default()
                }),
                recommended_defaults: None,
                description: "腾讯混元开源视频大模型 (Hunyuan Video) ComfyUI 工作流模板"
                    .to_string(),
            },
            RegistryModelTemplate {
                id: "wan-2.1".to_string(),
                name: "Wan2.1".to_string(),
                provider_type: ProviderType::Comfyui,
                model_id: "wan2.1-t2v-14b".to_string(),
                modality: Modality::Video,
                capabilities: json!({ "textToVideo": true, "imageToVideo": true }),
                parameters: vec![
                    ParameterDefinition {
                        key: "frames".to_string(),
                        label: "帧数".to_string(),
                        kind: ParameterType::Range,
                        default: Some(json!(81)),
                        min: Some(17.0),
                        max: Some(121.0),
                        step: Some(16.0),
                        options: None,
                        required: Some(true),
                        advanced: None,
                        group: Some("视频规格".to_string()),
                    },
                    ParameterDefinition {
                        key: "fps".to_string(),
                        label: "帧率 (FPS)".to_string(),
                        kind: ParameterType::Select,
                        default: Some(json!(16)),
                        min: None,
                        max: None,
                        step: None,
                        options: Some(vec![
                            SelectOption {
                                label: "16 帧/秒".to_string(),
                                value: json!(16),
                            },
                            SelectOption {
                                label: "24 帧/秒".to_string(),
                                value: json!(24),
                            },
                        ]),
                        required: None,
                        advanced: None,
                        group: Some("视频规格".to_string()),
                    },
                ],
                constraints: vec![],
                transport: None,
                recommended_defaults: None,
                description: "万象 Wan2.1 顶级开源视频生成大模型".to_string(),
            },
            RegistryModelTemplate {
                id: "whisper-large-v3".to_string(),
                name: "Whisper Large v3".to_string(),
                provider_type: ProviderType::Openai,
                model_id: "whisper-1".to_string(),
                modality: Modality::Audio,
                capabilities: json!({ "speechToText": true }),
                parameters: vec![
                    ParameterDefinition {
                        key: "language".to_string(),
                        label: "识别语言".to_string(),
                        kind: ParameterType::Select,
                        default: Some(json!("zh")),
                        min: None,
                        max: None,
                        step: None,
                        options: Some(vec![
                            SelectOption {
                                label: "中文 (Chinese)".to_string(),
                                value: json!("zh"),
                            },
                            SelectOption {
                                label: "英语 (English)".to_string(),
                                value: json!("en"),
                            },
                            SelectOption {
                                label: "日语 (Japanese)".to_string(),
                                value: json!("ja"),
                            },
                            SelectOption {
                                label: "韩语 (Korean)".to_string(),
                                value: json!("ko"),
                            },
                            SelectOption {
                                label: "法语 (French)".to_string(),
                                value: json!("fr"),
                            },
                            SelectOption {
                                label: "德语 (German)".to_string(),
                                value: json!("de"),
                            },
                            SelectOption {
                                label: "西班牙语 (Spanish)".to_string(),
                                value: json!("es"),
                            },
                        ]),
                        required: None,
                        advanced: None,
                        group: Some("识别设置".to_string()),
                    },
                    ParameterDefinition {
                        key: "timestamp".to_string(),
                        label: "输出分段时间戳".to_string(),
                        kind: ParameterType::Boolean,
                        default: Some(json!(true)),
                        min: None,
                        max: None,
                        step: None,
                        options: None,
                        required: None,
                        advanced: None,
                        group: Some("时间戳与格式".to_string()),
                    },
                    ParameterDefinition {
                        key: "word_timestamps".to_string(),
                        label: "词级时间戳 (Word-level)".to_string(),
                        kind: ParameterType::Boolean,
                        default: Some(json!(false)),
                        min: None,
                        max: None,
                        step: None,
                        options: None,
                        required: None,
                        advanced: None,
                        group: Some("时间戳与格式".to_string()),
                    },
                    ParameterDefinition {
                        key: "speaker".to_string(),
                        label: "说话人区分 (Diarization)".to_string(),
                        kind: ParameterType::Boolean,
                        default: Some(json!(false)),
                        min: None,
                        max: None,
                        step: None,
                        options: None,
                        required: None,
                        advanced: None,
                        group: Some("时间戳与格式".to_string()),
                    },
                ],
                constraints: vec![],
                transport: None,
                recommended_defaults: None,
                description: "高精度语音转文本模型，支持时间戳、词级定位与字幕导出".to_string(),
            },
            RegistryModelTemplate {
                id: "tts-1-hd".to_string(),
                name: "OpenAI TTS-1 HD".to_string(),
                provider_type: ProviderType::Openai,
                model_id: "tts-1-hd".to_string(),
                modality: Modality::Audio,
                capabilities: json!({ "textToSpeech": true }),
                parameters: vec![
                    ParameterDefinition {
                        key: "voice".to_string(),
                        label: "预设音色".to_string(),
                        kind: ParameterType::Select,
                        default: Some(json!("alloy")),
                        min: None,
                        max: None,
                        step: None,
                        options: Some(vec![
                            SelectOption {
                                label: "Alloy (中性沉稳)".to_string(),
                                value: json!("alloy"),
                            },
                            SelectOption {
                                label: "Echo (温暖男声)".to_string(),
                                value: json!("echo"),
                            },
                            SelectOption {
                                label: "Fable (英伦腔调)".to_string(),
                                value: json!("fable"),
                            },
                            SelectOption {
                                label: "Onyx (深沉厚重)".to_string(),
                                value: json!("onyx"),
                            },
                            SelectOption {
                                label: "Nova (年轻亲切)".to_string(),
                                value: json!("nova"),
                            },
                            SelectOption {
                                label: "Shimmer (清亮女声)".to_string(),
                                value: json!("shimmer"),
                            },
                        ]),
                        required: Some(true),
                        advanced: None,
                        group: Some("音色选择".to_string()),
                    },
                    ParameterDefinition {
                        key: "speed".to_string(),
                        label: "语速倍率".to_string(),
                        kind: ParameterType::Range,
                        default: Some(json!(1.0)),
                        min: Some(0.25),
                        max: Some(4.0),
                        step: Some(0.05),
                        options: None,
                        required: None,
                        advanced: None,
                        group: Some("音频参数".to_string()),
                    },
                    ParameterDefinition {
                        key: "emotion".to_string(),
                        label: "情感表现".to_string(),
                        kind: ParameterType::Select,
                        default: Some(json!("neutral")),
                        min: None,
                        max: None,
                        step: None,
                        options: Some(vec![
                            SelectOption {
                                label: "自然平静 (Neutral)".to_string(),
                                value: json!("neutral"),
                            },
                            SelectOption {
                                label: "愉悦开心 (Happy)".to_string(),
                                value: json!("happy"),
                            },
                            SelectOption {
                                label: "温柔亲近 (Gentle)".to_string(),
                                value: json!("gentle"),
                            },
                            SelectOption {
                                label: "严谨庄重 (Serious)".to_string(),
                                value: json!("serious"),
                            },
                            SelectOption {
                                label: "激昂兴奋 (Excited)".to_string(),
                                value: json!("excited"),
                            },
                            SelectOption {
                                label: "低落悲伤 (Sad)".to_string(),
                                value: json!("sad"),
                            },
                        ]),
                        required: None,
                        advanced: Some(true),
                        group: Some("音色表现".to_string()),
                    },
                    ParameterDefinition {
                        key: "style".to_string(),
                        label: "风格语境".to_string(),
                        kind: ParameterType::Select,
                        default: Some(json!("narration")),
                        min: None,
                        max: None,
                        step: None,
                        options: Some(vec![
                            SelectOption {
                                label: "旁白解说 (Narration)".to_string(),
                                value: json!("narration"),
                            },
                            SelectOption {
                                label: "日常会话 (Chat)".to_string(),
                                value: json!("chat"),
                            },
                            SelectOption {
                                label: "新闻广播 (News)".to_string(),
                                value: json!("news"),
                            },
                            SelectOption {
                                label: "有声读物 (Audiobook)".to_string(),
                                value: json!("audiobook"),
                            },
                            SelectOption {
                                label: "影视配音 (Cinema)".to_string(),
                                value: json!("cinema"),
                            },
                        ]),
                        required: None,
                        advanced: Some(true),
                        group: Some("音色表现".to_string()),
                    },
                ],
                constraints: vec![],
                transport: None,
                recommended_defaults: None,
                description: "OpenAI 官方高清拟真语音合成模型，支持语速控制与风格化预设"
                    .to_string(),
            },
            RegistryModelTemplate {
                id: "f5-tts-clone".to_string(),
                name: "F5-TTS Voice Clone".to_string(),
                provider_type: ProviderType::Fal,
                model_id: "fal-ai/f5-tts".to_string(),
                modality: Modality::Audio,
                capabilities: json!({ "textToSpeech": true, "voiceClone": true }),
                parameters: vec![ParameterDefinition {
                    key: "speed".to_string(),
                    label: "语速".to_string(),
                    kind: ParameterType::Range,
                    default: Some(json!(1.0)),
                    min: Some(0.5),
                    max: Some(2.0),
                    step: Some(0.1),
                    options: None,
                    required: None,
                    advanced: None,
                    group: Some("克隆控制".to_string()),
                }],
                constraints: vec![],
                transport: None,
                recommended_defaults: None,
                description: "开源 SOTA 极速非自回归声音克隆大模型，仅需少量参考音频即可克隆音色"
                    .to_string(),
            },
            RegistryModelTemplate {
                id: "cosyvoice-speech-to-speech".to_string(),
                name: "CosyVoice S2S".to_string(),
                provider_type: ProviderType::GenericHttp,
                model_id: "cosyvoice-s2s".to_string(),
                modality: Modality::Audio,
                capabilities: json!({ "speechToSpeech": true, "voiceClone": true }),
                parameters: vec![ParameterDefinition {
                    key: "speed".to_string(),
                    label: "目标语速".to_string(),
                    kind: ParameterType::Range,
                    default: Some(json!(1.0)),
                    min: Some(0.5),
                    max: Some(2.0),
                    step: Some(0.1),
                    options: None,
                    required: None,
                    advanced: None,
                    group: Some("变声控制".to_string()),
                }],
                constraints: vec![],
                transport: Some(TransportDefinition {
                    http: Some(crate::domain::HttpRecipe {
                        method: "POST".to_string(),
                        path: "v1/audio/speech-to-speech".to_string(),
                        headers: Vec::new(),
                        body: Some(json!({
                            "source_audio": "{{sourceAudio}}",
                            "reference_audio": "{{referenceAudio}}",
                            "speed": "{{speed}}"
                        })),
                        task_id_path: None,
                        poll: None,
                        result_path: "audio_url".to_string(),
                        result_kind: Some("url".to_string()),
                    }),
                    ..Default::default()
                }),
                recommended_defaults: None,
                description: "多语言多重情感语音转换与语音转语音模型".to_string(),
            },
        ],
    }
}

pub fn get_catalog(data_dir: &Path) -> RegistryCatalog {
    let registry_dir = data_dir.join("registry");
    let file = registry_dir.join("catalog.json");
    let backup_file = registry_dir.join("catalog.backup.json");

    let (can_rollback, backup_version) = if backup_file.exists() {
        if let Ok(bytes) = std::fs::read(&backup_file) {
            if let Ok(backup_cat) = serde_json::from_slice::<RegistryCatalog>(&bytes) {
                (true, Some(backup_cat.version))
            } else {
                (true, None)
            }
        } else {
            (false, None)
        }
    } else {
        (false, None)
    };

    let mut catalog = if file.exists() {
        if let Ok(bytes) = std::fs::read(&file) {
            if let Ok(cat) = serde_json::from_slice::<RegistryCatalog>(&bytes) {
                cat
            } else {
                default_catalog()
            }
        } else {
            default_catalog()
        }
    } else {
        default_catalog()
    };

    catalog.can_rollback = can_rollback;
    catalog.backup_version = backup_version;
    catalog
}

pub fn rollback_registry(data_dir: &Path) -> Result<RegistryCatalog, AppError> {
    let registry_dir = data_dir.join("registry");
    let backup_file = registry_dir.join("catalog.backup.json");
    let current_file = registry_dir.join("catalog.json");

    if !backup_file.exists() {
        return Err(AppError::validation("没有可供回滚的 Registry 备份版本"));
    }

    let backup_bytes = std::fs::read(&backup_file)
        .map_err(|e| AppError::validation(format!("读取备份文件失败: {e}")))?;
    let _backup_cat: RegistryCatalog = serde_json::from_slice(&backup_bytes)
        .map_err(|e| AppError::validation(format!("解析备份数据失败: {e}")))?;

    crate::storage_util::atomic_write(&current_file, &backup_bytes)
        .map_err(|e| AppError::validation(format!("还原备份文件失败: {e}")))?;

    let _ = std::fs::remove_file(&backup_file);

    let mut restored = get_catalog(data_dir);
    restored.can_rollback = false;
    restored.backup_version = None;
    Ok(restored)
}

pub async fn check_update(
    data_dir: &Path,
    http: &Client,
    custom_url: Option<String>,
) -> Result<RegistryUpdateCheck, AppError> {
    let current = get_catalog(data_dir);
    let target_url = custom_url
        .as_deref()
        .or(current.source_url.as_deref())
        .unwrap_or(DEFAULT_REGISTRY_URL);

    let res = http.get(target_url).send().await.map_err(|err| {
        AppError::provider(
            format!("连接远程 Registry 失败: {err}"),
            502,
            "Registry",
            None,
            None,
        )
    })?;

    if !res.status().is_success() {
        return Err(AppError::provider(
            format!("Registry 更新地址响应状态异常 {}", res.status()),
            res.status().as_u16(),
            "Registry",
            None,
            None,
        ));
    }

    let remote: RegistryCatalog = res
        .json()
        .await
        .map_err(|err| AppError::validation(format!("解析远程 Registry JSON 失败: {err}")))?;

    let has_update = current.version != remote.version;
    let release_notes = if has_update {
        Some(format!("包含 {} 个预设模型", remote.models.len()))
    } else {
        None
    };

    Ok(RegistryUpdateCheck {
        current_version: current.version.clone(),
        latest_version: remote.version.clone(),
        remote_version: remote.version.clone(),
        has_update,
        remote_model_count: remote.models.len(),
        release_notes,
    })
}

pub async fn apply_update(
    data_dir: &Path,
    http: &Client,
    custom_url: Option<String>,
) -> Result<RegistryCatalog, AppError> {
    let current = get_catalog(data_dir);
    let target_url = custom_url
        .as_deref()
        .or(current.source_url.as_deref())
        .unwrap_or(DEFAULT_REGISTRY_URL);

    let res = http.get(target_url).send().await.map_err(|err| {
        AppError::provider(
            format!("连接远程 Registry 失败: {err}"),
            502,
            "Registry",
            None,
            None,
        )
    })?;

    if !res.status().is_success() {
        return Err(AppError::provider(
            format!("Registry 更新地址响应状态异常 {}", res.status()),
            res.status().as_u16(),
            "Registry",
            None,
            None,
        ));
    }

    let mut remote: RegistryCatalog = res
        .json()
        .await
        .map_err(|err| AppError::validation(format!("解析远程 Registry JSON 失败: {err}")))?;

    remote.source_url = Some(target_url.to_string());

    let registry_dir = data_dir.join("registry");
    std::fs::create_dir_all(&registry_dir)
        .map_err(|e| AppError::validation(format!("创建目录失败: {e}")))?;
    let file = registry_dir.join("catalog.json");
    let backup_file = registry_dir.join("catalog.backup.json");

    let backup_bytes = if file.exists() {
        let current_bytes = std::fs::read(&file)
            .map_err(|e| AppError::validation(format!("读取当前 Registry 失败: {e}")))?;
        if serde_json::from_slice::<RegistryCatalog>(&current_bytes).is_ok() {
            current_bytes
        } else {
            serde_json::to_vec_pretty(&current)
                .map_err(|e| AppError::validation(format!("序列化 Registry 恢复备份失败: {e}")))?
        }
    } else {
        serde_json::to_vec_pretty(&default_catalog())
            .map_err(|e| AppError::validation(format!("序列化 Registry 默认备份失败: {e}")))?
    };
    // A successful remote update must always have a verified rollback copy.
    // Never replace the current catalog after a failed backup write.
    crate::storage_util::atomic_write(&backup_file, &backup_bytes)
        .map_err(|e| AppError::validation(format!("写入 Registry 备份失败: {e}")))?;
    let written_backup = std::fs::read(&backup_file)
        .map_err(|e| AppError::validation(format!("读取 Registry 备份失败: {e}")))?;
    serde_json::from_slice::<RegistryCatalog>(&written_backup)
        .map_err(|e| AppError::validation(format!("Registry 备份校验失败: {e}")))?;

    let json_bytes = serde_json::to_vec_pretty(&remote)
        .map_err(|e| AppError::validation(format!("序列化 Registry 失败: {e}")))?;
    crate::storage_util::atomic_write(&file, &json_bytes)
        .map_err(|e| AppError::validation(format!("写入 Registry 失败: {e}")))?;

    let mut updated = get_catalog(data_dir);
    updated.can_rollback = true;
    updated.backup_version = Some(current.version);
    Ok(updated)
}

pub async fn import_model_from_registry(
    pool: &SqlitePool,
    data_dir: &Path,
    template_id: &str,
    target_provider_id: &str,
    overwrite: Option<bool>,
) -> Result<ModelDefinition, AppError> {
    let catalog = get_catalog(data_dir);
    let template = catalog
        .models
        .into_iter()
        .find(|m| m.id == template_id)
        .ok_or_else(|| AppError::not_found("未在 Registry 中找到指定模型模板"))?;

    let provider = db::get_provider(pool, target_provider_id).await?;

    let existing_models = db::list_models(pool).await?;
    let conflict = existing_models
        .iter()
        .find(|m| m.provider_id == provider.id && m.model_id == template.model_id);

    if let Some(existing) = conflict {
        if overwrite == Some(true) {
            let update_input = ModelInput {
                name: template.name,
                provider_id: provider.id,
                model_id: template.model_id,
                modality: template.modality,
                capabilities: template.capabilities,
                parameters: template.parameters,
                constraints: template.constraints,
                transport: template.transport,
                enabled: true,
            };
            return crate::services::models::update(pool, &existing.id, update_input).await;
        } else {
            return Err(AppError::validation(format!(
                "目标 Provider 下已存在相同模型「{}」（ID: {}），为防止自动覆盖已有修改，请确认覆盖或更换目标 Provider",
                existing.name, existing.model_id
            )));
        }
    }

    let input = ModelInput {
        name: template.name,
        provider_id: provider.id,
        model_id: template.model_id,
        modality: template.modality,
        capabilities: template.capabilities,
        parameters: template.parameters,
        constraints: template.constraints,
        transport: template.transport,
        enabled: true,
    };

    crate::services::models::create(pool, input).await
}
