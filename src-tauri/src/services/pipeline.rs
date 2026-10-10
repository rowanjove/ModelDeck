use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::credentials::SecretStore;
use crate::domain::{
    now_iso, GenerateAudioInput, GenerateImageInput, GenerateTextInput, GenerateVideoInput,
    MediaRef, Modality,
};
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default = "crate::domain::default_schema_version")]
    pub schema_version: u32,
    pub steps: Vec<PipelineStep>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum PipelineStep {
    #[serde(rename = "input")]
    Input {
        id: String,
        label: String,
        #[serde(default)]
        default_value: String,
    },
    #[serde(rename = "model")]
    Model {
        id: String,
        label: String,
        model_definition_id: String,
        prompt_template: String,
        #[serde(default)]
        system_prompt: Option<String>,
        #[serde(default)]
        parameters: Map<String, Value>,
    },
    #[serde(rename = "transform")]
    Transform {
        id: String,
        label: String,
        kind: String, // "template" | "regex" | "json_field" | "trim"
        rule: String,
    },
}

impl PipelineStep {
    pub fn id(&self) -> &str {
        match self {
            Self::Input { id, .. } => id,
            Self::Model { id, .. } => id,
            Self::Transform { id, .. } => id,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Input { label, .. } => label,
            Self::Model { label, .. } => label,
            Self::Transform { label, .. } => label,
        }
    }

