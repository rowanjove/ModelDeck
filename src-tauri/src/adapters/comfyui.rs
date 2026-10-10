use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{json, Map, Value};
use uuid::Uuid;

use super::ProviderAdapter;
use crate::canonical::{
    AdapterContext, CanonicalRequest, CanonicalResponse, ConnectionReport, ImageBinary,
    ImagePayload, ImageResult, RemoteModel, VideoBinary, VideoPayload, VideoResult,
};
use crate::domain::{ModelDefinition, WorkflowBinding};
use crate::error::AppError;
use crate::http_util::{self, join_url};
use crate::media;
use crate::redact::shrink_json;

pub struct ComfyUiAdapter;

pub const COMFYUI: ComfyUiAdapter = ComfyUiAdapter;

#[async_trait]
impl ProviderAdapter for ComfyUiAdapter {
    async fn test_connection(&self, ctx: &AdapterContext) -> Result<ConnectionReport, AppError> {
        let started = Instant::now();
        let (_status, _body) = get_json(ctx, "system_stats", None).await?;
        Ok(ConnectionReport {
            latency_ms: elapsed_ms(started),
            message: "已连接".to_string(),
        })
    }

    async fn list_models(&self, ctx: &AdapterContext) -> Result<Vec<RemoteModel>, AppError> {
        let (_status, body) = get_json(ctx, "models/checkpoints", None).await?;
        let names = body
            .as_array()
            .ok_or_else(|| AppError::unsupported("ComfyUI 没有返回 checkpoint 列表"))?;
        let mut models: Vec<RemoteModel> = names
            .iter()
            .filter_map(|item| {
                let name = item.as_str()?.trim();
                if name.is_empty() {
                    None
                } else {
                    Some(RemoteModel {
                        id: name.to_string(),
                        name: name.to_string(),
                    })
                }
            })
            .take(2000)
            .collect();
        models.sort_by(|left, right| left.id.cmp(&right.id));
        models.dedup_by(|left, right| left.id == right.id);
        Ok(models)
    }

    async fn execute(
        &self,
        ctx: &AdapterContext,
        request: CanonicalRequest,
    ) -> Result<CanonicalResponse, AppError> {
        match request {
            CanonicalRequest::Image(payload) => execute_image(ctx, payload).await,
            CanonicalRequest::Video(payload) => execute_video(ctx, payload).await,
            CanonicalRequest::Audio(_) => Err(AppError::unsupported(
                "ComfyUI 适配器目前不执行音频生成",
            )),
            CanonicalRequest::Text(_) => Err(AppError::unsupported(
                "文本生成目前由 OpenAI、OpenAI Compatible 与 Ollama 适配器执行，ComfyUI 已可保存并测试连接",
            )),
        }
    }
}

