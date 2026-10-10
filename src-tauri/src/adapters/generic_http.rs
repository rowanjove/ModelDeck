use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{Map, Value};

use crate::canonical::{
    AdapterContext, AudioBinary, AudioPayload, AudioResult, CanonicalResponse, VideoBinary,
    VideoPayload, VideoResult,
};
use crate::domain::{HttpRecipe, ModelDefinition};
use crate::error::AppError;
use crate::http_util::{self, join_url};
use crate::media;
use crate::redact::{redact_header_map, redact_url, shrink_json};

pub async fn execute(
    ctx: &AdapterContext,
    payload: VideoPayload,
) -> Result<CanonicalResponse, AppError> {
    let model = ctx
        .model
        .as_ref()
        .ok_or_else(|| AppError::validation("缺少模型定义"))?;
    let recipe = model
        .transport
        .as_ref()
        .and_then(|item| item.http.clone())
        .ok_or_else(|| AppError::validation("模型没有 HTTP 请求模板"))?;
    let started = Instant::now();
    let tokens = tokens(ctx, model, &payload).await?;
    let template = recipe.body.clone().unwrap_or(Value::Null);
    let mut body = substitute(&template, &tokens);
    let mut logged_body = substitute(&template, &logged_tokens(&tokens));
    merge_extra(&mut body, payload.extra.as_ref())?;
    merge_extra(&mut logged_body, payload.extra.as_ref())?;
    let headers = live_headers(ctx, &recipe, &tokens);
    let kind = recipe.result_kind.as_deref().unwrap_or("url");
    if kind == "binary" {
        let (status, bytes, raw_request) = call_bytes(
            ctx,
            model,
            &recipe.method,
            &recipe.path,
            &headers,
            &body,
            &logged_body,
        )
        .await?;
        let mime_type = media::describe_video(&bytes)
            .map_err(|message| {
                AppError::provider(
                    message,
                    status,
                    &ctx.provider.name,
                    Some(&model.name),
                    Some(serde_json::json!({ "bytes": bytes.len() })),
                )
            })?
            .to_string();
        return Ok(CanonicalResponse::Video(VideoResult {
            videos: Vec::new(),
            latency_ms: elapsed_ms(started),
            provider_id: ctx.provider.id.clone(),
            provider_name: ctx.provider.name.clone(),
            model_definition_id: model.id.clone(),
            model_name: model.name.clone(),
            model_id: model.model_id.clone(),
            raw_request,
            raw_response: serde_json::json!({ "bytes": bytes.len(), "mimeType": mime_type }),
            status,
            binaries: vec![VideoBinary { bytes, mime_type }],
        }));
    }
    let (status, submitted, raw_request) = call(
        ctx,
        model,
        &recipe.method,
        &recipe.path,
        &headers,
        &body,
        &logged_body,
    )
    .await?;
    let final_body = if let Some(poll) = &recipe.poll {
        let task_id = task_id_of(&submitted, recipe.task_id_path.as_deref(), model, ctx)?;
        poll_until(ctx, model, &recipe, poll, &task_id, &tokens).await?
    } else {
        submitted
    };
    let found = at(&final_body, &recipe.result_path).ok_or_else(|| {
        AppError::provider(
            "响应里没有视频",
            status,
            &ctx.provider.name,
            Some(&model.name),
            Some(shrink_json(&final_body)),
        )
    })?;
    let kind = recipe.result_kind.as_deref().unwrap_or("url");
    let bytes = if kind == "base64" {
        decode_media(found, "视频")
            .map_err(|err| err.context(&ctx.provider.name, Some(&model.name)))?
    } else {
        let url = found.as_str().ok_or_else(|| {
            AppError::provider(
                "视频地址不是字符串",
                status,
                &ctx.provider.name,
                Some(&model.name),
                Some(shrink_json(&final_body)),
            )
        })?;
        download(ctx, model, url, "视频").await?
    };
    let mime_type = media::describe_video(&bytes)
        .map_err(|message| {
            AppError::provider(
                message,
                status,
                &ctx.provider.name,
                Some(&model.name),
                Some(shrink_json(&final_body)),
            )
        })?
        .to_string();
    Ok(CanonicalResponse::Video(VideoResult {
        videos: Vec::new(),
        latency_ms: elapsed_ms(started),
        provider_id: ctx.provider.id.clone(),
        provider_name: ctx.provider.name.clone(),
        model_definition_id: model.id.clone(),
        model_name: model.name.clone(),
        model_id: model.model_id.clone(),
        raw_request,
        raw_response: shrink_json(&final_body),
        status,
        binaries: vec![VideoBinary { bytes, mime_type }],
    }))
}

