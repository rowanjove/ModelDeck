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

pub fn default_data_type() -> String {
    "text".to_string()
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
        #[serde(default = "default_data_type")]
        data_type: String, // "text" | "image" | "video" | "audio"
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
        #[serde(default)]
        expected_input_kind: Option<String>, // "text" | "image" | "video" | "audio"
        #[serde(default)]
        media_input_variable: Option<String>, // e.g. "{{step_keyframe.media}}" or "{{prev}}"
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
    #[serde(default)]
    pub resume_from_step_index: Option<usize>,
    #[serde(default)]
    pub cached_step_results: Option<Vec<PipelineStepResult>>,
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

pub fn validate_pipeline_definition(def: &PipelineDefinition) -> Result<(), AppError> {
    if def.name.trim().is_empty() {
        return Err(AppError::validation("工作流名称不能为空"));
    }
    if def.steps.is_empty() {
        return Err(AppError::validation("工作流至少需要包含一个步骤"));
    }
    let mut seen_ids = std::collections::HashSet::new();
    for (idx, step) in def.steps.iter().enumerate() {
        let id = step.id().trim();
        if id.is_empty() {
            return Err(AppError::validation(format!(
                "第 {} 个步骤未指定有效 ID",
                idx + 1
            )));
        }
        if !seen_ids.insert(id.to_string()) {
            return Err(AppError::validation(format!("步骤 ID 重复:「{id}」")));
        }
    }
    Ok(())
}

pub fn export_pipeline_json(def: &PipelineDefinition) -> Result<String, AppError> {
    validate_pipeline_definition(def)?;
    serde_json::to_string_pretty(def)
        .map_err(|e| AppError::validation(format!("工作流导出 JSON 失败: {e}")))
}

pub fn import_pipeline_json(json_str: &str) -> Result<PipelineDefinition, AppError> {
    let def: PipelineDefinition = serde_json::from_str(json_str)
        .map_err(|e| AppError::validation(format!("工作流导入失败，JSON 格式或结构不合法: {e}")))?;
    validate_pipeline_definition(&def)?;
    Ok(def)
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
                    data_type: "text".to_string(),
                },
                PipelineStep::Model {
                    id: "step_summarize".to_string(),
                    label: "LLM 提炼绘画提示词".to_string(),
                    model_definition_id: "".to_string(),
                    prompt_template: "请阅读以下文章，为其提炼一段极其生动细腻的纯英文 Midjourney / Stable Diffusion 绘画画面描述，突出构图、光影与艺术风格。直接输出英文 Prompt，不要包含多余解说：\n\n{{input}}".to_string(),
                    system_prompt: Some("你是一位顶级 AI 视觉艺术家与 Prompt 专家。".to_string()),
                    parameters: Map::new(),
                    expected_input_kind: Some("text".to_string()),
                    media_input_variable: None,
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
                    expected_input_kind: Some("image".to_string()),
                    media_input_variable: None,
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
                    data_type: "text".to_string(),
                },
                PipelineStep::Model {
                    id: "step_translate".to_string(),
                    label: "多语言精准翻译".to_string(),
                    model_definition_id: "".to_string(),
                    prompt_template: "请将以下文本翻译为地道、自然的英文，适合作为有声读物旁白朗读，仅返回翻译结果：\n\n{{input}}".to_string(),
                    system_prompt: Some("你是一位专业影视旁白译员。".to_string()),
                    parameters: Map::new(),
                    expected_input_kind: Some("text".to_string()),
                    media_input_variable: None,
                },
                PipelineStep::Model {
                    id: "step_tts".to_string(),
                    label: "自然语音合成 (TTS)".to_string(),
                    model_definition_id: "".to_string(),
                    prompt_template: "{{prev}}".to_string(),
                    system_prompt: None,
                    parameters: Map::new(),
                    expected_input_kind: Some("audio".to_string()),
                    media_input_variable: None,
                },
            ],
            created_at: "".to_string(),
            updated_at: "".to_string(),
            schema_version: 1,
        },
        PipelineDefinition {
            id: "preset_story_to_video".to_string(),
            name: "🎬 故事分镜 → 关键帧概念图 → 动态视频".to_string(),
            description: "输入故事脚本，LLM 提炼分镜概念画面，生成参考关键帧概念图，再由视频模型以关键帧为首帧生成运镜视频片段。".to_string(),
            steps: vec![
                PipelineStep::Input {
                    id: "step_input".to_string(),
                    label: "故事脚本大纲".to_string(),
                    default_value: "一名孤独的宇航员缓步走在未知的红色沙漠星球上，夕阳将他的影子拉得很长，突然地平线处升起两个巨大的银色光环。".to_string(),
                    data_type: "text".to_string(),
                },
                PipelineStep::Model {
                    id: "step_concept_prompt".to_string(),
                    label: "提炼分镜概念画面 Prompt".to_string(),
                    model_definition_id: "".to_string(),
                    prompt_template: "请为以下故事剧本提炼一段电影级概念画面的纯英文 Midjourney / SD 绘画描述：\n\n{{input}}".to_string(),
                    system_prompt: Some("你是一位院线科幻电影美术概念总监。".to_string()),
                    parameters: Map::new(),
                    expected_input_kind: Some("text".to_string()),
                    media_input_variable: None,
                },
                PipelineStep::Model {
                    id: "step_keyframe".to_string(),
                    label: "生成分镜概念关键帧".to_string(),
                    model_definition_id: "".to_string(),
                    prompt_template: "{{prev}}".to_string(),
                    system_prompt: None,
                    parameters: Map::new(),
                    expected_input_kind: Some("image".to_string()),
                    media_input_variable: None,
                },
                PipelineStep::Model {
                    id: "step_shot_prompt".to_string(),
                    label: "设计镜头动态与运镜 Prompt".to_string(),
                    model_definition_id: "".to_string(),
                    prompt_template: "请根据故事脚本，设计一段英文视频镜头运动 Prompt（如 slow cinematic dolly zoom in）：\n故事：{{input}}".to_string(),
                    system_prompt: Some("你是一位专业电影运镜指导。".to_string()),
                    parameters: Map::new(),
                    expected_input_kind: Some("text".to_string()),
                    media_input_variable: None,
                },
                PipelineStep::Model {
                    id: "step_video".to_string(),
                    label: "首帧驱动生成电影镜头视频".to_string(),
                    model_definition_id: "".to_string(),
                    prompt_template: "{{prev}}".to_string(),
                    system_prompt: None,
                    parameters: Map::new(),
                    expected_input_kind: Some("video".to_string()),
                    media_input_variable: Some("{{step_keyframe.media}}".to_string()),
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

    // 1. 断点恢复检查与前置步骤注入
    let resume_idx = input.resume_from_step_index.unwrap_or(0);
    if resume_idx > 0 {
        if resume_idx >= input.steps.len() {
            return Err(AppError::validation("恢复执行的步骤索引超出工作流总步骤数"));
        }
        let cached = input
            .cached_step_results
            .as_ref()
            .ok_or_else(|| AppError::validation("断点恢复执行必须提供已完成步骤的缓存结果"))?;
        if cached.len() < resume_idx {
            return Err(AppError::validation(format!(
                "断点恢复需要前 {resume_idx} 步的执行结果，但仅提供了 {} 项",
                cached.len()
            )));
        }
        for (i, r) in cached.iter().take(resume_idx).enumerate() {
            if r.status != "completed" {
                return Err(AppError::validation(format!(
                    "前置步骤「{}」状态为「{}」，无法跳过，请从该失败步骤开始重新执行",
                    r.step_label, r.status
                )));
            }
            inject_step_result_to_vars(&mut var_map, r, i);
            last_output_text = r.text_output.clone();
            last_media_paths = r.media_paths.clone();
            last_output_kind = r.output_kind.clone();
            step_results.push(r.clone());
            if let Some(cb) = &on_step {
                cb(i, input.steps.len(), r);
            }
        }
    }

    for (index, step) in input.steps.iter().enumerate() {
        if index < resume_idx {
            continue;
        }

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
            PipelineStep::Input {
                default_value,
                data_type,
                ..
            } => {
                let text = if !input.initial_input.trim().is_empty() {
                    input.initial_input.clone()
                } else {
                    default_value.clone()
                };
                last_output_text = Some(text.clone());
                last_media_paths.clear();
                last_output_kind = data_type.clone();

                let step_res = PipelineStepResult {
                    step_id: step_id.clone(),
                    step_label: step_label.clone(),
                    step_type: step_type.clone(),
                    output_kind: data_type.clone(),
                    text_output: Some(text),
                    media_paths: Vec::new(),
                    latency_ms: step_start.elapsed().as_millis() as u64,
                    status: "completed".to_string(),
                    error: None,
                };
                inject_step_result_to_vars(&mut var_map, &step_res, index);
                if let Some(cb) = &on_step {
                    cb(index, input.steps.len(), &step_res);
                }
                step_results.push(step_res);
            }
            PipelineStep::Transform { kind, rule, .. } => {
                let transformed = match kind.as_str() {
                    "template" => substitute_vars_strict(rule, &var_map, &step_label)?,
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
                    _ => substitute_vars_strict(rule, &var_map, &step_label)?,
                };

                last_output_text = Some(transformed.clone());
                last_media_paths.clear();
                last_output_kind = "text".to_string();

                let step_res = PipelineStepResult {
                    step_id: step_id.clone(),
                    step_label: step_label.clone(),
                    step_type: step_type.clone(),
                    output_kind: "text".to_string(),
                    text_output: Some(transformed),
                    media_paths: Vec::new(),
                    latency_ms: step_start.elapsed().as_millis() as u64,
                    status: "completed".to_string(),
                    error: None,
                };
                inject_step_result_to_vars(&mut var_map, &step_res, index);
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
                media_input_variable,
                ..
            } => {
                if model_definition_id.trim().is_empty() {
                    return Err(AppError::validation(format!(
                        "步骤「{step_label}」未配置目标模型"
                    )));
                }

                let effective_prompt =
                    substitute_vars_strict(prompt_template, &var_map, &step_label)?;
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

                        let step_res = PipelineStepResult {
                            step_id: step_id.clone(),
                            step_label: step_label.clone(),
                            step_type: step_type.clone(),
                            output_kind: "text".to_string(),
                            text_output: Some(res.text),
                            media_paths: Vec::new(),
                            latency_ms: latency,
                            status: "completed".to_string(),
                            error: None,
                        };
                        inject_step_result_to_vars(&mut var_map, &step_res, index);
                        if let Some(cb) = &on_step {
                            cb(index, input.steps.len(), &step_res);
                        }
                        step_results.push(step_res);
                    }
                    Modality::Image => {
                        let source_image = if let Some(var_expr) = media_input_variable {
                            let resolved = substitute_vars_strict(var_expr, &var_map, &step_label)?;
                            if !resolved.trim().is_empty() {
                                Some(resolved.trim().to_string())
                            } else {
                                None
                            }
                        } else if !last_media_paths.is_empty() {
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

                        let step_res = PipelineStepResult {
                            step_id: step_id.clone(),
                            step_label: step_label.clone(),
                            step_type: step_type.clone(),
                            output_kind: "image".to_string(),
                            text_output: None,
                            media_paths: paths,
                            latency_ms: latency,
                            status: "completed".to_string(),
                            error: None,
                        };
                        inject_step_result_to_vars(&mut var_map, &step_res, index);
                        if let Some(cb) = &on_step {
                            cb(index, input.steps.len(), &step_res);
                        }
                        step_results.push(step_res);
                    }
                    Modality::Video => {
                        let first_frame = if let Some(var_expr) = media_input_variable {
                            let resolved = substitute_vars_strict(var_expr, &var_map, &step_label)?;
                            if !resolved.trim().is_empty() {
                                Some(resolved.trim().to_string())
                            } else {
                                None
                            }
                        } else if !last_media_paths.is_empty() {
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

                        let step_res = PipelineStepResult {
                            step_id: step_id.clone(),
                            step_label: step_label.clone(),
                            step_type: step_type.clone(),
                            output_kind: "video".to_string(),
                            text_output: None,
                            media_paths: paths,
                            latency_ms: latency,
                            status: "completed".to_string(),
                            error: None,
                        };
                        inject_step_result_to_vars(&mut var_map, &step_res, index);
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

                        let step_res = PipelineStepResult {
                            step_id: step_id.clone(),
                            step_label: step_label.clone(),
                            step_type: step_type.clone(),
                            output_kind: "audio".to_string(),
                            text_output: transcript,
                            media_paths: paths,
                            latency_ms: latency,
                            status: "completed".to_string(),
                            error: None,
                        };
                        inject_step_result_to_vars(&mut var_map, &step_res, index);
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

pub fn inject_step_result_to_vars(
    var_map: &mut HashMap<String, String>,
    res: &PipelineStepResult,
    step_index: usize,
) {
    let main_output = res
        .text_output
        .clone()
        .unwrap_or_else(|| res.media_paths.first().cloned().unwrap_or_default());
    var_map.insert("prev".to_string(), main_output.clone());
    if let Some(txt) = &res.text_output {
        var_map.insert("prev.text".to_string(), txt.clone());
    }
    if let Some(media) = res.media_paths.first() {
        var_map.insert("prev.media".to_string(), media.clone());
    }
    // step_<index>
    var_map.insert(format!("step_{step_index}"), main_output.clone());
    if let Some(txt) = &res.text_output {
        var_map.insert(format!("step_{step_index}.text"), txt.clone());
    }
    if let Some(media) = res.media_paths.first() {
        var_map.insert(format!("step_{step_index}.media"), media.clone());
    }
    // step_id
    var_map.insert(res.step_id.clone(), main_output.clone());
    var_map.insert(format!("{}.output", res.step_id), main_output.clone());
    var_map.insert(format!("step.{}.output", res.step_id), main_output.clone());
    if let Some(txt) = &res.text_output {
        var_map.insert(format!("{}.text", res.step_id), txt.clone());
        var_map.insert(format!("step.{}.text", res.step_id), txt.clone());
    }
    if let Some(media) = res.media_paths.first() {
        var_map.insert(format!("{}.media", res.step_id), media.clone());
        var_map.insert(format!("step.{}.media", res.step_id), media.clone());
    }
}

pub fn substitute_vars(template: &str, vars: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (k, v) in vars {
        let pattern = format!("{{{{{}}}}}", k);
        result = result.replace(&pattern, v);
    }
    result
}

pub fn substitute_vars_strict(
    template: &str,
    vars: &HashMap<String, String>,
    step_label: &str,
) -> Result<String, AppError> {
    let substituted = substitute_vars(template, vars);
    if let Some(unresolved) = find_unresolved_var(&substituted) {
        return Err(AppError::validation(format!(
            "步骤「{step_label}」引用的变量「{{{{{unresolved}}}}}」未定义或前置步骤尚未产出对应数据"
        )));
    }
    Ok(substituted)
}

fn find_unresolved_var(s: &str) -> Option<String> {
    let start = s.find("{{")?;
    let rest = &s[start + 2..];
    let end = rest.find("}}")?;
    let var_name = rest[..end].trim();
    if !var_name.is_empty() {
        Some(var_name.to_string())
    } else {
        None
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_validation_rules() {
        // 1. 空名字
        let invalid_empty_name = PipelineDefinition {
            id: "p1".to_string(),
            name: "   ".to_string(),
            description: "".to_string(),
            steps: vec![PipelineStep::Input {
                id: "step1".to_string(),
                label: "input".to_string(),
                default_value: "".to_string(),
                data_type: "text".to_string(),
            }],
            created_at: "".to_string(),
            updated_at: "".to_string(),
            schema_version: 1,
        };
        assert!(validate_pipeline_definition(&invalid_empty_name).is_err());

        // 2. 空步骤
        let invalid_no_steps = PipelineDefinition {
            id: "p2".to_string(),
            name: "Valid Name".to_string(),
            description: "".to_string(),
            steps: vec![],
            created_at: "".to_string(),
            updated_at: "".to_string(),
            schema_version: 1,
        };
        assert!(validate_pipeline_definition(&invalid_no_steps).is_err());

        // 3. 重复步骤 ID
        let duplicate_id = PipelineDefinition {
            id: "p3".to_string(),
            name: "Valid Name".to_string(),
            description: "".to_string(),
            steps: vec![
                PipelineStep::Input {
                    id: "step_same".to_string(),
                    label: "input".to_string(),
                    default_value: "".to_string(),
                    data_type: "text".to_string(),
                },
                PipelineStep::Transform {
                    id: "step_same".to_string(),
                    label: "transform".to_string(),
                    kind: "template".to_string(),
                    rule: "{{prev}}".to_string(),
                },
            ],
            created_at: "".to_string(),
            updated_at: "".to_string(),
            schema_version: 1,
        };
        assert!(validate_pipeline_definition(&duplicate_id).is_err());

        // 4. 合法预设均能通过校验
        for p in presets() {
            assert!(validate_pipeline_definition(&p).is_ok());
        }
    }

    #[test]
    fn test_pipeline_json_export_and_import() {
        let p = &presets()[2]; // story to video
        let exported = export_pipeline_json(p).expect("export should succeed");
        assert!(exported.contains("preset_story_to_video"));
        assert!(exported.contains("step_keyframe"));

        let imported = import_pipeline_json(&exported).expect("import should succeed");
        assert_eq!(imported.id, p.id);
        assert_eq!(imported.steps.len(), p.steps.len());

        // 畸变 JSON 拦截
        assert!(import_pipeline_json(r#"{"name": "broken"}"#).is_err());
    }

    #[test]
    fn test_variable_interpolation_and_strict_check() {
        let mut vars = HashMap::new();
        vars.insert("input".to_string(), "Cyberpunk city".to_string());
        vars.insert("prev".to_string(), "Masterpiece visual".to_string());
        vars.insert(
            "step_1.media".to_string(),
            "C:/images/frame.png".to_string(),
        );

        // 正常替换
        let substituted = substitute_vars_strict(
            "Style: {{prev}}, based on {{input}} with frame {{step_1.media}}",
            &vars,
            "test_step",
        )
        .unwrap();
        assert_eq!(
            substituted,
            "Style: Masterpiece visual, based on Cyberpunk city with frame C:/images/frame.png"
        );

        // 引用未定义变量报错拦截
        let err = substitute_vars_strict("Hello {{undefined_var}}", &vars, "测试步骤").unwrap_err();
        assert!(err.message.contains("undefined_var"));
    }

    #[test]
    fn test_inject_step_result_to_vars() {
        let mut vars = HashMap::new();
        let step_res = PipelineStepResult {
            step_id: "step_keyframe".to_string(),
            step_label: "概念关键帧".to_string(),
            step_type: "model".to_string(),
            output_kind: "image".to_string(),
            text_output: None,
            media_paths: vec!["/path/to/frame01.png".to_string()],
            latency_ms: 1200,
            status: "completed".to_string(),
            error: None,
        };

        inject_step_result_to_vars(&mut vars, &step_res, 1);
        assert_eq!(vars.get("prev").unwrap(), "/path/to/frame01.png");
        assert_eq!(vars.get("prev.media").unwrap(), "/path/to/frame01.png");
        assert_eq!(vars.get("step_1").unwrap(), "/path/to/frame01.png");
        assert_eq!(vars.get("step_1.media").unwrap(), "/path/to/frame01.png");
        assert_eq!(vars.get("step_keyframe").unwrap(), "/path/to/frame01.png");
        assert_eq!(
            vars.get("step_keyframe.media").unwrap(),
            "/path/to/frame01.png"
        );
        assert_eq!(
            vars.get("step.step_keyframe.media").unwrap(),
            "/path/to/frame01.png"
        );
    }

    #[tokio::test]
    async fn test_pipeline_resume_execution() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        let http = reqwest::Client::new();
        let secrets = crate::credentials::MemorySecretStore::default();
        let temp_dir = std::env::temp_dir();

        let steps = vec![
            PipelineStep::Input {
                id: "step_input".to_string(),
                label: "输入".to_string(),
                default_value: "hello".to_string(),
                data_type: "text".to_string(),
            },
            PipelineStep::Transform {
                id: "step_cached".to_string(),
                label: "前置缓存步骤".to_string(),
                kind: "template".to_string(),
                rule: "{{input}} world".to_string(),
            },
            PipelineStep::Transform {
                id: "step_resumed".to_string(),
                label: "恢复执行步骤".to_string(),
                kind: "template".to_string(),
                rule: "{{prev}} - restored successfully".to_string(),
            },
        ];

        // 模拟前两步已经成功完成的结果
        let cached = vec![
            PipelineStepResult {
                step_id: "step_input".to_string(),
                step_label: "输入".to_string(),
                step_type: "input".to_string(),
                output_kind: "text".to_string(),
                text_output: Some("hello".to_string()),
                media_paths: Vec::new(),
                latency_ms: 10,
                status: "completed".to_string(),
                error: None,
            },
            PipelineStepResult {
                step_id: "step_cached".to_string(),
                step_label: "前置缓存步骤".to_string(),
                step_type: "transform".to_string(),
                output_kind: "text".to_string(),
                text_output: Some("hello world".to_string()),
                media_paths: Vec::new(),
                latency_ms: 20,
                status: "completed".to_string(),
                error: None,
            },
        ];

        let input = PipelineExecutionInput {
            pipeline_id: None,
            steps,
            initial_input: "hello".to_string(),
            resume_from_step_index: Some(2),
            cached_step_results: Some(cached),
        };

        let res = run_with_cancel_and_callback(
            &pool, &http, &secrets, &temp_dir, input, None, None, None,
        )
        .await
        .unwrap();

        assert_eq!(res.step_results.len(), 3);
        assert_eq!(res.step_results[0].text_output.as_deref(), Some("hello"));
        assert_eq!(
            res.step_results[1].text_output.as_deref(),
            Some("hello world")
        );
        assert_eq!(
            res.step_results[2].text_output.as_deref(),
            Some("hello world - restored successfully")
        );
        assert_eq!(
            res.final_text.as_deref(),
            Some("hello world - restored successfully")
        );
    }

    #[tokio::test]
    async fn test_invalid_resume_rejected() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        let http = reqwest::Client::new();
        let secrets = crate::credentials::MemorySecretStore::default();
        let temp_dir = std::env::temp_dir();

        let steps = vec![
            PipelineStep::Input {
                id: "step_input".to_string(),
                label: "输入".to_string(),
                default_value: "hello".to_string(),
                data_type: "text".to_string(),
            },
            PipelineStep::Transform {
                id: "step_failed".to_string(),
                label: "失败步骤".to_string(),
                kind: "template".to_string(),
                rule: "{{input}}".to_string(),
            },
        ];

        // 尝试跳过失败的步骤
        let cached = vec![PipelineStepResult {
            step_id: "step_input".to_string(),
            step_label: "输入".to_string(),
            step_type: "input".to_string(),
            output_kind: "text".to_string(),
            text_output: None,
            media_paths: Vec::new(),
            latency_ms: 0,
            status: "failed".to_string(),
            error: Some("网络错误".to_string()),
        }];

        let input = PipelineExecutionInput {
            pipeline_id: None,
            steps,
            initial_input: "hello".to_string(),
            resume_from_step_index: Some(1),
            cached_step_results: Some(cached),
        };

        let err = run_with_cancel_and_callback(
            &pool, &http, &secrets, &temp_dir, input, None, None, None,
        )
        .await
        .unwrap_err();

        assert!(err.message.contains("无法跳过"));
    }
}