async fn execute_image(
    ctx: &AdapterContext,
    payload: ImagePayload,
) -> Result<CanonicalResponse, AppError> {
    let model = ctx
        .model
        .as_ref()
        .ok_or_else(|| AppError::validation("缺少模型定义"))?;
    let started = Instant::now();
    let (mut workflow, bindings) = workflow_of(model)?;
    if !payload.prompt.is_empty() && !bindings.iter().any(|item| item.key == "prompt") {
        return Err(AppError::validation("工作流没有绑定提示词"));
    }
    let uploads = upload_bound(ctx, model, &image_files(&payload), &bindings).await?;
    apply_bindings(
        &mut workflow,
        &bindings,
        &BoundText {
            prompt: &payload.prompt,
            negative: payload.negative_prompt.as_deref(),
            parameters: &payload.parameters,
            uploads: &uploads,
        },
    )?;
    let client_id = Uuid::new_v4().to_string();
    let mut request_body = json!({
        "prompt": workflow,
        "client_id": client_id,
    });
    if let Some(extra) = &payload.extra {
        let extra = extra
            .as_object()
            .ok_or_else(|| AppError::validation("Extra JSON 必须是对象"))?;
        let object = request_body
            .as_object_mut()
            .ok_or_else(|| AppError::storage("无法构造请求"))?;
        for (key, value) in extra {
            if key == "prompt" || key == "client_id" {
                return Err(AppError::validation(
                    "Extra JSON 不能覆盖 prompt 或 client_id",
                ));
            }
            object.insert(key.clone(), value.clone());
        }
    }
    let raw_request = shrink_json(&json!({
        "method": "POST",
        "url": join_url(&ctx.provider.base_url, "prompt")?,
        "body": request_body,
    }));
    let (status, submitted) = post_json(ctx, "prompt", &request_body, Some(&model.name)).await?;
    if submitted
        .get("node_errors")
        .and_then(Value::as_object)
        .is_some_and(|errors| !errors.is_empty())
    {
        return Err(AppError::provider(
            "ComfyUI 拒绝了工作流",
            status,
            &ctx.provider.name,
            Some(&model.name),
            Some(shrink_json(&submitted)),
        ));
    }
    let prompt_id = submitted
        .get("prompt_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .ok_or_else(|| {
            AppError::provider(
                "ComfyUI 没有返回任务编号",
                status,
                &ctx.provider.name,
                Some(&model.name),
                Some(shrink_json(&submitted)),
            )
        })?;
    let history = poll_history(ctx, model, prompt_id, MediaExpect::Image).await?;
    let views = output_views(&history, prompt_id, MediaExpect::Image).ok_or_else(|| {
        AppError::provider(
            "ComfyUI 没有返回图像",
            200,
            &ctx.provider.name,
            Some(&model.name),
            Some(shrink_json(&history)),
        )
    })?;
    let mut binaries = Vec::new();
    for view in views.into_iter().take(8) {
        let bytes = download_view(ctx, model, &view).await?;
        binaries.push(binary_from(bytes)?);
    }
    if binaries.is_empty() {
        return Err(AppError::provider(
            "ComfyUI 没有返回图像",
            200,
            &ctx.provider.name,
            Some(&model.name),
            Some(shrink_json(&history)),
        ));
    }
    Ok(CanonicalResponse::Image(ImageResult {
        images: Vec::new(),
        latency_ms: elapsed_ms(started),
        provider_id: ctx.provider.id.clone(),
        provider_name: ctx.provider.name.clone(),
        model_definition_id: model.id.clone(),
        model_name: model.name.clone(),
        model_id: model.model_id.clone(),
        raw_request,
        raw_response: shrink_json(&history),
        status,
        binaries,
    }))
}