pub async fn execute_audio(
    ctx: &AdapterContext,
    payload: AudioPayload,
) -> Result<CanonicalResponse, AppError> {
    let model = ctx
        .model
        .as_ref()
        .ok_or_else(|| AppError::validation("缺少模型定义"))?;
    let recipe = model
        .transport
        .as_ref()
        .and_then(|item| item.http.clone())
        .ok_or_else(|| AppError::validation("模型没有 HTTP 请求模板"))?;
    let started = Instant::now();
    let tokens = audio_tokens(ctx, model, &payload)?;
    let template = recipe.body.clone().unwrap_or(Value::Null);
    let mut body = substitute(&template, &tokens);
    let mut logged_body = substitute(&template, &logged_tokens(&tokens));
    merge_extra(&mut body, payload.extra.as_ref())?;
    merge_extra(&mut logged_body, payload.extra.as_ref())?;
    let headers = live_headers(ctx, &recipe, &tokens);
    let kind = recipe.result_kind.as_deref().unwrap_or("url");
    if kind == "text" {
        let (status, submitted, raw_request) = call(
            ctx,
            model,
            &recipe.method,
            &recipe.path,
            &headers,
            &body,
            &logged_body,
        )
        .await?;
        let final_body = if let Some(poll) = &recipe.poll {
            let task_id = task_id_of(&submitted, recipe.task_id_path.as_deref(), model, ctx)?;
            poll_until(ctx, model, &recipe, poll, &task_id, &tokens).await?
        } else {
            submitted
        };
        let found = at(&final_body, &recipe.result_path).ok_or_else(|| {
            AppError::provider(
                "响应里没有文本",
                status,
                &ctx.provider.name,
                Some(&model.name),
                Some(shrink_json(&final_body)),
            )
        })?;
        let transcript = found
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty());
        let Some(transcript) = transcript else {
            return Err(AppError::provider(
                "响应里没有文本",
                status,
                &ctx.provider.name,
                Some(&model.name),
                Some(shrink_json(&final_body)),
            ));
        };
        return Ok(audio_response(
            ctx,
            model,
            started,
            status,
            raw_request,
            shrink_json(&final_body),
            transcript.to_string(),
            Vec::new(),
        ));
    }
    if kind == "binary" {
        let (status, bytes, raw_request) = call_bytes(
            ctx,
            model,
            &recipe.method,
            &recipe.path,
            &headers,
            &body,
            &logged_body,
        )
        .await?;
        let mime_type = media::describe_audio(&bytes).map_err(|message| {
            AppError::provider(
                message,
                status,
                &ctx.provider.name,
                Some(&model.name),
                Some(serde_json::json!({ "bytes": bytes.len() })),
            )
        })?;
        return Ok(audio_response(
            ctx,
            model,
            started,
            status,
            raw_request,
            serde_json::json!({ "bytes": bytes.len(), "mimeType": mime_type }),
            String::new(),
            vec![AudioBinary { bytes, mime_type }],
        ));
    }
    let (status, submitted, raw_request) = call(
        ctx,
        model,
        &recipe.method,
        &recipe.path,
        &headers,
        &body,
        &logged_body,
    )
    .await?;
    let final_body = if let Some(poll) = &recipe.poll {
        let task_id = task_id_of(&submitted, recipe.task_id_path.as_deref(), model, ctx)?;
        poll_until(ctx, model, &recipe, poll, &task_id, &tokens).await?
    } else {
        submitted
    };
    let found = at(&final_body, &recipe.result_path).ok_or_else(|| {
        AppError::provider(
            "响应里没有音频",
            status,
            &ctx.provider.name,
            Some(&model.name),
            Some(shrink_json(&final_body)),
        )
    })?;
    let bytes = if kind == "base64" {
        decode_media(found, "音频")
            .map_err(|err| err.context(&ctx.provider.name, Some(&model.name)))?
    } else {
        let url = found.as_str().ok_or_else(|| {
            AppError::provider(
                "音频地址不是字符串",
                status,
                &ctx.provider.name,
                Some(&model.name),
                Some(shrink_json(&final_body)),
            )
        })?;
        download(ctx, model, url, "音频").await?
    };
    let mime_type = media::describe_audio(&bytes)
        .map_err(|message| {
            AppError::provider(
                message,
                status,
                &ctx.provider.name,
                Some(&model.name),
                Some(shrink_json(&final_body)),
            )
        })?
        .to_string();
    Ok(audio_response(
        ctx,
        model,
        started,
        status,
        raw_request,
        shrink_json(&final_body),
        String::new(),
        vec![AudioBinary { bytes, mime_type }],
    ))
}

