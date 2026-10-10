use std::path::Path;

use base64::Engine;
use reqwest::Client;
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::adapters::adapter_for;
use crate::canonical::{
    AdapterContext, AudioPayload, AudioResult, AudioTask, CanonicalRequest, CanonicalResponse,
};
use crate::credentials::SecretStore;
use crate::db;
use crate::domain::{
    capability_enabled, GenerateAudioInput, MediaRef, Modality, ParameterType, ProviderRecord,
    ProviderType,
};
use crate::error::AppError;
use crate::media;
use crate::schema::{is_empty_value, normalize_parameters, reject_exclusive};
use crate::services::history::{self, NewAudioTask, StoredOutput};
use crate::services::providers::load_secret;

struct Runtime<'a> {
    pool: &'a SqlitePool,
    http: &'a Client,
    secrets: &'a dyn SecretStore,
    data_dir: &'a Path,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioFile {
    pub mime_type: String,
    pub base64: String,
}

#[allow(dead_code)]
pub async fn execute(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    data_dir: &Path,
    input: GenerateAudioInput,
) -> Result<AudioResult, AppError> {
    execute_with_cancel(pool, http, secrets, data_dir, input, None, None).await
}

pub async fn execute_with_cancel(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    data_dir: &Path,
    input: GenerateAudioInput,
    task_id: Option<String>,
    cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Result<AudioResult, AppError> {
    let model = db::get_model(pool, &input.model_definition_id).await?;
    let provider = db::get_provider(pool, &model.provider_id).await?;
    let runtime = Runtime {
        pool,
        http,
        secrets,
        data_dir,
    };
    execute_with(&runtime, &provider, &model, input, task_id, cancel_flag)
        .await
        .map_err(|err| err.annotate(&provider.name, &model.name))
}

pub async fn read_file(path: &str) -> Result<AudioFile, AppError> {
    let file = Path::new(path);
    if !file.is_file() {
        return Err(AppError::validation("找不到音频"));
    }
    let bytes = crate::http_util::read_file_bounded(file, media::MAX_AUDIO_PREVIEW_BYTES)
        .await
        .map_err(|err| match err {
            crate::http_util::BoundedFileReadError::TooLarge => {
                AppError::validation("音频超过 20MB，无法在页面内预览")
            }
            crate::http_util::BoundedFileReadError::Io(err) => {
                AppError::validation(format!("无法读取音频: {err}"))
            }
        })?;
    let mime =
        media::sniff_audio(&bytes).ok_or_else(|| AppError::validation("不是受支持的音频"))?;
    Ok(AudioFile {
        mime_type: mime.to_string(),
        base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

async fn execute_with(
    runtime: &Runtime<'_>,
    provider: &ProviderRecord,
    model: &crate::domain::ModelDefinition,
    input: GenerateAudioInput,
    task_id: Option<String>,
    cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Result<AudioResult, AppError> {
    if !model.enabled {
        return Err(AppError::validation("模型已停用"));
    }
    if !provider.enabled {
        return Err(AppError::validation("Provider 已停用"));
    }
    if model.modality != Modality::Audio {
        return Err(AppError::unsupported("当前入口只执行音频生成"));
    }
    let task = parse_audio_task(input.task.as_deref())?;
    let text = input.text.trim().to_string();
    if text.chars().count() > 20_000 {
        return Err(AppError::validation("文本过长"));
    }
    if matches!(task, AudioTask::Tts | AudioTask::Clone) && text.is_empty() {
        return Err(AppError::validation("请填写要朗读的文本"));
    }
    let transcript = input
        .reference_transcript
        .as_deref()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned);
    if transcript
        .as_ref()
        .is_some_and(|item| item.chars().count() > 20_000)
    {
        return Err(AppError::validation("参考文本过长"));
    }
    if let Some(extra) = &input.extra {
        if !extra.is_object() {
            return Err(AppError::validation("Extra JSON 必须是对象"));
        }
        let size = serde_json::to_string(extra)
            .map(|item| item.len())
            .unwrap_or(0);
        if size > 100_000 {
            return Err(AppError::validation("Extra JSON 过大"));
        }
    }
    let explicit = input
        .voice
        .as_deref()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned);
    if explicit
        .as_ref()
        .is_some_and(|item| item.chars().count() > 80)
    {
        return Err(AppError::validation("声音名称过长"));
    }
    let mut supplied = input.parameters.clone();
    if explicit.is_some() {
        supplied.remove("voice");
    }
    let mut parameters = normalize_parameters(&model.parameters, &supplied, &model.constraints)?;
    if let Some(name) = &explicit {
        if voice_parameter_accepts(model, name) {
            parameters.insert("voice".to_string(), Value::String(name.clone()));
        } else {
            parameters.remove("voice");
        }
    }
    let voice = explicit.or_else(|| {
        parameters
            .get("voice")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
    });
    for definition in &model.parameters {
        if definition.kind.is_media()
            && parameters
                .get(&definition.key)
                .is_some_and(|value| !is_empty_value(Some(value)))
        {
            return Err(AppError::unsupported(format!(
                "参数「{}」是媒体输入。音频生成请使用参考音频",
                definition.label
            )));
        }
    }
    let source = input
        .source_audio
        .clone()
        .filter(|item| !item.path.trim().is_empty());
    let reference = input
        .reference_audio
        .clone()
        .filter(|item| !item.path.trim().is_empty());
    match task {
        AudioTask::Tts => {
            if !capability_enabled(&model.capabilities, "textToSpeech") {
                return Err(AppError::unsupported("当前模型不能文本转语音"));
            }
            if (reference.is_some() || transcript.is_some())
                && !capability_enabled(&model.capabilities, "voiceClone")
            {
                return Err(AppError::unsupported("当前模型不接受参考音频"));
            }
        }
        AudioTask::Stt => {
            if source.is_none() {
                return Err(AppError::validation("请选择源音频"));
            }
            if !capability_enabled(&model.capabilities, "speechToText") {
                return Err(AppError::unsupported("当前模型不能语音转文本"));
            }
            if reference.is_some() || transcript.is_some() {
                return Err(AppError::validation("这个任务不使用参考音频"));
            }
        }
        AudioTask::Sts => {
            if source.is_none() {
                return Err(AppError::validation("请选择源音频"));
            }
            if !capability_enabled(&model.capabilities, "speechToSpeech") {
                return Err(AppError::unsupported("当前模型不能语音转语音"));
            }
            if reference.is_some() || transcript.is_some() {
                return Err(AppError::validation("这个任务不使用参考音频"));
            }
        }
        AudioTask::Clone => {
            if reference.is_none() {
                return Err(AppError::validation("请选择参考音频"));
            }
            if !capability_enabled(&model.capabilities, "voiceClone") {
                return Err(AppError::unsupported("当前模型不能声音克隆"));
            }
        }
    }
    let mut present = Vec::new();
    if reference.is_some() {
        present.push("referenceAudio");
    }
    for key in parameters.keys() {
        present.push(key.as_str());
    }
    reject_exclusive(&model.constraints, &present)?;
    let allowed = match task {
        AudioTask::Tts | AudioTask::Stt => matches!(
            provider.provider_type,
            ProviderType::Openai
                | ProviderType::OpenaiCompatible
                | ProviderType::GenericHttp
                | ProviderType::Replicate
                | ProviderType::Fal
        ),
        AudioTask::Sts | AudioTask::Clone => matches!(
            provider.provider_type,
            ProviderType::GenericHttp | ProviderType::Replicate | ProviderType::Fal
        ),
    };
    if !allowed {
        let message = match task {
            AudioTask::Tts | AudioTask::Stt => {
                "这个音频任务目前由 OpenAI、OpenAI Compatible、Generic HTTP、Replicate 与 fal.ai 执行"
            }
            AudioTask::Sts | AudioTask::Clone => {
                "这个音频任务目前由 Generic HTTP、Replicate 与 fal.ai 执行"
            }
        };
        return Err(AppError::unsupported(message));
    }
    let api_key = load_secret(runtime.secrets, provider.credential_id.as_deref())?;
    let openai_family = matches!(
        provider.provider_type,
        ProviderType::Openai | ProviderType::OpenaiCompatible
    );
    if openai_family && (reference.is_some() || transcript.is_some()) {
        return Err(AppError::unsupported(
            "这个语音接口不接受参考音频，请改用 Generic HTTP",
        ));
    }
    if provider.provider_type.requires_api_key() && api_key.is_none() {
        return Err(AppError::auth(
            "缺少 API Key",
            &provider.name,
            Some(&model.name),
        ));
    }
    if provider.provider_type == ProviderType::GenericHttp {
        prepare_http(model, &api_key, input.extra.as_ref(), provider, &model.name)?;
    }
    if let Some(extra) = input.extra.as_ref().and_then(Value::as_object) {
        if extra.contains_key("model")
            || extra.contains_key("input")
            || (voice.is_some() && extra.contains_key("voice"))
            || extra.keys().any(|key| parameters.contains_key(key))
        {
            return Err(AppError::validation("Extra JSON 不能覆盖模板字段"));
        }
    }
    if let Some(audio) = &source {
        let bytes = crate::http_util::read_file_bounded(
            Path::new(audio.path.trim()),
            media::MAX_AUDIO_BYTES,
        )
        .await
        .map_err(|err| match err {
            crate::http_util::BoundedFileReadError::TooLarge => {
                AppError::validation("音频超过 40MB")
            }
            crate::http_util::BoundedFileReadError::Io(err) => {
                AppError::validation(format!("无法读取源音频: {err}"))
            }
        })?;
        media::describe_audio(&bytes).map_err(AppError::validation)?;
    }
    if let Some(audio) = &reference {
        let bytes = crate::http_util::read_file_bounded(
            Path::new(audio.path.trim()),
            media::MAX_AUDIO_BYTES,
        )
        .await
        .map_err(|err| match err {
            crate::http_util::BoundedFileReadError::TooLarge => {
                AppError::validation("音频超过 40MB")
            }
            crate::http_util::BoundedFileReadError::Io(err) => {
                AppError::validation(format!("无法读取参考音频: {err}"))
            }
        })?;
        media::describe_audio(&bytes).map_err(AppError::validation)?;
    }
    let task_id = task_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let (chat_path, models_path) = match &model.transport {
        Some(transport) => (transport.chat_path.clone(), transport.models_path.clone()),
        None => (None, None),
    };
    let mut ctx = AdapterContext::new(
        runtime.http.clone(),
        provider.clone(),
        Some(model.clone()),
        api_key,
        chat_path,
        models_path,
    );
    if let Some(flag) = cancel_flag {
        ctx = ctx.with_cancelled(flag);
    }
    let mut sent = parameters.clone();
    sent.remove("voice");
    let draft = Draft {
        text: text.clone(),
        task: audio_task_name(task).to_string(),
        voice: voice.clone(),
        source_audio: source.clone(),
        reference_audio: reference.clone(),
        reference_transcript: transcript.clone(),
        extra: input.extra.clone().filter(|value| !value.is_null()),
        parameters: parameters.clone(),
    };
    let request = CanonicalRequest::Audio(AudioPayload {
        task,
        text,
        voice,
        source_audio: source,
        reference_audio: reference,
        reference_transcript: transcript,
        parameters: sent,
        extra: draft.extra.clone(),
    });

    history::record_audio_task(
        runtime.pool,
        history::NewAudioTask {
            id: task_id.clone(),
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            model_id: model.id.clone(),
            model_name: model.name.clone(),
            model_api_id: model.model_id.clone(),
            text: draft.text.clone(),
            voice: draft.voice.clone(),
            reference_audio: draft.reference_audio.clone(),
            reference_transcript: draft.reference_transcript.clone(),
            source_audio: draft.source_audio.clone(),
            audio_task: draft.task.clone(),
            transcript_path: None,
            extra: draft.extra.clone(),
            parameters: draft.parameters.clone(),
            status: "running".to_string(),
            outputs: Vec::new(),
            raw_request: None,
            raw_response: None,
            error: None,
        },
    )
    .await?;

    let outcome = adapter_for(provider.provider_type)
        .execute(&ctx, request)
        .await;
    let outcome = match outcome {
        Ok(CanonicalResponse::Audio(mut result)) => {
            match persist(runtime.data_dir, &task_id, &mut result).await {
                Ok(()) => Ok(CanonicalResponse::Audio(result)),
                Err(mut err) => {
                    err.raw = Some(Box::new(result.raw_response));
                    Err(err)
                }
            }
        }
        Ok(CanonicalResponse::Text(_))
        | Ok(CanonicalResponse::Image(_))
        | Ok(CanonicalResponse::Video(_)) => Err(AppError::unsupported("音频入口收到了其他结果")),
        Err(err) => Err(err),
    };
    let remember_res = remember(runtime, provider, model, &task_id, draft, &outcome).await;
    let result = match outcome? {
        CanonicalResponse::Audio(result) => result,
        CanonicalResponse::Text(_) | CanonicalResponse::Image(_) | CanonicalResponse::Video(_) => {
            return Err(AppError::unsupported("音频入口收到了其他结果"));
        }
    };
    remember_res?;
    Ok(result)
}

fn voice_parameter_accepts(model: &crate::domain::ModelDefinition, name: &str) -> bool {
    let Some(definition) = model.parameters.iter().find(|item| item.key == "voice") else {
        return false;
    };
    if definition.kind != ParameterType::Select {
        return true;
    }
    definition
        .options
        .iter()
        .flatten()
        .any(|option| option.value.as_str() == Some(name))
}

fn prepare_http(
    model: &crate::domain::ModelDefinition,
    api_key: &Option<crate::credentials::SecretString>,
    extra: Option<&Value>,
    provider: &ProviderRecord,
    model_name: &str,
) -> Result<(), AppError> {
    let recipe = model
        .transport
        .as_ref()
        .and_then(|item| item.http.as_ref())
        .ok_or_else(|| AppError::validation("模型没有 HTTP 请求模板"))?;
    let text = serde_json::to_string(recipe).unwrap_or_default();
    if text.contains("{{apiKey}}") && api_key.is_none() {
        return Err(AppError::auth(
            "缺少 API Key",
            &provider.name,
            Some(model_name),
        ));
    }
    if let (Some(extra), Some(body)) = (
        extra.and_then(Value::as_object),
        recipe.body.as_ref().and_then(Value::as_object),
    ) {
        if extra.keys().any(|key| body.contains_key(key)) {
            return Err(AppError::validation("Extra JSON 不能覆盖模板字段"));
        }
    } else if extra.is_some() && recipe.body.is_none() {
        return Err(AppError::validation("Extra JSON 需要模板正文是对象"));
    }
    Ok(())
}

async fn persist(data_dir: &Path, task_id: &str, result: &mut AudioResult) -> Result<(), AppError> {
    crate::storage_util::validate_task_id(task_id)?;
    let dir = data_dir.join("audio");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|err| AppError::storage(format!("无法创建数据目录: {err}")))?;
    let mut saved: Vec<crate::canonical::GeneratedAudio> = Vec::new();
    let total_binaries = result.binaries.len();
    for (index, binary) in result.binaries.iter().enumerate() {
        let path = dir.join(format!(
            "{task_id}-{index}.{}",
            media::extension_for(&binary.mime_type)
        ));
        crate::storage_util::ensure_within_dir(data_dir, &path)?;
        if binary.bytes.is_empty() {
            for s in &saved {
                let _ = tokio::fs::remove_file(&s.path).await;
            }
            return Err(AppError::validation("生成的音频内容为空 (0 字节)"));
        }
        if media::sniff_audio(&binary.bytes).is_none() {
            for s in &saved {
                let _ = tokio::fs::remove_file(&s.path).await;
            }
            return Err(AppError::validation("生成的内容不是合法的音频格式"));
        }
        if let Err(err) = crate::storage_util::atomic_write_async(&path, &binary.bytes).await {
            for s in &saved {
                let _ = tokio::fs::remove_file(&s.path).await;
            }
            return Err(err);
        }
        saved.push(crate::canonical::GeneratedAudio {
            path: path.to_string_lossy().to_string(),
            mime_type: binary.mime_type.clone(),
        });
    }
    result.binaries.clear();
    if saved.len() < total_binaries {
        for s in &saved {
            let _ = tokio::fs::remove_file(&s.path).await;
        }
        return Err(AppError::storage("音频文件未能完整保存"));
    }
    if !result.transcript.trim().is_empty() {
        let path = dir.join(format!("{task_id}.txt"));
        crate::storage_util::ensure_within_dir(data_dir, &path)?;
        crate::storage_util::atomic_write_async(&path, result.transcript.as_bytes())
            .await
            .map_err(|err| AppError::storage(format!("无法保存文本: {err}")))?;
        result.transcript_path = Some(path.to_string_lossy().to_string());

        if let Some(srt) = crate::adapters::generate_srt(&result.raw_response) {
            let srt_path = dir.join(format!("{task_id}.srt"));
            crate::storage_util::ensure_within_dir(data_dir, &srt_path)?;
            let _ = crate::storage_util::atomic_write_async(&srt_path, srt.as_bytes()).await;
        }
        if let Some(vtt) = crate::adapters::generate_vtt(&result.raw_response) {
            let vtt_path = dir.join(format!("{task_id}.vtt"));
            crate::storage_util::ensure_within_dir(data_dir, &vtt_path)?;
            let _ = crate::storage_util::atomic_write_async(&vtt_path, vtt.as_bytes()).await;
        }
    }
    if saved.is_empty() && result.transcript_path.is_none() {
        return Err(AppError::storage("无法保存音频"));
    }
    result.audios = saved;
    Ok(())
}

fn parse_audio_task(value: Option<&str>) -> Result<AudioTask, AppError> {
    match value.unwrap_or("tts").trim().to_lowercase().as_str() {
        "" | "tts" | "text_to_speech" | "text-to-speech" => Ok(AudioTask::Tts),
        "stt" | "speech_to_text" | "speech-to-text" => Ok(AudioTask::Stt),
        "sts" | "speech_to_speech" | "speech-to-speech" => Ok(AudioTask::Sts),
        "clone" | "voice_clone" | "voice-clone" => Ok(AudioTask::Clone),
        _ => Err(AppError::validation("音频任务无效")),
    }
}

fn audio_task_name(task: AudioTask) -> &'static str {
    match task {
        AudioTask::Tts => "tts",
        AudioTask::Stt => "stt",
        AudioTask::Sts => "sts",
        AudioTask::Clone => "clone",
    }
}

struct Draft {
    text: String,
    task: String,
    voice: Option<String>,
    source_audio: Option<MediaRef>,
    reference_audio: Option<MediaRef>,
    reference_transcript: Option<String>,
    extra: Option<Value>,
    parameters: Map<String, Value>,
}

async fn remember(
    runtime: &Runtime<'_>,
    provider: &ProviderRecord,
    model: &crate::domain::ModelDefinition,
    task_id: &str,
    draft: Draft,
    outcome: &Result<CanonicalResponse, AppError>,
) -> Result<(), AppError> {
    let (status, outputs, raw_request, raw_response, error, transcript_path) = match outcome {
        Ok(CanonicalResponse::Audio(result)) => (
            "completed",
            result
                .audios
                .iter()
                .map(|audio| StoredOutput {
                    path: audio.path.clone(),
                    mime_type: audio.mime_type.clone(),
                    width: None,
                    height: None,
                })
                .collect(),
            Some(result.raw_request.clone()),
            Some(result.raw_response.clone()),
            None,
            result.transcript_path.clone(),
        ),
        Ok(_) => (
            "failed",
            Vec::new(),
            None,
            None,
            Some("音频入口收到了其他结果".to_string()),
            None,
        ),
        Err(err) => {
            let status = if err.message.contains("取消") || err.message.contains("停止") {
                "cancelled"
            } else {
                "failed"
            };
            (
                status,
                Vec::new(),
                None,
                err.raw.as_deref().cloned(),
                Some(err.message.clone()),
                None,
            )
        }
    };
    let record_result = history::record_audio_task(
        runtime.pool,
        NewAudioTask {
            id: task_id.to_string(),
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            model_id: model.id.clone(),
            model_name: model.name.clone(),
            model_api_id: model.model_id.clone(),
            text: draft.text,
            voice: draft.voice,
            reference_audio: draft.reference_audio,
            reference_transcript: draft.reference_transcript,
            source_audio: draft.source_audio,
            audio_task: draft.task,
            transcript_path,
            extra: draft.extra,
            parameters: draft.parameters,
            status: status.to_string(),
            outputs,
            raw_request,
            raw_response,
            error,
        },
    )
    .await
    .map(|_| ());
    if record_result.is_err() {
        let paths: Vec<String> = match outcome {
            Ok(CanonicalResponse::Audio(result)) => result
                .audios
                .iter()
                .map(|audio| audio.path.clone())
                .chain(result.transcript_path.iter().cloned())
                .collect(),
            _ => Vec::new(),
        };
        if let Err(err) = history::remove_unreferenced_files(runtime.pool, &paths).await {
            crate::logging::global_logger().warn(
                "audio",
                None,
                Some(task_id),
                Some(&provider.id),
                Some(&model.id),
                "orphan_output_cleanup_failed",
                &err.message,
            );
        }
    }
    record_result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn persist_rejects_empty_binary() {
        let temp_dir =
            std::env::temp_dir().join(format!("ai-wb-test-aud-{}", uuid::Uuid::new_v4()));
        let task_id = uuid::Uuid::new_v4().to_string();
        let mut result = AudioResult {
            audios: vec![],
            transcript: "".to_string(),
            transcript_path: None,
            latency_ms: 0,
            provider_id: "p".into(),
            provider_name: "p".into(),
            model_definition_id: "m".into(),
            model_name: "m".into(),
            model_id: "m".into(),
            raw_request: serde_json::json!({}),
            raw_response: serde_json::json!({}),
            status: 200,
            binaries: vec![crate::canonical::AudioBinary {
                bytes: vec![],
                mime_type: "audio/mp3".to_string(),
            }],
        };
        let err = persist(&temp_dir, &task_id, &mut result).await.unwrap_err();
        assert!(err.message.contains("为空 (0 字节)"));
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }

    #[tokio::test]
    async fn persist_rejects_invalid_media() {
        let temp_dir =
            std::env::temp_dir().join(format!("ai-wb-test-aud-bad-{}", uuid::Uuid::new_v4()));
        let task_id = uuid::Uuid::new_v4().to_string();
        let mut result = AudioResult {
            audios: vec![],
            transcript: "".to_string(),
            transcript_path: None,
            latency_ms: 0,
            provider_id: "p".into(),
            provider_name: "p".into(),
            model_definition_id: "m".into(),
            model_name: "m".into(),
            model_id: "m".into(),
            raw_request: serde_json::json!({}),
            raw_response: serde_json::json!({}),
            status: 200,
            binaries: vec![crate::canonical::AudioBinary {
                bytes: vec![9, 8, 7, 6],
                mime_type: "audio/mp3".to_string(),
            }],
        };
        let err = persist(&temp_dir, &task_id, &mut result).await.unwrap_err();
        assert!(err.message.contains("不是合法的音频格式"));
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