async fn execute_video(
    ctx: &AdapterContext,
    payload: VideoPayload,
) -> Result<CanonicalResponse, AppError> {
    let model = ctx
        .model
        .as_ref()
        .ok_or_else(|| AppError::validation("缺少模型定义"))?;
    let started = Instant::now();
    let (mut workflow, bindings) = workflow_of(model)?;
    if !payload.prompt.is_empty() && !bindings.iter().any(|item| item.key == "prompt") {
        return Err(AppError::validation("工作流没有绑定提示词"));
    }
    let uploads = upload_bound(ctx, model, &video_files(&payload), &bindings).await?;
    apply_bindings(
        &mut workflow,
        &bindings,
        &BoundText {
            prompt: &payload.prompt,
            negative: payload.negative_prompt.as_deref(),
            parameters: &payload.parameters,
            uploads: &uploads,
        },
    )?;
    let client_id = Uuid::new_v4().to_string();
    let mut request_body = json!({
        "prompt": workflow,
        "client_id": client_id,
    });
    if let Some(extra) = &payload.extra {
        let extra = extra
            .as_object()
            .ok_or_else(|| AppError::validation("Extra JSON 必须是对象"))?;
        let object = request_body
            .as_object_mut()
            .ok_or_else(|| AppError::storage("无法构造请求"))?;
        for (key, value) in extra {
            if key == "prompt" || key == "client_id" {
                return Err(AppError::validation(
                    "Extra JSON 不能覆盖 prompt 或 client_id",
                ));
            }
            object.insert(key.clone(), value.clone());
        }
    }
    let raw_request = shrink_json(&json!({
        "method": "POST",
        "url": join_url(&ctx.provider.base_url, "prompt")?,
        "body": request_body,
    }));
    let (status, submitted) = post_json(ctx, "prompt", &request_body, Some(&model.name)).await?;
    if submitted
        .get("node_errors")
        .and_then(Value::as_object)
        .is_some_and(|errors| !errors.is_empty())
    {
        return Err(AppError::provider(
            "ComfyUI 拒绝了工作流",
            status,
            &ctx.provider.name,
            Some(&model.name),
            Some(shrink_json(&submitted)),
        ));
    }
    let prompt_id = submitted
        .get("prompt_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .ok_or_else(|| {
            AppError::provider(
                "ComfyUI 没有返回任务编号",
                status,
                &ctx.provider.name,
                Some(&model.name),
                Some(shrink_json(&submitted)),
            )
        })?;
    let history = poll_history(ctx, model, prompt_id, MediaExpect::Video).await?;
    let views = output_views(&history, prompt_id, MediaExpect::Video).ok_or_else(|| {
        AppError::provider(
            "ComfyUI 没有返回视频",
            200,
            &ctx.provider.name,
            Some(&model.name),
            Some(shrink_json(&history)),
        )
    })?;
    let mut binaries = Vec::new();
    for view in views.into_iter().take(8) {
        let bytes = download_view(ctx, model, &view).await?;
        if let Ok(binary) = video_binary(bytes) {
            binaries.push(binary);
        }
    }
    if binaries.is_empty() {
        return Err(AppError::provider(
            "ComfyUI 没有返回视频",
            200,
            &ctx.provider.name,
            Some(&model.name),
            Some(shrink_json(&history)),
        ));
    }
    Ok(CanonicalResponse::Video(VideoResult {
        videos: Vec::new(),
        latency_ms: elapsed_ms(started),
        provider_id: ctx.provider.id.clone(),
        provider_name: ctx.provider.name.clone(),
        model_definition_id: model.id.clone(),
        model_name: model.name.clone(),
        model_id: model.model_id.clone(),
        raw_request,
        raw_response: shrink_json(&history),
        status,
        binaries,
    }))
}

fn workflow_of(model: &ModelDefinition) -> Result<(Value, Vec<WorkflowBinding>), AppError> {
    let transport = model.transport.as_ref();
    let workflow = transport
        .and_then(|item| item.workflow.clone())
        .filter(Value::is_object)
        .ok_or_else(|| AppError::validation("模型没有 ComfyUI Workflow"))?;
    let bindings = transport
        .and_then(|item| item.bindings.clone())
        .unwrap_or_default();
    Ok((workflow, bindings))
}

struct BoundText<'a> {
    prompt: &'a str,
    negative: Option<&'a str>,
    parameters: &'a Map<String, Value>,
    uploads: &'a BTreeMap<String, String>,
}

fn image_files(payload: &ImagePayload) -> Vec<(String, String)> {
    let mut files = Vec::new();
    if let Some(image) = &payload.source_image {
        files.push(("sourceImage".to_string(), image.path.clone()));
    }
    if let Some(image) = &payload.mask {
        files.push(("mask".to_string(), image.path.clone()));
    }
    for (index, image) in payload.reference_images.iter().enumerate() {
        files.push((format!("referenceImages[{index}]"), image.path.clone()));
    }
    files
}

fn video_files(payload: &VideoPayload) -> Vec<(String, String)> {
    let mut files = Vec::new();
    if let Some(image) = &payload.first_frame {
        files.push(("firstFrame".to_string(), image.path.clone()));
    }
    if let Some(image) = &payload.last_frame {
        files.push(("lastFrame".to_string(), image.path.clone()));
    }
    for (index, image) in payload.reference_images.iter().enumerate() {
        files.push((format!("referenceImages[{index}]"), image.path.clone()));
    }
    for (index, item) in payload.reference_videos.iter().enumerate() {
        files.push((format!("referenceVideos[{index}]"), item.path.clone()));
    }
    for (index, item) in payload.reference_audios.iter().enumerate() {
        files.push((format!("referenceAudios[{index}]"), item.path.clone()));
    }
    files
}