#[allow(clippy::too_many_arguments)]
fn audio_response(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    started: Instant,
    status: u16,
    raw_request: Value,
    raw_response: Value,
    transcript: String,
    binaries: Vec<AudioBinary>,
) -> CanonicalResponse {
    CanonicalResponse::Audio(AudioResult {
        audios: Vec::new(),
        latency_ms: elapsed_ms(started),
        provider_id: ctx.provider.id.clone(),
        provider_name: ctx.provider.name.clone(),
        model_definition_id: model.id.clone(),
        model_name: model.name.clone(),
        model_id: model.model_id.clone(),
        raw_request,
        raw_response,
        status,
        transcript,
        transcript_path: None,
        binaries,
    })
}

struct Tokens {
    model: String,
    prompt: String,
    negative: String,
    api_key: String,
    first_frame: Option<String>,
    last_frame: Option<String>,
    references: Vec<String>,
    reference_videos: Vec<String>,
    reference_audios: Vec<String>,
    parameters: Map<String, Value>,
    text: String,
    voice: String,
    transcript: String,
    reference_audio: Option<String>,
    source_audio: Option<String>,
}

async fn tokens(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    payload: &VideoPayload,
) -> Result<Tokens, AppError> {
    let frame = |path: &str| data_url(path);
    Ok(Tokens {
        model: model.model_id.clone(),
        prompt: payload.prompt.clone(),
        negative: payload.negative_prompt.clone().unwrap_or_default(),
        api_key: ctx
            .api_key
            .as_ref()
            .map(|secret| secret.expose().to_string())
            .unwrap_or_default(),
        first_frame: payload
            .first_frame
            .as_ref()
            .map(|item| frame(&item.path))
            .transpose()?,
        last_frame: payload
            .last_frame
            .as_ref()
            .map(|item| frame(&item.path))
            .transpose()?,
        references: payload
            .reference_images
            .iter()
            .map(|item| frame(&item.path))
            .collect::<Result<Vec<_>, _>>()?,
        reference_videos: payload
            .reference_videos
            .iter()
            .map(|item| media_data_url(&item.path, true))
            .collect::<Result<Vec<_>, _>>()?,
        reference_audios: payload
            .reference_audios
            .iter()
            .map(|item| media_data_url(&item.path, false))
            .collect::<Result<Vec<_>, _>>()?,
        parameters: payload.parameters.clone(),
        text: String::new(),
        voice: String::new(),
        transcript: String::new(),
        reference_audio: None,
        source_audio: None,
    })
}

