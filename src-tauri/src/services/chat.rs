use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use reqwest::Client;
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::adapters::adapter_for;
use crate::canonical::{
    AdapterContext, CanonicalRequest, CanonicalResponse, LiveDelta, TextPayload, TextResult,
    TextTurn, ToolLog, Usage,
};
use crate::credentials::SecretStore;
use crate::db::{self, ConversationRow, MessageRow};
use crate::domain::{
    capability_enabled, now_iso, ChatSendInput, ConversationUpdate, MediaRef, Modality,
    ProviderRecord, ProviderType,
};
use crate::error::{AppError, NOT_FOUND};
use crate::schema::{is_empty_value, normalize_parameters};
use crate::services::agent::{self, AgentEvent, AgentTurn};
use crate::services::context::{
    auto_title, compose_system, parse_strategy, plan_context, transcript, ContextItem, ContextPlan,
    ContextStrategy,
};
use crate::services::history::{self, NewTextTask};
use crate::services::providers::load_secret;

const SUMMARY_PROMPT: &str =
    "把下面的对话压缩成可用于继续对话的摘要。保留目标、约束、已确定事实和未完成事项。使用原语言。不要加标题.";

type Cancels = Mutex<HashMap<String, Arc<AtomicBool>>>;
type NoticeFn = dyn Fn(ChatNotice) -> bool + Send + Sync;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub model_definition_id: String,
    pub system_prompt: String,
    pub context_strategy: String,
    pub context_limit: u32,
    pub summary: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub text: String,
    pub images: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reasoning: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub stopped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_logs: Vec<ToolLog>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub max_steps_hit: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_pinned: bool,
    pub token_count: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDetail {
    pub conversation: Conversation,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ChatNotice {
    Started {
        conversation: Conversation,
        messages: Vec<ChatMessage>,
    },
    Delta {
        text: String,
    },
    Reasoning {
        text: String,
    },
    Tool {
        log: ToolLog,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTurnResult {
    pub conversation: Conversation,
    pub assistant: ChatMessage,
    pub context_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredMessage {
    #[serde(default)]
    text: String,
    #[serde(default)]
    images: Vec<String>,
    #[serde(default)]
    reasoning: String,
    #[serde(default)]
    usage: Option<Usage>,
    #[serde(default)]
    stopped: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    tool_logs: Vec<ToolLog>,
    #[serde(default)]
    max_steps_hit: bool,
}

struct CancelGuard<'a> {
    cancels: &'a Cancels,
    id: String,
    flag: Arc<AtomicBool>,
}

impl Drop for CancelGuard<'_> {
    fn drop(&mut self) {
        let mut map = self.cancels.lock().unwrap_or_else(|err| err.into_inner());
        if map
            .get(&self.id)
            .is_some_and(|current| Arc::ptr_eq(current, &self.flag))
        {
            map.remove(&self.id);
        }
    }
}

pub fn cancel(cancels: &Cancels, conversation_id: &str) {
    let map = cancels.lock().unwrap_or_else(|err| err.into_inner());
    if let Some(flag) = map.get(conversation_id) {
        flag.store(true, Ordering::Relaxed);
    }
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<Conversation>, AppError> {
    let rows = db::list_conversations(pool).await?;
    Ok(rows.iter().map(to_conversation).collect())
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<ConversationDetail, AppError> {
    let conversation = db::get_conversation(pool, id).await?;
    detail(pool, conversation).await
}

pub async fn update(
    pool: &SqlitePool,
    id: &str,
    input: ConversationUpdate,
) -> Result<Conversation, AppError> {
    let mut row = db::get_conversation(pool, id).await?;
    if let Some(title) = input.title {
        row.title = clean_title(&title)?;
    }
    if let Some(model_id) = input.model_definition_id {
        let model = db::get_model(pool, &model_id).await?;
        if model.modality != Modality::Text {
            return Err(AppError::unsupported("对话目前只支持文本模型"));
        }
        row.model_id = model.id;
    }
    if let Some(system) = input.system_prompt {
        row.system_prompt = bounded_text(&system, "System Prompt")?;
    }
    let strategy = match input.context_strategy.as_deref() {
        Some(value) => {
            let strategy = parse_strategy(value)?;
            row.context_strategy = strategy.as_str().to_string();
            strategy
        }
        None => parse_strategy(&row.context_strategy)?,
    };
    let limit = input.context_limit.unwrap_or(row.context_limit);
    row.context_limit = crate::services::context::normalize_limit(strategy, limit);
    touch(pool, &mut row).await?;
    Ok(to_conversation(&row))
}

pub async fn delete(pool: &SqlitePool, cancels: &Cancels, id: &str) -> Result<(), AppError> {
    cancel(cancels, id);
    db::delete_conversation(pool, id).await
}

pub async fn delete_message(
    pool: &SqlitePool,
    cancels: &Cancels,
    conversation_id: &str,
    message_id: &str,
) -> Result<ConversationDetail, AppError> {
    cancel(cancels, conversation_id);
    let mut conversation = db::get_conversation(pool, conversation_id).await?;
    let message = db::get_message(pool, message_id).await?;
    if message.conversation_id != conversation.id {
        return Err(AppError::validation("消息不属于这个对话"));
    }
    let mut tx = pool.begin().await.map_err(AppError::db)?;
    db::delete_messages_after_conn(
        &mut *tx,
        &conversation.id,
        &message.created_at,
        &message.id,
        true,
    )
    .await?;
    conversation.summary = None;
    conversation.summary_until = None;
    conversation.updated_at = now_iso();
    db::update_conversation_conn(&mut *tx, &conversation).await?;
    tx.commit().await.map_err(AppError::db)?;
    detail(pool, conversation).await
}

#[allow(dead_code)]
pub async fn send(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    cancels: &Cancels,
    data_dir: &Path,
    input: ChatSendInput,
    on_notice: &NoticeFn,
) -> Result<ChatTurnResult, AppError> {
    send_with_scheduler(
        pool, http, secrets, cancels, data_dir, input, on_notice, None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn send_with_scheduler(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    cancels: &Cancels,
    data_dir: &Path,
    input: ChatSendInput,
    on_notice: &NoticeFn,
    scheduler: Option<&crate::services::task_scheduler::TaskScheduler>,
) -> Result<ChatTurnResult, AppError> {
    let pre_registered = input.conversation_id.as_ref().map(|id| {
        let flag = register(cancels, id);
        CancelGuard {
            cancels,
            id: id.clone(),
            flag,
        }
    });
    let pre_cancel = pre_registered.as_ref().map(|guard| Arc::clone(&guard.flag));
    let model = db::get_model(pool, &input.model_definition_id).await?;
    let provider = db::get_provider(pool, &model.provider_id).await?;
    let env = Env {
        pool,
        http,
        secrets,
        cancels,
        data_dir,
    };
    send_with(
        &env, &provider, &model, input, on_notice, scheduler, pre_cancel,
    )
    .await
    .map_err(|err| err.annotate(&provider.name, &model.name))
}

struct Env<'a> {
    pool: &'a SqlitePool,
    http: &'a Client,
    secrets: &'a dyn SecretStore,
    cancels: &'a Cancels,
    data_dir: &'a Path,
}

async fn send_with(
    env: &Env<'_>,
    provider: &ProviderRecord,
    model: &crate::domain::ModelDefinition,
    input: ChatSendInput,
    on_notice: &NoticeFn,
    scheduler: Option<&crate::services::task_scheduler::TaskScheduler>,
    pre_cancel: Option<Arc<AtomicBool>>,
) -> Result<ChatTurnResult, AppError> {
    let pool = env.pool;
    let http = env.http;
    let secrets = env.secrets;
    let cancels = env.cancels;
    let prepared = prepare_input(provider, model, secrets, &input)?;
    if input.conversation_id.is_none() && input.replace_from_message_id.is_some() {
        return Err(AppError::validation("找不到要替换的消息"));
    }
    let vision = capability_enabled(&model.capabilities, "vision");
    if input.replace_from_message_id.is_none() {
        validate_user_text(&input.prompt)?;
        if !input.images.is_empty() && !vision {
            return Err(AppError::unsupported("该模型没有图像输入能力"));
        }
    }
    let (mut conversation, is_new) = open_conversation(pool, model, &input, &prepared).await?;
    let (messages, anchor, replace_plan) =
        match prepare_turn(pool, &conversation, &input, is_new, vision).await {
            Ok(res) => res,
            Err(err) => {
                if is_new {
                    let _ = db::delete_conversation(pool, &conversation.id).await;
                }
                return Err(err);
            }
        };
    if input.replace_from_message_id.is_none() && conversation.title == "新对话" {
        if let Some(text) = messages
            .iter()
            .find(|item| item.role == "user")
            .map(|item| item.text.clone())
        {
            let title = auto_title(&text);
            if title != "新对话" {
                conversation.title = title;
            }
        }
    }
    conversation.model_id = model.id.clone();
    conversation.system_prompt = prepared.system.clone();
    conversation.context_strategy = prepared.strategy.as_str().to_string();
    conversation.context_limit = prepared.limit;
    conversation.mode = if prepared.tools.is_empty() {
        "chat".to_string()
    } else {
        "agent".to_string()
    };
    touch(pool, &mut conversation).await?;

    let flag = pre_cancel.unwrap_or_else(|| register(cancels, &conversation.id));
    let _guard = CancelGuard {
        cancels,
        id: conversation.id.clone(),
        flag: Arc::clone(&flag),
    };
    let _scheduler_permit = if let Some(scheduler) = scheduler {
        Some(
            scheduler
                .acquire_cancellable(Modality::Text, false, Some(&flag))
                .await?,
        )
    } else {
        None
    };
    if flag.load(Ordering::Relaxed) {
        return Err(AppError::new("cancelled", "对话已取消"));
    }

    let execute_flow = async {
        if !notify(
            on_notice,
            &flag,
            ChatNotice::Started {
                conversation: to_conversation(&conversation),
                messages: messages.clone(),
            },
        ) {
            return Err(AppError::validation("输出通道已关闭"));
        }

        let host = Host {
            http,
            secrets,
            provider,
            model,
        };
        let (plan, summary_error) = assemble_context(
            &host,
            &prepared.parameters,
            &flag,
            &mut conversation,
            &messages,
        )
        .await?;
        if plan.turns.is_empty() {
            return Err(AppError::validation("没有可发送的消息"));
        }
        if plan.turns.iter().any(|turn| !turn.images.is_empty())
            && !capability_enabled(&model.capabilities, "vision")
        {
            return Err(AppError::unsupported("该模型没有图像输入能力"));
        }
        if flag.load(Ordering::Relaxed) {
            return Err(AppError::validation("已停止"));
        }
        // Context assembly may calculate a replacement summary on the local
        // conversation row. Keep that change speculative until the replacement
        // response is committed; a cancelled turn must retain the old summary.
        if matches!(&replace_plan, ReplacePlan::None) {
            touch(pool, &mut conversation).await?;
        }

        let memory_block = crate::services::memory::format_for_prompt(pool, Some(&conversation.id))
            .await
            .unwrap_or_default();
        let base_system = compose_system(&conversation.system_prompt, plan.summary.as_deref());
        let full_system = match (base_system, memory_block.is_empty()) {
            (Some(sys), false) => Some(format!("{sys}\n\n{memory_block}")),
            (Some(sys), true) => Some(sys),
            (None, false) => Some(memory_block),
            (None, true) => None,
        };

        let payload = TextPayload {
            system_prompt: full_system,
            turns: plan.turns,
            parameters: prepared.parameters,
            extra: input.extra.filter(|value| !value.is_null()),
            tools: Vec::new(),
        };
        let note = TurnNote {
            prompt: payload
                .turns
                .iter()
                .rev()
                .find(|turn| turn.role == "user")
                .map(|turn| turn.text.clone())
                .unwrap_or_default(),
            system_prompt: {
                let text = conversation.system_prompt.trim();
                if text.is_empty() {
                    None
                } else {
                    Some(text.to_string())
                }
            },
            images: payload
                .turns
                .iter()
                .filter(|turn| turn.role == "user")
                .flat_map(|turn| turn.images.iter().cloned())
                .collect(),
            extra: payload.extra.clone(),
            conversation_id: conversation.id.clone(),
            parameters: payload.parameters.clone(),
        };
        let outcome = if prepared.tools.is_empty() {
            complete(
                &host,
                Arc::clone(&flag),
                payload,
                capability_enabled(&model.capabilities, "streaming"),
                on_notice,
            )
            .await
            .map(|result| Reply {
                result,
                logs: Vec::new(),
                max_steps_hit: false,
            })
        } else {
            let ctx = context_for(&host, Some(Arc::clone(&flag)))?;
            let mcp = if prepared.tools.iter().any(|name| name == "mcp") {
                crate::services::mcp::tools_for_agent(env.pool, env.http).await?
            } else {
                Vec::new()
            };
            agent::run(
                AgentTurn {
                    ctx: &ctx,
                    provider_type: provider.provider_type,
                    data_dir: env.data_dir,
                    workspace_root: None,
                    allow_trusted_local: cfg!(test)
                        || std::env::var("MODELDECK_ALLOW_LOCAL_NETWORK")
                            .map(|v| v == "1")
                            .unwrap_or(false),
                    flag: &flag,
                    mcp,
                    pool: Some(env.pool),
                    confirmed_tools: prepared.confirmed_tools.clone(),
                },
                payload,
                &prepared.tools,
                prepared.max_steps,
                &|event| {
                    let notice = match event {
                        AgentEvent::Delta(text) => ChatNotice::Delta { text },
                        AgentEvent::Reasoning(text) => ChatNotice::Reasoning { text },
                        AgentEvent::Tool(log) => ChatNotice::Tool { log },
                    };
                    let _ = notify(on_notice, &flag, notice);
                },
            )
            .await
            .map(|reply| Reply {
                result: reply.result,
                logs: reply.logs,
                max_steps_hit: reply.max_steps_hit,
            })
        };
        let projected = match &outcome {
            Ok(reply) => Ok(reply.result.clone()),
            Err(err) => Err(err.clone()),
        };
        let record_res = remember(pool, env.data_dir, provider, model, &note, &projected).await;
        let reply = outcome?;
        if let Err(record_err) = record_res {
            return Err(AppError::storage(format!(
                "聊天回复生成成功，但持久化任务历史失败: {}。回复内容预览: {}",
                record_err.message,
                reply.result.text.chars().take(100).collect::<String>()
            )));
        }
        if reply.result.text.is_empty()
            && reply.result.reasoning.is_empty()
            && reply.logs.is_empty()
        {
            if reply.result.stopped {
                return Err(AppError::validation("已停止"));
            }
            let message = reply
                .result
                .interrupted
                .clone()
                .unwrap_or_else(|| "模型没有返回内容".to_string());
            return Err(AppError::provider(
                message,
                reply.result.status,
                &provider.name,
                Some(&model.name),
                None,
            ));
        }
        if !matches!(&replace_plan, ReplacePlan::None)
            && (reply.result.stopped || reply.result.interrupted.is_some())
        {
            return Err(AppError::validation("生成已停止或中断，原有消息未被替换"));
        }
        ensure_anchor(pool, &conversation.id, &anchor).await?;
        let assistant =
            insert_assistant_and_touch(pool, &mut conversation, &reply, &replace_plan, &anchor)
                .await?;
        Ok(ChatTurnResult {
            conversation: to_conversation(&conversation),
            assistant,
            context_tokens: plan.tokens,
            summary_error,
        })
    };

    match execute_flow.await {
        Ok(result) => Ok(result),
        Err(err) => {
            if is_new {
                let _ = db::delete_conversation(pool, &conversation.id).await;
            }
            Err(err)
        }
    }
}

struct Prepared {
    system: String,
    strategy: ContextStrategy,
    limit: u32,
    parameters: Map<String, Value>,
    tools: Vec<String>,
    confirmed_tools: Vec<String>,
    max_steps: u32,
}

struct Reply {
    result: TextResult,
    logs: Vec<ToolLog>,
    max_steps_hit: bool,
}

fn prepare_input(
    provider: &ProviderRecord,
    model: &crate::domain::ModelDefinition,
    secrets: &dyn SecretStore,
    input: &ChatSendInput,
) -> Result<Prepared, AppError> {
    if !model.enabled {
        return Err(AppError::validation("模型已停用"));
    }
    if !provider.enabled {
        return Err(AppError::validation("Provider 已停用"));
    }
    if model.modality != Modality::Text {
        return Err(AppError::unsupported("当前入口只执行文本生成"));
    }
    if input.images.len() > 8 {
        return Err(AppError::validation("一次最多附加 8 张图像"));
    }
    if let Some(extra) = &input.extra {
        if !extra.is_object() {
            return Err(AppError::validation("Extra JSON 必须是对象"));
        }
        let size = serde_json::to_string(extra)
            .map(|text| text.len())
            .unwrap_or(0);
        if size > 100_000 {
            return Err(AppError::validation("Extra JSON 过大"));
        }
    }
    let system = bounded_text(
        input.system_prompt.as_deref().unwrap_or(""),
        "System Prompt",
    )?;
    let strategy = parse_strategy(&input.context_strategy)?;
    let limit = crate::services::context::normalize_limit(strategy, input.context_limit);
    let parameters =
        normalize_parameters(&model.parameters, &input.parameters, &model.constraints)?;
    for definition in &model.parameters {
        if definition.kind.is_media()
            && parameters
                .get(&definition.key)
                .is_some_and(|value| !is_empty_value(Some(value)))
        {
            return Err(AppError::unsupported(format!(
                "参数「{}」是媒体输入。文本生成请使用模型的图像输入能力，或清空该参数",
                definition.label
            )));
        }
    }
    let api_key = load_secret(secrets, provider.credential_id.as_deref())?;
    if provider.provider_type.requires_api_key() && api_key.is_none() {
        return Err(AppError::auth(
            "缺少 API Key",
            &provider.name,
            Some(&model.name),
        ));
    }
    let tools = agent::normalize_tools(&input.tools)?;
    let max_steps = agent::normalize_steps(!tools.is_empty(), input.max_steps)?;
    if !tools.is_empty()
        && !matches!(
            provider.provider_type,
            ProviderType::Openai
                | ProviderType::OpenaiCompatible
                | ProviderType::Ollama
                | ProviderType::Anthropic
                | ProviderType::Gemini
        )
    {
        return Err(AppError::unsupported(
            "Agent 工具调用目前由 OpenAI、OpenAI Compatible、Ollama、Anthropic 与 Gemini 适配器执行",
        ));
    }
    Ok(Prepared {
        system,
        strategy,
        limit,
        parameters,
        tools,
        confirmed_tools: input.confirmed_tools.clone(),
        max_steps,
    })
}

async fn open_conversation(
    pool: &SqlitePool,
    model: &crate::domain::ModelDefinition,
    input: &ChatSendInput,
    prepared: &Prepared,
) -> Result<(ConversationRow, bool), AppError> {
    if let Some(id) = &input.conversation_id {
        return Ok((db::get_conversation(pool, id).await?, false));
    }
    let now = now_iso();
    let row = ConversationRow {
        id: Uuid::new_v4().to_string(),
        title: "新对话".to_string(),
        mode: "chat".to_string(),
        model_id: model.id.clone(),
        system_prompt: prepared.system.clone(),
        context_strategy: prepared.strategy.as_str().to_string(),
        context_limit: prepared.limit,
        summary: None,
        summary_until: None,
        created_at: now.clone(),
        updated_at: now,
    };
    db::insert_conversation(pool, &row).await?;
    Ok((row, true))
}

enum ReplacePlan {
    None,
    Assistant {
        target_message_id: String,
        target_created_at: String,
        expected_tail_id: String,
    },
    User {
        updated_user_message: crate::db::MessageRow,
        target_message_id: String,
        target_created_at: String,
        expected_tail_id: String,
    },
}

async fn prepare_turn(
    pool: &SqlitePool,
    conversation: &ConversationRow,
    input: &ChatSendInput,
    is_new: bool,
    vision: bool,
) -> Result<(Vec<ChatMessage>, String, ReplacePlan), AppError> {
    let replacing = match &input.replace_from_message_id {
        Some(id) => Some(db::get_message(pool, id).await?),
        None => None,
    };
    if let Some(message) = &replacing {
        if message.conversation_id != conversation.id {
            return Err(AppError::validation("消息不属于这个对话"));
        }
    }
    let role = replacing.as_ref().map(|message| message.role.as_str());
    if role != Some("assistant") {
        validate_user_text(&input.prompt)?;
        if !input.images.is_empty() && !vision {
            return Err(AppError::unsupported("该模型没有图像输入能力"));
        }
    }
    match role {
        Some("assistant") => {
            let message = replacing.ok_or_else(|| AppError::validation("找不到要替换的消息"))?;
            let all_messages = read_messages(pool, &conversation.id).await?;
            let target_idx = all_messages
                .iter()
                .position(|m| m.id == message.id)
                .ok_or_else(|| AppError::validation("找不到要替换的消息"))?;
            let messages = all_messages[..target_idx].to_vec();
            let anchor = messages
                .last()
                .map(|item| item.id.clone())
                .ok_or_else(|| AppError::validation("没有可发送的消息"))?;
            let plan = ReplacePlan::Assistant {
                target_message_id: message.id,
                target_created_at: message.created_at,
                expected_tail_id: all_messages
                    .last()
                    .map(|item| item.id.clone())
                    .ok_or_else(|| AppError::validation("没有可替换的消息"))?,
            };
            Ok((messages, anchor, plan))
        }
        Some("user") => {
            let mut message =
                replacing.ok_or_else(|| AppError::validation("找不到要替换的消息"))?;
            let all_messages = read_messages(pool, &conversation.id).await?;
            let target_idx = all_messages
                .iter()
                .position(|m| m.id == message.id)
                .ok_or_else(|| AppError::validation("找不到要替换的消息"))?;
            message.content_json = user_json(&input.prompt, &input.images)?;
            message.token_count = user_tokens(&input.prompt, input.images.len());
            message.raw_request_json = None;
            message.raw_response_json = None;
            let updated_chat_msg = read_message(&message)?;
            let mut messages = all_messages[..target_idx].to_vec();
            let anchor = message.id.clone();
            messages.push(updated_chat_msg);
            let plan = ReplacePlan::User {
                updated_user_message: message.clone(),
                target_message_id: message.id,
                target_created_at: message.created_at,
                expected_tail_id: all_messages
                    .last()
                    .map(|item| item.id.clone())
                    .ok_or_else(|| AppError::validation("没有可替换的消息"))?,
            };
            Ok((messages, anchor, plan))
        }
        Some(_) => Err(AppError::validation("消息角色无效")),
        None => {
            if let Err(err) = insert_user(pool, &conversation.id, input).await {
                if is_new {
                    let _ = db::delete_conversation(pool, &conversation.id).await;
                }
                return Err(err);
            }
            let messages = read_messages(pool, &conversation.id).await?;
            let anchor = messages
                .last()
                .map(|item| item.id.clone())
                .ok_or_else(|| AppError::validation("没有可发送的消息"))?;
            Ok((messages, anchor, ReplacePlan::None))
        }
    }
}

fn validate_user_text(prompt: &str) -> Result<(), AppError> {
    if prompt.trim().is_empty() {
        return Err(AppError::validation("提示词不能为空"));
    }
    if prompt.chars().count() > 100_000 {
        return Err(AppError::validation("提示词过长"));
    }
    Ok(())
}

fn forget_summary(conversation: &mut ConversationRow) {
    conversation.summary = None;
    conversation.summary_until = None;
}

struct Host<'a> {
    http: &'a Client,
    secrets: &'a dyn SecretStore,
    provider: &'a ProviderRecord,
    model: &'a crate::domain::ModelDefinition,
}

async fn assemble_context(
    host: &Host<'_>,
    parameters: &Map<String, Value>,
    flag: &Arc<AtomicBool>,
    conversation: &mut ConversationRow,
    messages: &[ChatMessage],
) -> Result<(ContextPlan, Option<String>), AppError> {
    let items: Vec<ContextItem> = messages.iter().map(to_item).collect();
    let strategy = parse_strategy(&conversation.context_strategy)?;
    let mut plan = plan_context(
        &items,
        strategy,
        conversation.context_limit,
        &conversation.system_prompt,
        conversation.summary.as_deref(),
        conversation.summary_until.as_deref(),
    );
    if plan.older.is_empty() {
        return Ok((plan, None));
    }
    if flag.load(Ordering::Relaxed) {
        return Err(AppError::validation("已停止"));
    }
    match summarize(host, parameters, flag, &plan.older).await {
        Ok(text) => {
            conversation.summary = Some(text);
            conversation.summary_until = plan.older.last().map(|item| item.id.clone());
            plan = plan_context(
                &items,
                strategy,
                conversation.context_limit,
                &conversation.system_prompt,
                conversation.summary.as_deref(),
                conversation.summary_until.as_deref(),
            );
            Ok((plan, None))
        }
        Err(err) => {
            forget_summary(conversation);
            let fallback = plan_context(
                &items,
                ContextStrategy::Recent,
                conversation.context_limit,
                &conversation.system_prompt,
                None,
                None,
            );
            Ok((fallback, Some(err.message)))
        }
    }
}

async fn summarize(
    host: &Host<'_>,
    parameters: &Map<String, Value>,
    flag: &Arc<AtomicBool>,
    older: &[ContextItem],
) -> Result<String, AppError> {
    let request = CanonicalRequest::Text(TextPayload {
        system_prompt: Some(SUMMARY_PROMPT.to_string()),
        turns: vec![TextTurn::plain("user", transcript(older), Vec::new())],
        parameters: parameters.clone(),
        extra: None,
        tools: Vec::new(),
    });
    let ctx = context_for(host, Some(Arc::clone(flag)))?;
    let CanonicalResponse::Text(result) = adapter_for(host.provider.provider_type)
        .execute(&ctx, request)
        .await?
    else {
        return Err(AppError::unsupported("文本入口收到了图像结果"));
    };
    let text = result.text.trim().to_string();
    if text.is_empty() {
        return Err(AppError::provider(
            "摘要为空",
            result.status,
            &host.provider.name,
            Some(&host.model.name),
            None,
        ));
    }
    Ok(text)
}

struct TurnNote {
    prompt: String,
    system_prompt: Option<String>,
    images: Vec<MediaRef>,
    extra: Option<Value>,
    conversation_id: String,
    parameters: Map<String, Value>,
}

async fn remember(
    pool: &SqlitePool,
    data_dir: &Path,
    provider: &ProviderRecord,
    model: &crate::domain::ModelDefinition,
    note: &TurnNote,
    outcome: &Result<TextResult, AppError>,
) -> Result<String, AppError> {
    let (status, error, output_text, raw_request, raw_response) = match outcome {
        Ok(result) => {
            let (status, error, output_text) = classify(result);
            (
                status,
                error,
                output_text,
                Some(result.raw_request.clone()),
                Some(result.raw_response.clone()),
            )
        }
        Err(err) => (
            "failed".to_string(),
            Some(err.message.clone()),
            None,
            None,
            err.raw.as_deref().cloned(),
        ),
    };
    history::record_text_task(
        pool,
        data_dir,
        NewTextTask {
            kind: "chat".to_string(),
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            model_id: model.id.clone(),
            model_name: model.name.clone(),
            model_api_id: model.model_id.clone(),
            prompt: note.prompt.clone(),
            system_prompt: note.system_prompt.clone(),
            images: note.images.clone(),
            extra: note.extra.clone(),
            conversation_id: Some(note.conversation_id.clone()),
            parameters: note.parameters.clone(),
            status,
            output_text,
            raw_request,
            raw_response,
            error,
        },
    )
    .await
}

fn classify(result: &TextResult) -> (String, Option<String>, Option<String>) {
    let output = if !result.text.is_empty() {
        Some(result.text.clone())
    } else if !result.reasoning.is_empty() {
        Some(result.reasoning.clone())
    } else {
        None
    };
    if result.stopped {
        return ("cancelled".to_string(), None, output);
    }
    if let Some(message) = &result.interrupted {
        return ("failed".to_string(), Some(message.clone()), output);
    }
    if output.is_none() {
        return (
            "failed".to_string(),
            Some("模型没有返回内容".to_string()),
            None,
        );
    }
    ("completed".to_string(), None, output)
}

async fn complete(
    host: &Host<'_>,
    flag: Arc<AtomicBool>,
    payload: TextPayload,
    streaming: bool,
    on_notice: &NoticeFn,
) -> Result<TextResult, AppError> {
    let ctx = context_for(host, Some(Arc::clone(&flag)))?;
    let adapter = adapter_for(host.provider.provider_type);
    if streaming {
        let on_delta = |delta: LiveDelta| {
            let notice = match delta {
                LiveDelta::Text(text) => ChatNotice::Delta { text },
                LiveDelta::Reasoning(text) => ChatNotice::Reasoning { text },
            };
            let _ = notify(on_notice, &flag, notice);
        };
        adapter
            .stream(&ctx, CanonicalRequest::Text(payload), &on_delta)
            .await
    } else {
        let CanonicalResponse::Text(result) = adapter
            .execute(&ctx, CanonicalRequest::Text(payload))
            .await?
        else {
            return Err(AppError::unsupported("文本入口收到了图像结果"));
        };
        if !result.reasoning.is_empty() {
            let _ = notify(
                on_notice,
                &flag,
                ChatNotice::Reasoning {
                    text: result.reasoning.clone(),
                },
            );
        }
        if !result.text.is_empty() {
            let _ = notify(
                on_notice,
                &flag,
                ChatNotice::Delta {
                    text: result.text.clone(),
                },
            );
        }
        Ok(result)
    }
}

fn context_for(host: &Host<'_>, flag: Option<Arc<AtomicBool>>) -> Result<AdapterContext, AppError> {
    let api_key = load_secret(host.secrets, host.provider.credential_id.as_deref())?;
    let (chat_path, models_path) = match &host.model.transport {
        Some(transport) => (transport.chat_path.clone(), transport.models_path.clone()),
        None => (None, None),
    };
    let mut ctx = AdapterContext::new(
        host.http.clone(),
        host.provider.clone(),
        Some(host.model.clone()),
        api_key,
        chat_path,
        models_path,
    );
    if let Some(flag) = flag {
        ctx.cancelled = flag;
    }
    Ok(ctx)
}

async fn ensure_anchor(
    pool: &SqlitePool,
    conversation_id: &str,
    anchor: &str,
) -> Result<(), AppError> {
    match db::get_message(pool, anchor).await {
        Ok(message) if message.conversation_id == conversation_id => Ok(()),
        Ok(_) => Err(AppError::validation("对话已变更，没有保存回复")),
        Err(err) if err.code == NOT_FOUND => Err(AppError::validation("对话已变更，没有保存回复")),
        Err(err) => Err(err),
    }
}

async fn insert_user(
    pool: &SqlitePool,
    conversation_id: &str,
    input: &ChatSendInput,
) -> Result<(), AppError> {
    let row = MessageRow {
        id: Uuid::new_v4().to_string(),
        conversation_id: conversation_id.to_string(),
        role: "user".to_string(),
        content_json: user_json(&input.prompt, &input.images)?,
        token_count: user_tokens(&input.prompt, input.images.len()),
        raw_request_json: None,
        raw_response_json: None,
        is_pinned: false,
        created_at: now_iso(),
    };
    db::insert_message(pool, &row).await
}

async fn insert_assistant_and_touch(
    pool: &SqlitePool,
    conversation: &mut ConversationRow,
    reply: &Reply,
    replace_plan: &ReplacePlan,
    anchor: &str,
) -> Result<ChatMessage, AppError> {
    let result = &reply.result;
    let stored = StoredMessage {
        text: result.text.clone(),
        images: Vec::new(),
        reasoning: result.reasoning.clone(),
        usage: result.usage.clone(),
        stopped: result.stopped,
        error: result.interrupted.clone(),
        tool_logs: reply.logs.clone(),
        max_steps_hit: reply.max_steps_hit,
    };
    let tokens = result
        .usage
        .as_ref()
        .and_then(|usage| usage.output_tokens)
        .unwrap_or_else(|| {
            crate::services::context::estimate_tokens(&result.text)
                + crate::services::context::estimate_tokens(&result.reasoning)
        });
    let now = now_iso();
    let row = MessageRow {
        id: Uuid::new_v4().to_string(),
        conversation_id: conversation.id.clone(),
        role: "assistant".to_string(),
        content_json: to_json(&stored)?,
        token_count: as_i64(tokens),
        raw_request_json: serde_json::to_string(&result.raw_request).ok(),
        raw_response_json: serde_json::to_string(&result.raw_response).ok(),
        is_pinned: false,
        created_at: now.clone(),
    };
    let mut tx = pool.begin().await.map_err(AppError::db)?;
    let expected_tail_id: &str = match replace_plan {
        ReplacePlan::None => anchor,
        ReplacePlan::Assistant {
            expected_tail_id, ..
        }
        | ReplacePlan::User {
            expected_tail_id, ..
        } => expected_tail_id,
    };
    let actual_tail_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM messages WHERE conversation_id = ? ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(&conversation.id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::db)?;
    if actual_tail_id.as_deref() != Some(expected_tail_id) {
        return Err(AppError::validation("对话已变更，没有保存回复"));
    }
    match replace_plan {
        ReplacePlan::None => {}
        ReplacePlan::Assistant {
            target_message_id,
            target_created_at,
            ..
        } => {
            db::delete_messages_after_conn(
                &mut *tx,
                &conversation.id,
                target_created_at,
                target_message_id,
                true,
            )
            .await?;
            forget_summary(conversation);
        }
        ReplacePlan::User {
            updated_user_message,
            target_message_id,
            target_created_at,
            ..
        } => {
            db::update_message_conn(&mut *tx, updated_user_message).await?;
            db::delete_messages_after_conn(
                &mut *tx,
                &conversation.id,
                target_created_at,
                target_message_id,
                false,
            )
            .await?;
            forget_summary(conversation);
        }
    }
    db::insert_message_conn(&mut *tx, &row).await?;
    conversation.updated_at = now;
    sqlx::query(
        "UPDATE conversations SET summary = ?, summary_until = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&conversation.summary)
    .bind(&conversation.summary_until)
    .bind(&conversation.updated_at)
    .bind(&conversation.id)
    .execute(&mut *tx)
    .await
    .map_err(AppError::db)?;
    tx.commit().await.map_err(AppError::db)?;
    *conversation = db::get_conversation(pool, &conversation.id).await?;
    read_message(&row)
}

fn user_json(text: &str, images: &[crate::domain::MediaRef]) -> Result<String, AppError> {
    to_json(&StoredMessage {
        text: text.to_string(),
        images: images.iter().map(|image| image.path.clone()).collect(),
        reasoning: String::new(),
        usage: None,
        stopped: false,
        error: None,
        tool_logs: Vec::new(),
        max_steps_hit: false,
    })
}

fn user_tokens(text: &str, images: usize) -> i64 {
    as_i64(crate::services::context::estimate_tokens(text) + images as u64 * 256)
}

async fn read_messages(
    pool: &SqlitePool,
    conversation_id: &str,
) -> Result<Vec<ChatMessage>, AppError> {
    let rows = db::list_messages(pool, conversation_id).await?;
    rows.iter().map(read_message).collect()
}

async fn detail(
    pool: &SqlitePool,
    conversation: ConversationRow,
) -> Result<ConversationDetail, AppError> {
    let messages = read_messages(pool, &conversation.id).await?;
    Ok(ConversationDetail {
        conversation: to_conversation(&conversation),
        messages,
    })
}

async fn touch(pool: &SqlitePool, row: &mut ConversationRow) -> Result<(), AppError> {
    row.updated_at = now_iso();
    db::update_conversation(pool, row).await
}

fn read_message(row: &MessageRow) -> Result<ChatMessage, AppError> {
    let stored: StoredMessage = serde_json::from_str(&row.content_json)
        .map_err(|err| AppError::storage(format!("消息已损坏: {err}")))?;
    Ok(ChatMessage {
        id: row.id.clone(),
        role: row.role.clone(),
        text: stored.text,
        images: stored.images,
        reasoning: stored.reasoning,
        usage: stored.usage,
        stopped: stored.stopped,
        error: stored.error,
        tool_logs: stored.tool_logs,
        max_steps_hit: stored.max_steps_hit,
        is_pinned: row.is_pinned,
        token_count: u64::try_from(row.token_count).unwrap_or(0),
        created_at: row.created_at.clone(),
    })
}

fn to_item(message: &ChatMessage) -> ContextItem {
    ContextItem {
        id: message.id.clone(),
        role: message.role.clone(),
        text: message.text.clone(),
        images: message.images.clone(),
        is_pinned: message.is_pinned,
    }
}

fn to_conversation(row: &ConversationRow) -> Conversation {
    Conversation {
        id: row.id.clone(),
        title: row.title.clone(),
        model_definition_id: row.model_id.clone(),
        system_prompt: row.system_prompt.clone(),
        context_strategy: row.context_strategy.clone(),
        context_limit: row.context_limit,
        summary: row.summary.clone(),
        created_at: row.created_at.clone(),
        updated_at: row.updated_at.clone(),
    }
}

fn to_json(value: &impl Serialize) -> Result<String, AppError> {
    serde_json::to_string(value).map_err(|err| AppError::storage(format!("无法保存 JSON: {err}")))
}

fn bounded_text(value: &str, label: &str) -> Result<String, AppError> {
    let text = value.trim().to_string();
    if text.chars().count() > 100_000 {
        return Err(AppError::validation(format!("{label}过长")));
    }
    Ok(text)
}

fn clean_title(value: &str) -> Result<String, AppError> {
    let title = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() || title.chars().count() > 80 {
        return Err(AppError::validation("标题需要 1 到 80 个字符"));
    }
    Ok(title)
}

fn as_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn register(cancels: &Cancels, id: &str) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    let mut map = cancels.lock().unwrap_or_else(|err| err.into_inner());
    if let Some(previous) = map.insert(id.to_string(), Arc::clone(&flag)) {
        previous.store(true, Ordering::Relaxed);
    }
    flag
}

fn notify(on_notice: &NoticeFn, flag: &AtomicBool, notice: ChatNotice) -> bool {
    if on_notice(notice) {
        true
    } else {
        flag.store(true, Ordering::Relaxed);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::classify;
    use crate::canonical::TextResult;
    use serde_json::json;

    fn sample(text: &str, stopped: bool, interrupted: Option<&str>, reasoning: &str) -> TextResult {
        TextResult {
            text: text.to_string(),
            usage: None,
            latency_ms: 1,
            provider_id: "p".to_string(),
            provider_name: "P".to_string(),
            model_definition_id: "m".to_string(),
            model_name: "M".to_string(),
            model_id: "api".to_string(),
            raw_request: json!({}),
            raw_response: json!({}),
            status: 200,
            stopped,
            reasoning: reasoning.to_string(),
            interrupted: interrupted.map(str::to_string),
            tool_calls: Vec::new(),
        }
    }

    #[test]
    fn classifies_terminal_text() {
        let (status, error, output) = classify(&sample("hi", false, None, ""));
        assert_eq!(status, "completed");
        assert!(error.is_none());
        assert_eq!(output.as_deref(), Some("hi"));

        let (status, error, output) = classify(&sample("partial", true, None, ""));
        assert_eq!(status, "cancelled");
        assert!(error.is_none());
        assert_eq!(output.as_deref(), Some("partial"));

        let (status, _, output) = classify(&sample("", true, None, ""));
        assert_eq!(status, "cancelled");
        assert!(output.is_none());

        let (status, error, output) = classify(&sample("partial", false, Some("boom"), ""));
        assert_eq!(status, "failed");
        assert_eq!(error.as_deref(), Some("boom"));
        assert_eq!(output.as_deref(), Some("partial"));

        let (status, error, output) = classify(&sample("", false, None, "think"));
        assert_eq!(status, "completed");
        assert!(error.is_none());
        assert_eq!(output.as_deref(), Some("think"));

        let (status, error, output) = classify(&sample("", false, None, ""));
        assert_eq!(status, "failed");
        assert_eq!(error.as_deref(), Some("模型没有返回内容"));
        assert!(output.is_none());
    }
}