async fn upload_bound(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    files: &[(String, String)],
    bindings: &[WorkflowBinding],
) -> Result<BTreeMap<String, String>, AppError> {
    let mut uploads = BTreeMap::new();
    for (key, path) in files {
        if bindings.iter().any(|item| item.key == *key) {
            let uploaded = if key.starts_with("referenceVideos") {
                upload_reference(ctx, model, path, true).await?
            } else if key.starts_with("referenceAudios") {
                upload_reference(ctx, model, path, false).await?
            } else {
                upload_image(ctx, model, path).await?
            };
            uploads.insert(key.clone(), uploaded);
        }
    }
    Ok(uploads)
}

async fn upload_reference(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    path: &str,
    video: bool,
) -> Result<String, AppError> {
    let (limit, label, too_large) = if video {
        (
            crate::media::MAX_VIDEO_PREVIEW_BYTES,
            "参考视频",
            "参考视频超过 32MB",
        )
    } else {
        (
            crate::media::MAX_AUDIO_PREVIEW_BYTES,
            "参考音频",
            "参考音频超过 20MB",
        )
    };
    let bytes = crate::http_util::read_file_bounded_sync(std::path::Path::new(path), limit)
        .map_err(|err| match err {
            crate::http_util::BoundedFileReadError::TooLarge => AppError::validation(too_large),
            crate::http_util::BoundedFileReadError::Io(err) => {
                AppError::validation(format!("无法读取{label}: {err}"))
            }
        })?;
    let mime = if video {
        media::sniff_video(&bytes).ok_or_else(|| AppError::validation("参考视频格式不受支持"))?
    } else {
        media::sniff_audio(&bytes).ok_or_else(|| AppError::validation("参考音频格式不受支持"))?
    };
    post_upload(ctx, model, mime, bytes).await
}

async fn upload_image(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    path: &str,
) -> Result<String, AppError> {
    let (mime, bytes) = crate::adapters::openai::data_url_parts(path)?;
    post_upload(ctx, model, mime, bytes).await
}

async fn post_upload(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    mime: &str,
    bytes: Vec<u8>,
) -> Result<String, AppError> {
    let filename = format!("{}.{}", Uuid::new_v4(), media::extension_for(mime));
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename.clone())
        .mime_str(mime)
        .map_err(|_| AppError::validation("文件类型无效"))?;
    let form = reqwest::multipart::Form::new()
        .text("type", "input")
        .part("image", part);
    let url = join_url(&ctx.provider.base_url, "upload/image")?;
    let response = http_util::send(
        ctx.http.post(url).multipart(form),
        Duration::from_millis(ctx.provider.timeout_ms),
        &ctx.provider.name,
        Some(&model.name),
    )
    .await?;
    let (_status, body) =
        http_util::read_response(response, &ctx.provider.name, Some(&model.name)).await?;
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .ok_or_else(|| {
            AppError::provider(
                "ComfyUI 没有返回上传文件名",
                200,
                &ctx.provider.name,
                Some(&model.name),
                Some(shrink_json(&body)),
            )
        })?;
    let subfolder = body
        .get("subfolder")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if subfolder.is_empty() {
        Ok(name.to_string())
    } else {
        Ok(format!("{subfolder}/{name}"))
    }
}

fn apply_bindings(
    workflow: &mut Value,
    bindings: &[WorkflowBinding],
    text: &BoundText<'_>,
) -> Result<(), AppError> {
    for binding in bindings {
        let Some(value) = binding_value(&binding.key, text) else {
            continue;
        };
        let node = workflow
            .get_mut(&binding.node)
            .ok_or_else(|| AppError::validation(format!("工作流没有节点 {}", binding.node)))?;
        let inputs = node
            .get_mut("inputs")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| AppError::validation(format!("节点 {} 没有 inputs", binding.node)))?;
        inputs.insert(binding.input.clone(), value);
    }
    Ok(())
}