fn audio_tokens(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    payload: &AudioPayload,
) -> Result<Tokens, AppError> {
    let reference_audio = match payload.reference_audio.as_ref() {
        Some(item) => Some(audio_data_url(&item.path, "参考音频")?),
        None => None,
    };
    let source_audio = match payload.source_audio.as_ref() {
        Some(item) => Some(audio_data_url(&item.path, "源音频")?),
        None => None,
    };
    Ok(Tokens {
        model: model.model_id.clone(),
        prompt: payload.text.clone(),
        negative: String::new(),
        api_key: ctx
            .api_key
            .as_ref()
            .map(|secret| secret.expose().to_string())
            .unwrap_or_default(),
        first_frame: None,
        last_frame: None,
        references: Vec::new(),
        reference_videos: Vec::new(),
        reference_audios: Vec::new(),
        parameters: payload.parameters.clone(),
        text: payload.text.clone(),
        voice: payload.voice.clone().unwrap_or_default(),
        transcript: payload.reference_transcript.clone().unwrap_or_default(),
        reference_audio,
        source_audio,
    })
}

fn audio_data_url(path: &str, label: &str) -> Result<String, AppError> {
    let bytes =
        http_util::read_file_bounded_sync(std::path::Path::new(path), media::MAX_AUDIO_BYTES)
            .map_err(|err| match err {
                http_util::BoundedFileReadError::TooLarge => AppError::validation("音频超过 40MB"),
                http_util::BoundedFileReadError::Io(err) => {
                    AppError::validation(format!("无法读取{label}: {err}"))
                }
            })?;
    let mime = media::describe_audio(&bytes).map_err(AppError::validation)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{mime};base64,{encoded}"))
}

fn media_data_url(path: &str, video: bool) -> Result<String, AppError> {
    let (limit, too_large, label) = if video {
        (
            media::MAX_VIDEO_PREVIEW_BYTES,
            "参考视频超过 32MB",
            "参考视频",
        )
    } else {
        (
            media::MAX_AUDIO_PREVIEW_BYTES,
            "参考音频超过 20MB",
            "参考音频",
        )
    };
    let bytes =
        http_util::read_file_bounded_sync(std::path::Path::new(path), limit).map_err(|err| {
            match err {
                http_util::BoundedFileReadError::TooLarge => AppError::validation(too_large),
                http_util::BoundedFileReadError::Io(err) => {
                    AppError::validation(format!("无法读取{label}: {err}"))
                }
            }
        })?;
    let mime = if video {
        media::sniff_video(&bytes).ok_or_else(|| AppError::validation("参考视频格式不受支持"))?
    } else {
        media::sniff_audio(&bytes).ok_or_else(|| AppError::validation("参考音频格式不受支持"))?
    };
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{mime};base64,{encoded}"))
}

fn data_url(path: &str) -> Result<String, AppError> {
    let (mime, bytes) = crate::adapters::data_url_parts(path)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{mime};base64,{encoded}"))
}

fn merge_extra(body: &mut Value, extra: Option<&Value>) -> Result<(), AppError> {
    let Some(extra) = extra.and_then(Value::as_object) else {
        return Ok(());
    };
    let Some(object) = body.as_object_mut() else {
        return Err(AppError::validation("Extra JSON 需要模板正文是对象"));
    };
    for (key, value) in extra {
        if object.contains_key(key) {
            return Err(AppError::validation("Extra JSON 不能覆盖模板字段"));
        }
        object.insert(key.clone(), value.clone());
    }
    Ok(())
}

fn live_headers(
    ctx: &AdapterContext,
    recipe: &HttpRecipe,
    tokens: &Tokens,
) -> BTreeMap<String, String> {
    let mut headers = ctx.provider.headers.clone();
    for header in &recipe.headers {
        let value = match substitute(&Value::String(header.value.clone()), tokens) {
            Value::String(text) => text,
            other => other.to_string(),
        };
        headers.insert(header.name.clone(), value);
    }
    headers
}

fn logged_headers(headers: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    redact_header_map(headers)
}

async fn call(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    method: &str,
    path: &str,
    headers: &BTreeMap<String, String>,
    body: &Value,
    logged_body: &Value,
) -> Result<(u16, Value, Value), AppError> {
    let url = join_url(&ctx.provider.base_url, path)?;
    let send_body = method == "POST";
    let mut builder = match method {
        "GET" => ctx.http.get(&url),
        _ => ctx.http.post(&url),
    };
    for (key, value) in headers {
        builder = builder.header(key, value);
    }
    if send_body {
        let payload = if body.is_null() {
            Value::Object(Map::new())
        } else {
            body.clone()
        };
        builder = builder.json(&payload);
    }
    let response = http_util::send(
        builder,
        Duration::from_millis(ctx.provider.timeout_ms),
        &ctx.provider.name,
        Some(&model.name),
    )
    .await?;
    let (status, parsed) =
        http_util::read_response(response, &ctx.provider.name, Some(&model.name)).await?;
    let raw = json_request(
        method,
        &url,
        headers,
        if send_body { logged_body } else { &Value::Null },
    );
    Ok((status, parsed, raw))
}