    pub fn step_type(&self) -> &'static str {
        match self {
            Self::Input { .. } => "input",
            Self::Model { .. } => "model",
            Self::Transform { .. } => "transform",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineExecutionInput {
    pub pipeline_id: Option<String>,
    pub steps: Vec<PipelineStep>,
    pub initial_input: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineStepResult {
    pub step_id: String,
    pub step_label: String,
    pub step_type: String,
    pub output_kind: String, // "text" | "image" | "video" | "audio"
    pub text_output: Option<String>,
    pub media_paths: Vec<String>,
    pub latency_ms: u64,
    pub status: String, // "completed" | "failed"
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineExecutionResult {
    pub total_latency_ms: u64,
    pub step_results: Vec<PipelineStepResult>,
    pub final_output_kind: String,
    pub final_text: Option<String>,
    pub final_media_paths: Vec<String>,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<PipelineDefinition>, AppError> {
    let rows = sqlx::query(
        "SELECT id, name, description, steps_json, created_at, updated_at
         FROM pipelines ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(AppError::db)?;

    let mut defs = Vec::new();
    for row in rows {
        defs.push(definition_from_row(&row)?);
    }
    Ok(defs)
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<PipelineDefinition, AppError> {
    let row = sqlx::query(
        "SELECT id, name, description, steps_json, created_at, updated_at
         FROM pipelines WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::db)?;

    let Some(row) = row else {
        return Err(AppError::not_found("找不到指定的工作流管道"));
    };

    definition_from_row(&row)
}

pub async fn save(
    pool: &SqlitePool,
    mut def: PipelineDefinition,
) -> Result<PipelineDefinition, AppError> {
    if def.name.trim().is_empty() {
        return Err(AppError::validation("工作流名称不能为空"));
    }
    if def.steps.is_empty() {
        return Err(AppError::validation("工作流至少需要包含一个步骤"));
    }

    let now = now_iso();
    let is_new = def.id.trim().is_empty();
    if is_new {
        def.id = Uuid::new_v4().to_string();
        def.created_at = now.clone();
    }
    def.updated_at = now;

    let steps_json = serde_json::to_string(&def.steps)
        .map_err(|e| AppError::validation(format!("步骤配置序列化失败: {e}")))?;

    if is_new {
        sqlx::query(
            "INSERT INTO pipelines (id, name, description, steps_json, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&def.id)
        .bind(&def.name)
        .bind(&def.description)
        .bind(&steps_json)
        .bind(&def.created_at)
        .bind(&def.updated_at)
        .execute(pool)
        .await
        .map_err(AppError::db)?;
    } else {
        sqlx::query(
            "UPDATE pipelines
             SET name = ?, description = ?, steps_json = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&def.name)
        .bind(&def.description)
        .bind(&steps_json)
        .bind(&def.updated_at)
        .bind(&def.id)
        .execute(pool)
        .await
        .map_err(AppError::db)?;
    }

    Ok(def)
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let res = sqlx::query("DELETE FROM pipelines WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(AppError::db)?;

    if res.rows_affected() == 0 {
        return Err(AppError::not_found("找不到要删除的工作流"));
    }
    Ok(())
}

pub fn presets() -> Vec<PipelineDefinition> {
    vec![
        PipelineDefinition {
            id: "preset_article_to_image".to_string(),
            name: "📖 文章提炼 → 视觉绘图".to_string(),
            description: "输入长文或新闻，由文本大模型提炼核心画面描述，再送入图像模型生成配图。".to_string(),
            steps: vec![
                PipelineStep::Input {
                    id: "step_input".to_string(),
                    label: "文章输入".to_string(),
                    default_value: "在一个漂浮于云海之上的赛博朋克悬浮岛屿上，巨大的发光机械樱花树在霓虹细雨中盛开，远处可见穿梭的磁悬浮列车。".to_string(),
                },
                PipelineStep::Model {
                    id: "step_summarize".to_string(),
                    label: "LLM 提炼绘画提示词".to_string(),
                    model_definition_id: "".to_string(),
                    prompt_template: "请阅读以下文章，为其提炼一段极其生动细腻的纯英文 Midjourney / Stable Diffusion 绘画画面描述，突出构图、光影与艺术风格。直接输出英文 Prompt，不要包含多余解说：\n\n{{input}}".to_string(),
                    system_prompt: Some("你是一位顶级 AI 视觉艺术家与 Prompt 专家。".to_string()),
                    parameters: Map::new(),
                },
                PipelineStep::Transform {
                    id: "step_format_prompt".to_string(),
                    label: "提示词清洗与质量增强".to_string(),
                    kind: "template".to_string(),
                    rule: "{{prev}}, masterpiece, highly detailed, 8k resolution, cinematic lighting".to_string(),
                },
                PipelineStep::Model {
                    id: "step_generate_image".to_string(),
                    label: "AI 图像生成".to_string(),
                    model_definition_id: "".to_string(),
                    prompt_template: "{{prev}}".to_string(),
                    system_prompt: None,
                    parameters: Map::new(),
                },
            ],
            created_at: "".to_string(),
            updated_at: "".to_string(),
            schema_version: 1,
        },
        PipelineDefinition {
            id: "preset_translate_and_tts".to_string(),
            name: "🌐 智能翻译 → 语音朗读 (TTS)".to_string(),
            description: "输入任意语言文本，由大模型翻译为优美目标语言，随后由语音模型朗读发音。".to_string(),
            steps: vec![
                PipelineStep::Input {
                    id: "step_input".to_string(),
                    label: "待翻译文本".to_string(),
                    default_value: "人工智能正深刻改变着每一个人的工作与创作方式，赋予每个人无限的想象力。".to_string(),
                },
                PipelineStep::Model {
                    id: "step_translate".to_string(),
                    label: "多语言精准翻译".to_string(),
                    model_definition_id: "".to_string(),
                    prompt_template: "请将以下文本翻译为地道、自然的英文，适合作为有声读物旁白朗读，仅返回翻译结果：\n\n{{input}}".to_string(),
                    system_prompt: Some("你是一位专业影视旁白译员。".to_string()),
                    parameters: Map::new(),
                },
                PipelineStep::Model {
                    id: "step_tts".to_string(),
                    label: "自然语音合成 (TTS)".to_string(),
                    model_definition_id: "".to_string(),
                    prompt_template: "{{prev}}".to_string(),
                    system_prompt: None,
                    parameters: Map::new(),
                },
            ],
            created_at: "".to_string(),
            updated_at: "".to_string(),
            schema_version: 1,
        },
        PipelineDefinition {
            id: "preset_story_to_video".to_string(),
            name: "🎬 故事分镜 → 视频生成".to_string(),
            description: "输入故事脚本，LLM 提炼分镜镜头运镜与动态视觉描述，送入视频大模型生成镜头片段。".to_string(),
            steps: vec![
                PipelineStep::Input {
                    id: "step_input".to_string(),
                    label: "故事脚本".to_string(),
                    default_value: "一名宇航员缓步走在未知的红色沙漠星球上，夕阳将他的影子拉得很长，突然地平线处升起两个巨大的银色光环。".to_string(),
                },
                PipelineStep::Model {
                    id: "step_shot_prompt".to_string(),
                    label: "视频运镜 Prompt 提炼".to_string(),
                    model_definition_id: "".to_string(),
                    prompt_template: "请为以下故事设计一段电影级镜头动态英文 Prompt，包含镜头景别、运镜轨迹与主体动态：\n\n{{input}}".to_string(),
                    system_prompt: Some("你是一位院线电影导演与运镜指导。".to_string()),
                    parameters: Map::new(),
                },
                PipelineStep::Model {
                    id: "step_video".to_string(),
                    label: "AI 视频模型生成".to_string(),
                    model_definition_id: "".to_string(),
                    prompt_template: "{{prev}}".to_string(),
                    system_prompt: None,
                    parameters: Map::new(),
                },
            ],
            created_at: "".to_string(),
            updated_at: "".to_string(),
            schema_version: 1,
        },
    ]
}

#[allow(dead_code)]
pub async fn run(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    data_dir: &Path,
    input: PipelineExecutionInput,
) -> Result<PipelineExecutionResult, AppError> {
    run_with_cancel(pool, http, secrets, data_dir, input, None).await
}

pub type StepProgressCallback = Arc<dyn Fn(usize, usize, &PipelineStepResult) + Send + Sync>;

pub async fn run_with_cancel(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    data_dir: &Path,
    input: PipelineExecutionInput,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<PipelineExecutionResult, AppError> {
    run_with_cancel_and_callback(pool, http, secrets, data_dir, input, cancel, None, None).await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_with_cancel_and_callback(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    data_dir: &Path,
    input: PipelineExecutionInput,
    cancel: Option<Arc<AtomicBool>>,
    on_step: Option<StepProgressCallback>,
    scheduler: Option<&Arc<crate::services::task_scheduler::TaskScheduler>>,
) -> Result<PipelineExecutionResult, AppError> {
    if input.steps.is_empty() {
        return Err(AppError::validation("没有可执行的工作流步骤"));
    }

    let overall_start = Instant::now();
    let mut step_results = Vec::new();
    let mut var_map: HashMap<String, String> = HashMap::new();
    var_map.insert("input".to_string(), input.initial_input.clone());

    let mut last_output_text: Option<String> = None;
    let mut last_media_paths: Vec<String> = Vec::new();
    let mut last_output_kind = "text".to_string();

    for (index, step) in input.steps.iter().enumerate() {
        if let Some(cancel_flag) = &cancel {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(AppError::new("cancelled", "工作流执行已被取消"));
            }
        }

        let step_start = Instant::now();
        let step_id = step.id().to_string();
        let step_label = step.label().to_string();
        let step_type = step.step_type().to_string();

        let prev_val = last_output_text.clone().unwrap_or_else(|| {
            last_media_paths
                .first()
                .cloned()
                .unwrap_or_else(|| input.initial_input.clone())
        });
        var_map.insert("prev".to_string(), prev_val);
        var_map.insert(
            format!("step_{index}"),
            last_output_text.clone().unwrap_or_default(),
        );

        if cancel.as_ref().is_some_and(|f| f.load(Ordering::Relaxed)) {
            return Err(AppError::validation("工作流已取消"));
        }

        match step {
            PipelineStep::Input { default_value, .. } => {
                let text = if !input.initial_input.trim().is_empty() {
                    input.initial_input.clone()
                } else {
                    default_value.clone()
                };
                last_output_text = Some(text.clone());
                last_media_paths.clear();
                last_output_kind = "text".to_string();
                var_map.insert(step_id.clone(), text.clone());

                let step_res = PipelineStepResult {
                    step_id,
                    step_label,
                    step_type,
                    output_kind: "text".to_string(),
                    text_output: Some(text),
                    media_paths: Vec::new(),
                    latency_ms: step_start.elapsed().as_millis() as u64,
                    status: "completed".to_string(),
                    error: None,
                };
                if let Some(cb) = &on_step {
                    cb(index, input.steps.len(), &step_res);
                }
                step_results.push(step_res);
            }
            PipelineStep::Transform { kind, rule, .. } => {
                let transformed = match kind.as_str() {
                    "template" => substitute_vars(rule, &var_map),
                    "trim" => {
                        let base = var_map.get("prev").cloned().unwrap_or_default();
                        base.trim().to_string()
                    }
                    "json_field" => {
                        let base = var_map.get("prev").cloned().unwrap_or_default();
                        match serde_json::from_str::<Value>(&base) {
                            Ok(val) => val
                                .get(rule.trim())
                                .and_then(Value::as_str)
                                .unwrap_or(&base)
                                .to_string(),
                            Err(_) => base,
                        }
                    }
                    _ => substitute_vars(rule, &var_map),
                };

                last_output_text = Some(transformed.clone());
                last_media_paths.clear();
                last_output_kind = "text".to_string();
                var_map.insert(step_id.clone(), transformed.clone());

                let step_res = PipelineStepResult {
                    step_id,
                    step_label,
                    step_type,
                    output_kind: "text".to_string(),
                    text_output: Some(transformed),
                    media_paths: Vec::new(),
                    latency_ms: step_start.elapsed().as_millis() as u64,
                    status: "completed".to_string(),
                    error: None,
                };
                if let Some(cb) = &on_step {
                    cb(index, input.steps.len(), &step_res);
                }
                step_results.push(step_res);
            }
            PipelineStep::Model {
                model_definition_id,
                prompt_template,
                system_prompt,
                parameters,
                ..
            } => {
                if model_definition_id.trim().is_empty() {
                    return Err(AppError::validation(format!(
                        "步骤「{step_label}」未配置目标模型"
                    )));
                }

                let effective_prompt = substitute_vars(prompt_template, &var_map);
                let model = crate::db::get_model(pool, model_definition_id).await?;
                let is_comfyui = crate::db::is_comfyui_model(pool, &model.id).await;
                let _permit = if let Some(sched) = scheduler {
                    Some(
                        sched
                            .acquire_cancellable(model.modality, is_comfyui, cancel.as_deref())
                            .await?,
                    )
                } else {
                    None
                };

                let latency: u64;
                match model.modality {
                    Modality::Text => {
                        let res = crate::services::generate::execute_with_cancel(
                            pool,
                            http,
                            secrets,
                            data_dir,
                            GenerateTextInput {
                                model_definition_id: model.id.clone(),
                                system_prompt: system_prompt.clone(),
                                prompt: effective_prompt,
                                parameters: parameters.clone(),
                                extra: None,
                                images: Vec::new(),
                            },
                            cancel.clone(),
                        )
                        .await?;

                        latency = res.latency_ms;
                        last_output_text = Some(res.text.clone());
                        last_media_paths.clear();
                        last_output_kind = "text".to_string();
                        var_map.insert(step_id.clone(), res.text.clone());

                        let step_res = PipelineStepResult {
                            step_id,
                            step_label,
                            step_type,
                            output_kind: "text".to_string(),
                            text_output: Some(res.text),
                            media_paths: Vec::new(),
                            latency_ms: latency,
                            status: "completed".to_string(),
                            error: None,
                        };
                        if let Some(cb) = &on_step {
                            cb(index, input.steps.len(), &step_res);
                        }
                        step_results.push(step_res);
                    }
                    Modality::Image => {
                        let source_image = if !last_media_paths.is_empty() {
                            Some(last_media_paths[0].clone())
                        } else {
                            None
                        };

                        let res = crate::services::image::execute_with_cancel(
                            pool,
                            http,
                            secrets,
                            data_dir,
                            GenerateImageInput {
                                model_definition_id: model.id.clone(),
                                prompt: effective_prompt,
                                negative_prompt: None,
                                source_image: source_image.map(MediaRef::from_path),
                                reference_images: Vec::new(),
                                mask_base64: None,
                                mask_path: None,
                                parameters: parameters.clone(),
                                extra: None,
                            },
                            None,
                            cancel.clone(),
                        )
                        .await?;

                        latency = res.latency_ms;
                        let paths: Vec<String> = res.images.into_iter().map(|f| f.path).collect();
                        last_output_text = None;
                        last_media_paths = paths.clone();
                        last_output_kind = "image".to_string();
                        if let Some(p) = paths.first() {
                            var_map.insert(step_id.clone(), p.clone());
                        }

                        let step_res = PipelineStepResult {
                            step_id,
                            step_label,
                            step_type,
                            output_kind: "image".to_string(),
                            text_output: None,
                            media_paths: paths,
                            latency_ms: latency,
                            status: "completed".to_string(),
                            error: None,
                        };
                        if let Some(cb) = &on_step {
                            cb(index, input.steps.len(), &step_res);
                        }
                        step_results.push(step_res);
                    }
                    Modality::Video => {
                        let first_frame = if !last_media_paths.is_empty() {
                            Some(last_media_paths[0].clone())
                        } else {
                            None
                        };

                        let res = crate::services::video::execute_with_cancel(
                            pool,
                            http,
                            secrets,
                            data_dir,
                            GenerateVideoInput {
                                model_definition_id: model.id.clone(),
                                prompt: effective_prompt,
                                negative_prompt: None,
                                first_frame: first_frame.map(MediaRef::from_path),
                                last_frame: None,
                                reference_images: Vec::new(),
                                reference_videos: Vec::new(),
                                reference_audios: Vec::new(),
                                parameters: parameters.clone(),
                                extra: None,
                            },
                            None,
                            cancel.clone(),
                        )
                        .await?;

                        latency = res.latency_ms;
                        let paths: Vec<String> = res.videos.into_iter().map(|f| f.path).collect();
                        last_output_text = None;
                        last_media_paths = paths.clone();
                        last_output_kind = "video".to_string();
                        if let Some(p) = paths.first() {
                            var_map.insert(step_id.clone(), p.clone());
                        }

                        let step_res = PipelineStepResult {
                            step_id,
                            step_label,
                            step_type,
                            output_kind: "video".to_string(),
                            text_output: None,
                            media_paths: paths,
                            latency_ms: latency,
                            status: "completed".to_string(),
                            error: None,
                        };
                        if let Some(cb) = &on_step {
                            cb(index, input.steps.len(), &step_res);
                        }
                        step_results.push(step_res);
                    }
                    Modality::Audio => {
                        let res = crate::services::audio::execute_with_cancel(
                            pool,
                            http,
                            secrets,
                            data_dir,
                            GenerateAudioInput {
                                model_definition_id: model.id.clone(),
                                task: Some("tts".to_string()),
                                text: effective_prompt,
                                voice: None,
                                source_audio: None,
                                reference_audio: None,
                                reference_transcript: None,
                                parameters: parameters.clone(),
                                extra: None,
                            },
                            None,
                            cancel.clone(),
                        )
                        .await?;

                        latency = res.latency_ms;
                        let paths: Vec<String> = res.audios.into_iter().map(|f| f.path).collect();
                        let transcript = if !res.transcript.trim().is_empty() {
                            Some(res.transcript)
                        } else {
                            None
                        };
                        last_output_text = transcript.clone();
                        last_media_paths = paths.clone();
                        last_output_kind = "audio".to_string();
                        if let Some(p) = paths.first() {
                            var_map.insert(step_id.clone(), p.clone());
                        }

                        let step_res = PipelineStepResult {
                            step_id,
                            step_label,
                            step_type,
                            output_kind: "audio".to_string(),
                            text_output: transcript,
                            media_paths: paths,
                            latency_ms: latency,
                            status: "completed".to_string(),
                            error: None,
                        };
                        if let Some(cb) = &on_step {
                            cb(index, input.steps.len(), &step_res);
                        }
                        step_results.push(step_res);
                    }
                }
            }
        }
    }

    if cancel.as_ref().is_some_and(|f| f.load(Ordering::Relaxed)) {
        return Err(AppError::validation("工作流已取消"));
    }

    Ok(PipelineExecutionResult {
        total_latency_ms: overall_start.elapsed().as_millis() as u64,
        step_results,
        final_output_kind: last_output_kind,
        final_text: last_output_text,
        final_media_paths: last_media_paths,
    })
}

fn substitute_vars(template: &str, vars: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (k, v) in vars {
        let pattern = format!("{{{{{}}}}}", k);
        result = result.replace(&pattern, v);
    }
    result
}

fn definition_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<PipelineDefinition, AppError> {
    let steps_json: String = row.try_get("steps_json").map_err(AppError::db)?;
    let steps: Vec<PipelineStep> = serde_json::from_str(&steps_json).unwrap_or_default();
    Ok(PipelineDefinition {
        id: row.try_get("id").map_err(AppError::db)?,
        name: row.try_get("name").map_err(AppError::db)?,
        description: row.try_get("description").map_err(AppError::db)?,
        steps,
        created_at: row.try_get("created_at").map_err(AppError::db)?,
        updated_at: row.try_get("updated_at").map_err(AppError::db)?,
        schema_version: 1,
    })
}