fn binding_value(key: &str, text: &BoundText<'_>) -> Option<Value> {
    if key == "prompt" {
        return (!text.prompt.is_empty()).then(|| Value::String(text.prompt.to_string()));
    }
    if key == "negativePrompt" {
        return text
            .negative
            .filter(|item| !item.is_empty())
            .map(|item| Value::String(item.to_string()));
    }
    if let Some(name) = text.uploads.get(key) {
        return Some(Value::String(name.clone()));
    }
    text.parameters.get(key).cloned()
}

#[derive(Clone, Copy)]
enum MediaExpect {
    Image,
    Video,
}

async fn cancel_comfyui_job(ctx: &AdapterContext, model_name: &str, prompt_id: &str) {
    if let Ok((_status, queue)) = get_json(ctx, "queue", Some(model_name)).await {
        let is_running = queue
            .get("queue_running")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .any(|item| item.get(1).and_then(Value::as_str) == Some(prompt_id))
            })
            .unwrap_or(false);

        if is_running {
            let _ = post_json(ctx, "interrupt", &json!({}), Some(model_name)).await;
        } else {
            let _ = post_json(
                ctx,
                "queue",
                &json!({ "delete": [prompt_id] }),
                Some(model_name),
            )
            .await;
        }
    }
}

async fn poll_history(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    prompt_id: &str,
    expect: MediaExpect,
) -> Result<Value, AppError> {
    let budget = ctx
        .provider
        .timeout_ms
        .saturating_mul(40)
        .clamp(120_000, 600_000);
    let started = Instant::now();
    let path = format!("history/{prompt_id}");
    loop {
        if ctx.cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            cancel_comfyui_job(ctx, &model.name, prompt_id).await;
            return Err(AppError::validation("任务已取消"));
        }
        if started.elapsed() >= Duration::from_millis(budget) {
            return Err(AppError::timeout(&ctx.provider.name, Some(&model.name)));
        }
        let (_status, body) = get_json(ctx, &path, Some(&model.name)).await?;
        match history_state(&body, prompt_id, expect) {
            HistoryState::Pending => {
                tokio::select! {
                    _ = super::exchange::wait_for_cancel(&ctx.cancelled) => {
                        return Err(AppError::validation("任务已取消"));
                    }
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                }
            }
            HistoryState::Failed => {
                return Err(AppError::provider(
                    "ComfyUI 执行失败",
                    200,
                    &ctx.provider.name,
                    Some(&model.name),
                    Some(shrink_json(&body)),
                ));
            }
            HistoryState::Ready => return Ok(body),
            HistoryState::Empty => {
                let message = match expect {
                    MediaExpect::Image => "ComfyUI 没有返回图像",
                    MediaExpect::Video => "ComfyUI 没有返回视频",
                };
                return Err(AppError::provider(
                    message,
                    200,
                    &ctx.provider.name,
                    Some(&model.name),
                    Some(shrink_json(&body)),
                ));
            }
        }
    }
}

enum HistoryState {
    Pending,
    Failed,
    Ready,
    Empty,
}

fn history_state(body: &Value, prompt_id: &str, expect: MediaExpect) -> HistoryState {
    let Some(entry) = body.get(prompt_id) else {
        return HistoryState::Pending;
    };
    let status = entry
        .pointer("/status/status_str")
        .and_then(Value::as_str)
        .unwrap_or("");
    if status == "error" {
        return HistoryState::Failed;
    }
    if output_views(body, prompt_id, expect).is_some() {
        return HistoryState::Ready;
    }
    let completed = entry
        .pointer("/status/completed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if completed || status == "success" {
        HistoryState::Empty
    } else {
        HistoryState::Pending
    }
}

struct ViewRef {
    filename: String,
    subfolder: String,
    kind: String,
}