async fn call_bytes(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    method: &str,
    path: &str,
    headers: &BTreeMap<String, String>,
    body: &Value,
    logged_body: &Value,
) -> Result<(u16, Vec<u8>, Value), AppError> {
    let url = join_url(&ctx.provider.base_url, path)?;
    let send_body = method == "POST";
    let mut builder = match method {
        "GET" => ctx.http.get(&url),
        _ => ctx.http.post(&url),
    };
    for (key, value) in headers {
        builder = builder.header(key, value);
    }
    if send_body {
        let payload = if body.is_null() {
            Value::Object(Map::new())
        } else {
            body.clone()
        };
        builder = builder.json(&payload);
    }
    let response = http_util::send(
        builder,
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
    let bytes = tokio::select! {
        _ = super::exchange::wait_for_cancel(&ctx.cancelled) => {
            return Err(AppError::validation("任务已取消"));
        }
        bytes = http_util::read_body_with_limit(
            response,
            media::MAX_VIDEO_BYTES,
            &ctx.provider.name,
            Some(&model.name),
        ) => bytes?,
    };
    let raw = json_request(
        method,
        &url,
        headers,
        if send_body { logged_body } else { &Value::Null },
    );
    Ok((status, bytes, raw))
}

fn logged_tokens(tokens: &Tokens) -> Tokens {
    Tokens {
        api_key: "••••".to_string(),
        model: tokens.model.clone(),
        prompt: tokens.prompt.clone(),
        negative: tokens.negative.clone(),
        first_frame: tokens.first_frame.clone(),
        last_frame: tokens.last_frame.clone(),
        references: tokens.references.clone(),
        reference_videos: tokens.reference_videos.clone(),
        reference_audios: tokens.reference_audios.clone(),
        parameters: tokens.parameters.clone(),
        text: tokens.text.clone(),
        voice: tokens.voice.clone(),
        transcript: tokens.transcript.clone(),
        reference_audio: tokens.reference_audio.clone(),
        source_audio: tokens.source_audio.clone(),
    }
}

fn json_request(
    method: &str,
    url: &str,
    headers: &BTreeMap<String, String>,
    body: &Value,
) -> Value {
    shrink_json(&serde_json::json!({
        "method": method,
        "url": redact_url(url),
        "headers": logged_headers(headers),
        "body": body,
    }))
}

fn task_id_of(
    body: &Value,
    path: Option<&str>,
    model: &ModelDefinition,
    ctx: &AdapterContext,
) -> Result<String, AppError> {
    let path = path.unwrap_or("$");
    let value = at(body, path).ok_or_else(|| {
        AppError::provider(
            "响应里没有任务编号",
            200,
            &ctx.provider.name,
            Some(&model.name),
            Some(shrink_json(body)),
        )
    })?;
    let task_id = match value {
        Value::String(text) => text.trim().to_string(),
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    };
    if task_id.is_empty() || task_id.contains(['/', '\\', '?', ' ', '#']) || task_id.contains("..")
    {
        return Err(AppError::provider(
            "任务编号无效",
            200,
            &ctx.provider.name,
            Some(&model.name),
            Some(shrink_json(body)),
        ));
    }
    Ok(task_id)
}

async fn poll_until(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    recipe: &HttpRecipe,
    poll: &crate::domain::HttpPoll,
    task_id: &str,
    tokens: &Tokens,
) -> Result<Value, AppError> {
    let budget = ctx
        .provider
        .timeout_ms
        .saturating_mul(40)
        .clamp(120_000, 600_000);
    let interval = poll.interval_ms.unwrap_or(1_000).clamp(500, 10_000);
    let started = Instant::now();
    let path = poll.path.replace("{{taskId}}", task_id);
    let headers = live_headers(ctx, recipe, tokens);
    let template = poll.body.clone().unwrap_or(Value::Null);
    let body = substitute(&template, tokens);
    let logged_body = substitute(&template, &logged_tokens(tokens));
    loop {
        if ctx.cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(AppError::validation("任务已取消"));
        }
        if started.elapsed() >= Duration::from_millis(budget) {
            return Err(AppError::timeout(&ctx.provider.name, Some(&model.name)));
        }
        let (_status, parsed, _) = call(
            ctx,
            model,
            &poll.method,
            &path,
            &headers,
            &body,
            &logged_body,
        )
        .await?;
        let state = at(&parsed, &poll.status_path)
            .map(status_text)
            .unwrap_or_default();
        if state == poll.success {
            return Ok(parsed);
        }
        if state == poll.failure {
            let message = parsed
                .get("error")
                .and_then(Value::as_str)
                .or_else(|| parsed.pointer("/error/message").and_then(Value::as_str))
                .unwrap_or("视频任务失败")
                .to_string();
            return Err(AppError::provider(
                message,
                200,
                &ctx.provider.name,
                Some(&model.name),
                Some(shrink_json(&parsed)),
            ));
        }
        tokio::select! {
            _ = super::exchange::wait_for_cancel(&ctx.cancelled) => {
                return Err(AppError::validation("任务已取消"));
            }
            _ = tokio::time::sleep(Duration::from_millis(interval)) => {}
        }
    }
}