fn output_views(body: &Value, prompt_id: &str, expect: MediaExpect) -> Option<Vec<ViewRef>> {
    let outputs = body.get(prompt_id)?.get("outputs")?.as_object()?;
    let keys: &[&str] = match expect {
        MediaExpect::Image => &["images"],
        MediaExpect::Video => &["gifs", "videos", "images"],
    };
    let mut views = Vec::new();
    for node in outputs.values() {
        for key in keys {
            let Some(list) = node.get(*key).and_then(Value::as_array) else {
                continue;
            };
            for item in list {
                let Some(filename) = item.get("filename").and_then(Value::as_str) else {
                    continue;
                };
                if filename.trim().is_empty() {
                    continue;
                }
                if matches!(expect, MediaExpect::Video) && !video_name(filename) {
                    continue;
                }
                views.push(ViewRef {
                    filename: filename.to_string(),
                    subfolder: item
                        .get("subfolder")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    kind: item
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("output")
                        .to_string(),
                });
            }
        }
    }
    if views.is_empty() {
        None
    } else {
        Some(views)
    }
}

fn video_name(filename: &str) -> bool {
    let name = filename.to_ascii_lowercase();
    name.ends_with(".mp4")
        || name.ends_with(".webm")
        || name.ends_with(".mov")
        || name.ends_with(".mkv")
        || name.ends_with(".gif")
}

fn video_binary(bytes: Vec<u8>) -> Result<VideoBinary, AppError> {
    let mime_type = media::describe_video(&bytes)
        .map_err(|message| AppError::new(crate::error::PROVIDER, message))?;
    Ok(VideoBinary {
        bytes,
        mime_type: mime_type.to_string(),
    })
}

async fn download_view(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    view: &ViewRef,
) -> Result<Vec<u8>, AppError> {
    let mut url = reqwest::Url::parse(&join_url(&ctx.provider.base_url, "view")?)
        .map_err(|_| AppError::validation("请求地址不合法"))?;
    url.query_pairs_mut()
        .append_pair("filename", &view.filename)
        .append_pair("subfolder", &view.subfolder)
        .append_pair("type", &view.kind);
    let response = http_util::send(
        ctx.http.get(url),
        Duration::from_millis(ctx.provider.timeout_ms),
        &ctx.provider.name,
        Some(&model.name),
    )
    .await?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let text = String::from_utf8_lossy(
            &http_util::read_body(response, &ctx.provider.name, Some(&model.name)).await?,
        )
        .into_owned();
        return Err(http_util::error_from_status(
            status,
            &text,
            &ctx.provider.name,
            Some(&model.name),
        ));
    }
    tokio::select! {
        _ = super::exchange::wait_for_cancel(&ctx.cancelled) => {
            Err(AppError::validation("任务已取消"))
        }
        bytes = http_util::read_body_with_limit(
            response,
            crate::media::MAX_IMAGE_BYTES,
            &ctx.provider.name,
            Some(&model.name),
        ) => bytes,
    }
}

fn binary_from(bytes: Vec<u8>) -> Result<ImageBinary, AppError> {
    let (mime_type, width, height) = media::describe(&bytes)
        .map_err(|message| AppError::new(crate::error::PROVIDER, message))?;
    Ok(ImageBinary {
        bytes,
        mime_type,
        width,
        height,
    })
}

async fn get_json(
    ctx: &AdapterContext,
    path: &str,
    model: Option<&str>,
) -> Result<(u16, Value), AppError> {
    let url = join_url(&ctx.provider.base_url, path)?;
    let response = http_util::send(
        ctx.http.get(url),
        Duration::from_millis(ctx.provider.timeout_ms),
        &ctx.provider.name,
        model,
    )
    .await?;
    http_util::read_response(response, &ctx.provider.name, model).await
}

async fn post_json(
    ctx: &AdapterContext,
    path: &str,
    body: &Value,
    model: Option<&str>,
) -> Result<(u16, Value), AppError> {
    let url = join_url(&ctx.provider.base_url, path)?;
    let response = http_util::send(
        ctx.http.post(url).json(body),
        Duration::from_millis(ctx.provider.timeout_ms),
        &ctx.provider.name,
        model,
    )
    .await?;
    http_util::read_response(response, &ctx.provider.name, model).await
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}