fn status_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.trim().to_string(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        _ => String::new(),
    }
}

fn decode_media(value: &Value, label: &str) -> Result<Vec<u8>, AppError> {
    let text = value
        .as_str()
        .ok_or_else(|| AppError::provider(format!("{label}数据不是字符串"), 200, "", None, None))?;
    let payload = text
        .split_once(',')
        .filter(|(head, _)| head.contains("base64"))
        .map(|(_, data)| data)
        .unwrap_or(text);
    base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .map_err(|_| AppError::new(crate::error::PROVIDER, format!("响应里的{label}无法解码")))
}

async fn download(
    ctx: &AdapterContext,
    model: &ModelDefinition,
    url: &str,
    label: &str,
) -> Result<Vec<u8>, AppError> {
    let mut builder = ctx.http.get(url);
    if super::exchange::same_origin(&ctx.provider.base_url, url) {
        for (key, value) in &ctx.provider.headers {
            builder = builder.header(key, value);
        }
        if let Some(secret) = &ctx.api_key {
            builder = builder.header("Authorization", format!("Bearer {}", secret.expose()));
        }
    }
    let response = http_util::send(
        builder,
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
            media::MAX_AUDIO_BYTES,
            &ctx.provider.name,
            Some(&model.name),
        ) => bytes.map_err(|err| {
            if err.code == crate::error::PROVIDER {
                AppError::provider(
                    format!("无法下载{label}: {}", err.message),
                    200,
                    &ctx.provider.name,
                    Some(&model.name),
                    None,
                )
            } else {
                err
            }
        }),
    }
}

fn substitute(value: &Value, tokens: &Tokens) -> Value {
    match value {
        Value::String(text) => replace_string(text, tokens),
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| substitute(item, tokens)).collect())
        }
        Value::Object(map) => {
            let mut next = Map::new();
            for (key, item) in map {
                next.insert(key.clone(), substitute(item, tokens));
            }
            Value::Object(next)
        }
        other => other.clone(),
    }
}

fn replace_string(text: &str, tokens: &Tokens) -> Value {
    if let Some(key) = whole_placeholder(text) {
        return token_value(key, tokens);
    }
    let mut output = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let Some(end) = rest[start + 2..].find("}}") else {
            output.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let key = &rest[start + 2..start + 2 + end];
        output.push_str(&token_text(key, tokens));
        rest = &rest[start + 2 + end + 2..];
    }
    output.push_str(rest);
    Value::String(output)
}

fn whole_placeholder(text: &str) -> Option<&str> {
    let inner = text.trim().strip_prefix("{{")?.strip_suffix("}}")?;
    if inner.contains('{') || inner.contains('}') {
        None
    } else {
        Some(inner.trim())
    }
}

fn token_value(key: &str, tokens: &Tokens) -> Value {
    match key {
        "model" => Value::String(tokens.model.clone()),
        "prompt" => Value::String(tokens.prompt.clone()),
        "text" => {
            if tokens.text.is_empty() {
                Value::Null
            } else {
                Value::String(tokens.text.clone())
            }
        }
        "voice" => {
            if !tokens.voice.is_empty() {
                Value::String(tokens.voice.clone())
            } else {
                tokens
                    .parameters
                    .get("voice")
                    .cloned()
                    .unwrap_or(Value::Null)
            }
        }
        "referenceTranscript" => {
            if tokens.transcript.is_empty() {
                Value::Null
            } else {
                Value::String(tokens.transcript.clone())
            }
        }
        "sourceAudio" => tokens
            .source_audio
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
        "referenceAudio" => tokens
            .reference_audio
            .clone()
            .or_else(|| tokens.reference_audios.first().cloned())
            .map(Value::String)
            .unwrap_or(Value::Null),
        "referenceVideos" => Value::Array(
            tokens
                .reference_videos
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        ),
        "referenceAudios" => Value::Array(
            tokens
                .reference_audios
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        ),
        "negativePrompt" => {
            if tokens.negative.is_empty() {
                Value::Null
            } else {
                Value::String(tokens.negative.clone())
            }
        }
        "apiKey" => Value::String(tokens.api_key.clone()),
        "firstFrame" => tokens
            .first_frame
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
        "lastFrame" => tokens
            .last_frame
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
        "referenceImages" => Value::Array(
            tokens
                .references
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        ),
        _ => {
            if let Some(index) = indexed(key, "referenceImages") {
                return tokens
                    .references
                    .get(index)
                    .cloned()
                    .map(Value::String)
                    .unwrap_or(Value::Null);
            }
            if let Some(index) = indexed(key, "referenceVideos") {
                return tokens
                    .reference_videos
                    .get(index)
                    .cloned()
                    .map(Value::String)
                    .unwrap_or(Value::Null);
            }
            if let Some(index) = indexed(key, "referenceAudios") {
                return tokens
                    .reference_audios
                    .get(index)
                    .cloned()
                    .map(Value::String)
                    .unwrap_or(Value::Null);
            }
            tokens.parameters.get(key).cloned().unwrap_or(Value::Null)
        }
    }
}

fn token_text(key: &str, tokens: &Tokens) -> String {
    match token_value(key, tokens) {
        Value::Null => String::new(),
        Value::String(text) => text,
        other => other.to_string(),
    }
}

fn indexed(key: &str, prefix: &str) -> Option<usize> {
    let rest = key.strip_prefix(prefix)?.strip_prefix('[')?;
    let number = rest.strip_suffix(']')?;
    number.parse().ok()
}

fn at<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    let mut rest = path.strip_prefix('$')?;
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
            return None;
        }
    }
    Some(current)
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn substitutes_typed_parameters_and_leaves_missing_frames_null() {
        let tokens = Tokens {
            model: "video-1".to_string(),
            prompt: "a fox".to_string(),
            negative: String::new(),
            api_key: "secret".to_string(),
            first_frame: None,
            last_frame: None,
            references: Vec::new(),
            reference_videos: Vec::new(),
            reference_audios: Vec::new(),
            parameters: Map::from_iter([("seed".to_string(), json!(7))]),
            text: String::new(),
            voice: String::new(),
            transcript: String::new(),
            reference_audio: None,
            source_audio: None,
        };
        let body = substitute(
            &json!({
                "model": "{{model}}",
                "prompt": "see {{prompt}}",
                "seed": "{{seed}}",
                "first": "{{firstFrame}}"
            }),
            &tokens,
        );
        assert_eq!(body["model"], json!("video-1"));
        assert_eq!(body["prompt"], json!("see a fox"));
        assert_eq!(body["seed"], json!(7));
        assert!(body["first"].is_null());
    }
}
