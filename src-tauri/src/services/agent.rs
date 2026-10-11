use std::net::{IpAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Map, Value};
use tokio::process::Command;
use tokio::time::timeout;

use crate::adapters::adapter_for;
use crate::canonical::{
    AdapterContext, CanonicalRequest, CanonicalResponse, TextPayload, TextResult, TextTurn,
    ToolCall, ToolLog, ToolSpec,
};
use crate::domain::ProviderType;
use crate::error::AppError;

const HINT: &str = "可以使用提供的工具。根据工具结果继续回答，不要编造工具没有返回的内容。";
const MAX_CALLS: usize = 8;
const MAX_ARG_CHARS: usize = 8_000;
const MAX_RESULT_CHARS: usize = 8_000;
const MAX_FILE_BYTES: usize = 64 * 1024;
const MAX_ENTRIES: usize = 200;

pub struct AgentTurn<'a> {
    pub ctx: &'a AdapterContext,
    pub provider_type: ProviderType,
    pub data_dir: &'a Path,
    pub workspace_root: Option<&'a Path>,
    pub allow_trusted_local: bool,
    pub flag: &'a AtomicBool,
    pub mcp: Vec<crate::services::mcp::ExposedTool>,
    pub pool: Option<&'a sqlx::SqlitePool>,
    pub confirmed_tools: Vec<String>,
}

pub enum AgentEvent {
    Delta(String),
    Reasoning(String),
    Tool(ToolLog),
}

pub struct AgentReply {
    pub result: TextResult,
    pub logs: Vec<ToolLog>,
    pub max_steps_hit: bool,
}

pub fn normalize_tools(names: &[String]) -> Result<Vec<String>, AppError> {
    let mut chosen = Vec::new();
    for name in names {
        let name = name.trim();
        if name != "mcp" && spec(name).is_none() {
            return Err(AppError::validation(format!("未知工具「{name}」")));
        }
        if chosen.iter().any(|item: &String| item == name) {
            return Err(AppError::validation("工具重复"));
        }
        chosen.push(name.to_string());
    }
    Ok(chosen)
}

pub fn normalize_steps(enabled: bool, value: Option<u32>) -> Result<u32, AppError> {
    if !enabled {
        return Ok(0);
    }
    match value {
        None => Ok(10),
        Some(steps) if (1..=20).contains(&steps) => Ok(steps),
        Some(_) => Err(AppError::validation("最大步数需要在 1 到 20 之间")),
    }
}

pub async fn run(
    mut turn: AgentTurn<'_>,
    mut payload: TextPayload,
    tool_names: &[String],
    max_steps: u32,
    on_event: &(dyn Fn(AgentEvent) + Send + Sync),
) -> Result<AgentReply, AppError> {
    let mut tools = tool_names
        .iter()
        .filter(|name| name.as_str() != "mcp")
        .filter_map(|name| spec(name))
        .collect::<Vec<_>>();
    for item in &turn.mcp {
        tools.push(item.spec.clone());
    }
    if tools.is_empty() {
        return Err(AppError::validation("没有可调用的工具"));
    }
    payload.system_prompt = with_hint(payload.system_prompt.take());
    payload.tools = tools.clone();
    let mut logs = Vec::new();
    let mut carried: Option<TextResult> = None;
    for step in 1..=max_steps {
        if turn.flag.load(Ordering::Relaxed) {
            return stopped(logs, carried);
        }
        let result = call_model(&turn, &payload).await?;
        if !result.reasoning.is_empty() {
            on_event(AgentEvent::Reasoning(result.reasoning.clone()));
        }
        if result.stopped || result.interrupted.is_some() || result.tool_calls.is_empty() {
            if !result.text.is_empty() {
                on_event(AgentEvent::Delta(result.text.clone()));
            }
            return Ok(AgentReply {
                result,
                logs,
                max_steps_hit: false,
            });
        }
        if result.tool_calls.len() > MAX_CALLS {
            return Err(AppError::validation("一次最多调用 8 个工具"));
        }
        payload.turns.push(assistant_turn(&result));
        let calls = result.tool_calls.clone();
        for call in &calls {
            if turn.flag.load(Ordering::Relaxed) {
                return stopped(logs, Some(result));
            }
            let log = run_call(&mut turn, &tools, call).await;
            on_event(AgentEvent::Tool(log.clone()));
            payload.turns.push(tool_turn(&log));
            logs.push(log);
        }
        if step == max_steps {
            let mut result = result;
            let note = format!("已达到最大步数 {max_steps}，停止继续调用工具。");
            result.text = if result.text.trim().is_empty() {
                note
            } else {
                format!("{}\n\n{note}", result.text.trim())
            };
            on_event(AgentEvent::Delta(result.text.clone()));
            return Ok(AgentReply {
                result,
                logs,
                max_steps_hit: true,
            });
        }
        carried = Some(result);
    }
    Err(AppError::validation("已达到最大步数"))
}

fn stopped(logs: Vec<ToolLog>, last: Option<TextResult>) -> Result<AgentReply, AppError> {
    let Some(mut result) = last else {
        return Err(AppError::validation("已停止"));
    };
    result.stopped = true;
    if result.text.trim().is_empty() {
        result.text = "已停止".to_string();
    }
    Ok(AgentReply {
        result,
        logs,
        max_steps_hit: false,
    })
}

async fn call_model(turn: &AgentTurn<'_>, payload: &TextPayload) -> Result<TextResult, AppError> {
    let response = adapter_for(turn.provider_type)
        .execute(turn.ctx, CanonicalRequest::Text(payload.clone()))
        .await?;
    match response {
        CanonicalResponse::Text(result) => Ok(result),
        CanonicalResponse::Image(_) | CanonicalResponse::Video(_) | CanonicalResponse::Audio(_) => {
            Err(AppError::unsupported("文本入口收到了其他结果"))
        }
    }
}

fn assistant_turn(result: &TextResult) -> TextTurn {
    TextTurn {
        role: "assistant".to_string(),
        text: result.text.clone(),
        images: Vec::new(),
        tool_calls: result.tool_calls.clone(),
        tool_call_id: None,
        name: None,
    }
}

fn tool_turn(log: &ToolLog) -> TextTurn {
    TextTurn {
        role: "tool".to_string(),
        text: log.result.clone(),
        images: Vec::new(),
        tool_calls: Vec::new(),
        tool_call_id: Some(log.id.clone()),
        name: Some(log.name.clone()),
    }
}

async fn run_call(turn: &mut AgentTurn<'_>, tools: &[ToolSpec], call: &ToolCall) -> ToolLog {
    let start_time = Instant::now();
    let arguments = clip(&call.arguments, MAX_ARG_CHARS);
    let outcome = if arguments.ends_with('…') && call.arguments.chars().count() > MAX_ARG_CHARS {
        Err("参数过长".to_string())
    } else if !tools.iter().any(|tool| tool.name == call.name) {
        Err("工具未启用".to_string())
    } else {
        match args_object(&arguments) {
            Ok(args) => {
                let mcp_tool = turn.mcp.iter().find(|item| item.name == call.name);
                let mcp_stable_id = mcp_tool.map(|item| item.stable_id.as_str());

                let effective_level = match turn.pool {
                    Some(pool) => {
                        let direct =
                            crate::services::tool_permissions::get_level(pool, &call.name).await;
                        match direct {
                            Ok(crate::services::tool_permissions::ToolPermissionLevel::Confirm)
                                if mcp_stable_id.is_some() =>
                            {
                                if let Some(stable_id) = mcp_stable_id {
                                    crate::services::tool_permissions::get_level(pool, stable_id).await
                                } else {
                                    Ok(crate::services::tool_permissions::ToolPermissionLevel::Confirm)
                                }
                            }
                            res => res,
                        }
                        .map_err(|err| {
                            format!("读取工具「{}」权限失败: {err}，已禁止执行。", call.name)
                        })
                    }
                    None => Ok(crate::services::tool_permissions::default_level_for(
                        &call.name,
                    )),
                };

                let effective_level = effective_level.map(|level| {
                    if turn.workspace_root.is_none()
                        && matches!(call.name.as_str(), "read_file" | "list_directory")
                        && level == crate::services::tool_permissions::ToolPermissionLevel::Safe
                    {
                        crate::services::tool_permissions::ToolPermissionLevel::Confirm
                    } else {
                        level
                    }
                });

                match effective_level {
                    Err(err_msg) => Err(err_msg),
                    Ok(crate::services::tool_permissions::ToolPermissionLevel::Restricted) => Err(
                        format!("工具「{}」权限级别为 Restricted，当前已被禁用。", call.name),
                    ),
                    Ok(crate::services::tool_permissions::ToolPermissionLevel::Confirm) => {
                        let is_confirmed = consume_confirmation(turn, &call.name, &args);
                        if !is_confirmed {
                            let hint = if turn.workspace_root.is_none()
                                && matches!(call.name.as_str(), "read_file" | "list_directory")
                            {
                                "未选择受控工作区，文件读取与目录浏览必须逐次确认。"
                            } else {
                                "请确认本次调用，或在工具权限中将其设置为 Safe。"
                            };
                            Err(format!(
                                "⚠️ 工具「{}」需要人工确认 (Confirm)。{}",
                                call.name, hint
                            ))
                        } else {
                            execute_tool(
                                turn,
                                tool_timeout(turn.ctx.provider.timeout_ms),
                                &call.name,
                                &args,
                            )
                            .await
                        }
                    }
                    Ok(crate::services::tool_permissions::ToolPermissionLevel::Safe) => {
                        execute_tool(
                            turn,
                            tool_timeout(turn.ctx.provider.timeout_ms),
                            &call.name,
                            &args,
                        )
                        .await
                    }
                }
            }
            Err(message) => Err(message),
        }
    };
    let (ok, result) = match &outcome {
        Ok(text) => (true, clip(text, MAX_RESULT_CHARS)),
        Err(message) => (false, clip(message, MAX_RESULT_CHARS)),
    };

    let duration_ms = start_time.elapsed().as_millis();
    let sanitized_args = sanitize_audit_arguments(&arguments);
    eprintln!(
        "[AUDIT_LOG] {}",
        json!({
            "type": "agent_tool_audit",
            "tool": call.name,
            "ok": ok,
            "duration_ms": duration_ms,
            "arguments": sanitized_args
        })
    );

    ToolLog {
        id: call.id.clone(),
        name: call.name.clone(),
        arguments,
        result,
        ok,
    }
}

fn with_hint(system: Option<String>) -> Option<String> {
    Some(match system {
        Some(text) if !text.trim().is_empty() => format!("{text}\n\n{HINT}"),
        _ => HINT.to_string(),
    })
}

fn spec(name: &str) -> Option<ToolSpec> {
    let (description, parameters) = match name {
        "current_time" => (
            "返回当前 UTC 时间。没有任何参数。",
            json!({"type": "object", "properties": {}}),
        ),
        "http_request" => (
            "发送一个 HTTP 请求并返回状态码和响应正文。只接受 http 和 https。",
            json!({
                "type": "object",
                "properties": {
                    "method": {"type": "string", "enum": ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"]},
                    "url": {"type": "string"},
                    "headers": {"type": "object", "additionalProperties": {"type": "string"}},
                    "body": {"type": "string"}
                },
                "required": ["method", "url"]
            }),
        ),
        "read_file" => (
            "读取本机文本文件。",
            json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
        ),
        "write_file" => (
            "把文本写入本机文件。父目录不存在时会创建。",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"]
            }),
        ),
        "delete_file" => (
            "删除本机指定路径的文件。高风险不可逆操作。",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "要删除的文件路径"}
                },
                "required": ["path"]
            }),
        ),
        "run_command" => (
            "在操作系统命令行中执行指定命令并返回输出。支持工作目录配置。",
            json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "要执行的命令行指令"},
                    "cwd": {"type": "string", "description": "可选执行目录路径"}
                },
                "required": ["command"]
            }),
        ),
        "list_directory" => (
            "列出本机目录中的文件和子目录。",
            json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
        ),
        "memory" => (
            "保存或回忆长期记忆。action 为 'save' 时保存记忆键值；为 'recall' 时列出已有记忆。",
            json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["save", "recall"]},
                    "key": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["action"]
            }),
        ),
        _ => return None,
    };
    Some(ToolSpec {
        name: name.to_string(),
        description: description.to_string(),
        parameters,
    })
}

fn args_object(raw: &str) -> Result<Map<String, Value>, String> {
    let text = if raw.trim().is_empty() { "{}" } else { raw };
    let value: Value = serde_json::from_str(text).map_err(|_| "参数不是 JSON".to_string())?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| "参数不是 JSON 对象".to_string())
}

fn consume_confirmation(
    turn: &mut AgentTurn<'_>,
    tool_name: &str,
    arguments: &Map<String, Value>,
) -> bool {
    let Some(index) = turn.confirmed_tools.iter().position(|token| {
        let Ok(value) = serde_json::from_str::<Value>(token) else {
            return false;
        };
        let Some(object) = value.as_object() else {
            return false;
        };
        object.len() == 2
            && object.get("name").and_then(Value::as_str) == Some(tool_name)
            && object.get("arguments") == Some(&Value::Object(arguments.clone()))
    }) else {
        return false;
    };

    turn.confirmed_tools.remove(index);
    true
}

fn tool_timeout(ms: u64) -> Duration {
    Duration::from_millis(ms.clamp(1_000, 20_000))
}

async fn execute_tool(
    turn: &AgentTurn<'_>,
    timeout: Duration,
    name: &str,
    args: &Map<String, Value>,
) -> Result<String, String> {
    if let Some(tool) = turn.mcp.iter().find(|item| item.name == name) {
        return crate::services::mcp::call_tool(&turn.ctx.http, &tool.server, &tool.remote, args)
            .await;
    }
    match name {
        "current_time" => Ok(chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string()),
        "http_request" => http_request(timeout, turn.allow_trusted_local, args).await,
        "read_file" => read_file(turn.workspace_root, turn.data_dir, required(args, "path")?),
        "write_file" => write_file(
            turn.workspace_root,
            turn.data_dir,
            required(args, "path")?,
            required(args, "content")?,
        ),
        "delete_file" => delete_file(turn.workspace_root, turn.data_dir, required(args, "path")?),
        "run_command" => {
            let command_str = required(args, "command")?;
            let cwd = args.get("cwd").and_then(Value::as_str);
            run_command(turn.workspace_root, turn.data_dir, command_str, cwd).await
        }
        "list_directory" => {
            list_directory(turn.workspace_root, turn.data_dir, required(args, "path")?)
        }
        "memory" => {
            let pool = turn.pool.ok_or_else(|| "数据库连接不可用".to_string())?;
            let action = required(args, "action")?;
            if action == "save" {
                let key = required(args, "key")?;
                let content = required(args, "content")?;
                crate::services::memory::save(
                    pool,
                    crate::services::memory::AgentMemoryInput {
                        id: None,
                        conversation_id: None,
                        scope: Some("global".to_string()),
                        key: key.to_string(),
                        content: content.to_string(),
                        importance: Some(1),
                    },
                )
                .await
                .map_err(|e| e.message)?;
                Ok(format!("已成功保存记忆「{key}」"))
            } else if action == "recall" {
                let memories = crate::services::memory::list(pool, None)
                    .await
                    .map_err(|e| e.message)?;
                if memories.is_empty() {
                    Ok("当前没有已保存的长期记忆".to_string())
                } else {
                    let mut list = Vec::new();
                    for m in memories.iter().take(10) {
                        list.push(format!("- [{}]: {}", m.key, m.content));
                    }
                    Ok(list.join("\n"))
                }
            } else {
                Err("未知记忆操作，支持 save 或 recall".to_string())
            }
        }
        _ => Err("工具未启用".to_string()),
    }
}

fn required<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("缺少 {key}"))
}

fn agent_http_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("初始化 Agent HTTP 客户端失败")
    })
}

fn is_ssrf_blocked_ip(ip: IpAddr, allow_trusted_local: bool) -> Result<(), String> {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            if octets[0] == 169 && octets[1] == 254 {
                return Err(
                    "安全策略拦截：禁止访问云元数据服务或链路本地地址 (169.254.x.x)".to_string(),
                );
            }
            if v4.is_loopback() {
                if allow_trusted_local {
                    return Ok(());
                } else {
                    return Err("安全策略拦截：禁止访问本地回环地址 (127.x.x.x)".to_string());
                }
            }
            if v4.is_private() {
                return Err("安全策略拦截：禁止访问私有局域网网段 (RFC 1918)".to_string());
            }
            if octets[0] == 100 && (octets[1] & 0b1100_0000) == 64 {
                return Err("安全策略拦截：禁止访问运营商级私有网络地址 (100.64.x.x)".to_string());
            }
            if v4.is_broadcast() || v4.is_unspecified() {
                return Err("安全策略拦截：禁止访问广播或未指定地址".to_string());
            }
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() {
                if allow_trusted_local {
                    return Ok(());
                } else {
                    return Err("安全策略拦截：禁止访问本地回环地址 (::1)".to_string());
                }
            }
            if v6.is_unspecified() {
                return Err("安全策略拦截：禁止访问未指定地址 (::)".to_string());
            }
            let segments = v6.segments();
            if (segments[0] & 0xfe00) == 0xfc00 {
                return Err("安全策略拦截：禁止访问 IPv6 唯一本地网络 (fc00::/7)".to_string());
            }
            if (segments[0] & 0xffc0) == 0xfe80 {
                return Err("安全策略拦截：禁止访问 IPv6 链路本地地址 (fe80::/10)".to_string());
            }
        }
    }
    Ok(())
}

fn validate_http_target(url: &reqwest::Url, allow_trusted_local: bool) -> Result<(), String> {
    let host_str = url.host_str().ok_or_else(|| "网址缺少主机".to_string())?;
    let lower_host = host_str.to_ascii_lowercase();

    if lower_host == "metadata.google.internal"
        || lower_host == "instance-data"
        || lower_host.ends_with(".internal")
        || lower_host.ends_with(".lan")
        || lower_host.ends_with(".corp")
        || lower_host.ends_with(".local")
    {
        return Err("安全策略拦截：禁止访问云元数据服务或内网专用域名".to_string());
    }

    if lower_host == "localhost" {
        if allow_trusted_local {
            return Ok(());
        } else {
            return Err("安全策略拦截：禁止访问本地 localhost".to_string());
        }
    }

    if let Ok(ip) = lower_host.parse::<IpAddr>() {
        return is_ssrf_blocked_ip(ip, allow_trusted_local);
    }

    let port = url.port_or_known_default().unwrap_or(80);
    if let Ok(addrs) = (host_str, port).to_socket_addrs() {
        for socket_addr in addrs {
            is_ssrf_blocked_ip(socket_addr.ip(), allow_trusted_local)?;
        }
    }

    Ok(())
}

async fn http_request(
    timeout: Duration,
    allow_trusted_local: bool,
    args: &Map<String, Value>,
) -> Result<String, String> {
    let method = required(args, "method")?.to_ascii_uppercase();
    if !matches!(
        method.as_str(),
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD"
    ) {
        return Err("请求方法无效".to_string());
    }
    let url = required(args, "url")?;
    if url.chars().count() > 2_000 {
        return Err("网址过长".to_string());
    }
    let parsed = reqwest::Url::parse(url).map_err(|_| "网址无效".to_string())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("只接受 http 或 https".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("请把身份信息放在请求头里".to_string());
    }
    if parsed.host_str().is_none() {
        return Err("网址缺少主机".to_string());
    }

    validate_http_target(&parsed, allow_trusted_local)?;

    let body = args
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if !body.is_empty() && (method == "GET" || method == "HEAD") {
        return Err("GET 和 HEAD 不能带正文".to_string());
    }
    if body.chars().count() > MAX_ARG_CHARS {
        return Err("正文过长".to_string());
    }
    let headers = header_pairs(args.get("headers"))?;
    let verb = reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| "请求方法无效")?;
    let http = agent_http_client();
    let mut request = http.request(verb, parsed).timeout(timeout);
    for (name, value) in headers {
        request = request.header(name, value);
    }
    if !body.is_empty() {
        request = request.body(body);
    }
    let response = request
        .send()
        .await
        .map_err(|err| format!("请求失败: {err}"))?;
    let status = response.status().as_u16();
    let mut collected = Vec::new();
    let mut stream = response.bytes_stream();
    let mut truncated = false;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| format!("无法读取响应: {err}"))?;
        let room = MAX_RESULT_CHARS.saturating_sub(collected.len());
        if room == 0 {
            truncated = true;
            break;
        }
        if chunk.len() > room {
            collected.extend_from_slice(&chunk[..room]);
            truncated = true;
            break;
        }
        collected.extend_from_slice(&chunk);
    }
    let mut text = format!("HTTP {status}\n{}", String::from_utf8_lossy(&collected));
    if truncated {
        text.push_str("\n…已截断");
    }
    Ok(text)
}

fn header_pairs(value: Option<&Value>) -> Result<Vec<(String, String)>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    let object = value
        .as_object()
        .ok_or_else(|| "请求头必须是对象".to_string())?;
    if object.len() > 10 {
        return Err("请求头过多".to_string());
    }
    let mut headers = Vec::new();
    for (name, item) in object {
        let name = name.trim();
        if name.is_empty()
            || name.len() > 40
            || name.contains(' ')
            || name.contains(':')
            || !name.is_ascii()
        {
            return Err("请求头名称无效".to_string());
        }
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "host" | "content-length" | "transfer-encoding" | "connection"
        ) {
            continue;
        }
        let value = item
            .as_str()
            .ok_or_else(|| "请求头的值必须是字符串".to_string())?;
        if value.chars().count() > 300 {
            return Err("请求头的值过长".to_string());
        }
        headers.push((name.to_string(), value.to_string()));
    }
    Ok(headers)
}

fn read_file(workspace_root: Option<&Path>, data_dir: &Path, path: &str) -> Result<String, String> {
    let file = resolve_existing_path(workspace_root, data_dir, path)?;
    if !file.is_file() {
        return Err("这不是文件".to_string());
    }
    let bytes =
        crate::http_util::read_file_bounded_sync(&file, MAX_FILE_BYTES).map_err(
            |err| match err {
                crate::http_util::BoundedFileReadError::TooLarge => "文件超过 64KB".to_string(),
                crate::http_util::BoundedFileReadError::Io(err) => {
                    format!("无法读取文件: {err}")
                }
            },
        )?;
    if bytes.contains(&0) {
        return Err("文件不是文本".to_string());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn write_file(
    workspace_root: Option<&Path>,
    data_dir: &Path,
    path: &str,
    content: &str,
) -> Result<String, String> {
    if content.chars().count() > MAX_FILE_BYTES {
        return Err("内容超过 64KB".to_string());
    }
    let file = resolve_write_path(workspace_root, data_dir, path)?;
    if let Some(ext) = file.extension().and_then(|s| s.to_str()) {
        let ext_lower = ext.to_ascii_lowercase();
        if matches!(
            ext_lower.as_str(),
            "exe"
                | "bat"
                | "cmd"
                | "vbs"
                | "ps1"
                | "sh"
                | "bash"
                | "msi"
                | "dll"
                | "com"
                | "scr"
                | "pif"
                | "reg"
                | "sys"
        ) {
            return Err("安全策略拦截：禁止写入可执行脚本或系统扩展名文件".to_string());
        }
    }
    std::fs::write(&file, content).map_err(|err| format!("无法写入文件: {err}"))?;
    Ok("已写入".to_string())
}

fn delete_file(
    workspace_root: Option<&Path>,
    data_dir: &Path,
    raw: &str,
) -> Result<String, String> {
    let file = resolve_existing_path(workspace_root, data_dir, raw)?;
    if !file.is_file() {
        return Err("目标不是常规文件，无法删除".to_string());
    }
    std::fs::remove_file(&file).map_err(|err| format!("无法删除文件: {err}"))?;
    Ok(format!("文件已成功删除: {}", file.display()))
}

async fn run_command(
    workspace_root: Option<&Path>,
    data_dir: &Path,
    command_str: &str,
    cwd: Option<&str>,
) -> Result<String, String> {
    let cmd_trimmed = command_str.trim();
    if cmd_trimmed.is_empty() {
        return Err("命令不能为空".to_string());
    }

    let lower = cmd_trimmed.to_ascii_lowercase();
    if lower.contains("format ")
        || lower.contains("del /f /s /q c:\\")
        || lower.contains("rmdir /s /q c:\\")
        || lower.contains("rm -rf /")
        || lower.contains(":(){ :|:& };:")
        || lower.contains("certutil")
        || lower.contains("powershell -enc")
        || lower.contains("invoke-webrequest")
        || (lower.contains("curl ") && lower.contains("|"))
    {
        return Err("安全策略拦截：禁止执行潜在破坏性系统或远程脚本执行指令".to_string());
    }

    let run_dir = if let Some(dir_str) = cwd.filter(|s| !s.trim().is_empty()) {
        let dir = resolve_existing_path(workspace_root, data_dir, dir_str)?;
        if !dir.is_dir() {
            return Err("指定的执行目录不是有效目录".to_string());
        }
        dir
    } else if let Some(ws) = workspace_root {
        std::fs::canonicalize(ws).map_err(|e| format!("无法解析工作区目录: {e}"))?
    } else {
        std::env::current_dir().map_err(|e| format!("无法获取工作目录: {e}"))?
    };

    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd.exe");
        c.arg("/C").arg(cmd_trimmed);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(cmd_trimmed);
        c
    };

    cmd.current_dir(run_dir);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.kill_on_drop(true);

    let mut child = cmd.spawn().map_err(|e| format!("启动命令失败: {e}"))?;

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let stdout_fut = async move {
        if let Some(mut out) = stdout_pipe {
            use tokio::io::AsyncReadExt;
            let mut buf = Vec::new();
            let _ = (&mut out).take(1024 * 1024).read_to_end(&mut buf).await;
            buf
        } else {
            Vec::new()
        }
    };
    let stderr_fut = async move {
        if let Some(mut err) = stderr_pipe {
            use tokio::io::AsyncReadExt;
            let mut buf = Vec::new();
            let _ = (&mut err).take(1024 * 1024).read_to_end(&mut buf).await;
            buf
        } else {
            Vec::new()
        }
    };

    let wait_fut = async {
        let (out, err, status) = tokio::join!(stdout_fut, stderr_fut, child.wait());
        let status = status.map_err(|e| format!("等待进程退出失败: {e}"))?;
        Ok::<_, String>((out, err, status))
    };

    let (stdout_bytes, stderr_bytes, status) =
        match timeout(Duration::from_secs(15), wait_fut).await {
            Ok(res) => res?,
            Err(_) => {
                if let Some(pid) = child.id() {
                    if cfg!(target_os = "windows") {
                        let _ = std::process::Command::new("taskkill")
                            .args(["/F", "/T", "/PID", &pid.to_string()])
                            .output();
                    } else {
                        let _ = child.start_kill();
                    }
                } else {
                    let _ = child.start_kill();
                }
                let _ = child.wait().await;
                return Err("命令执行超时 (15 秒上限)".to_string());
            }
        };

    let stdout = String::from_utf8_lossy(&stdout_bytes);
    let stderr = String::from_utf8_lossy(&stderr_bytes);
    let code = status.code().unwrap_or(-1);

    let mut result = format!("进程退出码: {code}");
    if !stdout.is_empty() {
        result.push_str(&format!("\n[STDOUT]\n{stdout}"));
    }
    if !stderr.is_empty() {
        result.push_str(&format!("\n[STDERR]\n{stderr}"));
    }

    Ok(result)
}

fn list_directory(
    workspace_root: Option<&Path>,
    data_dir: &Path,
    path: &str,
) -> Result<String, String> {
    let dir = resolve_existing_path(workspace_root, data_dir, path)?;
    if !dir.is_dir() {
        return Err("这不是目录".to_string());
    }
    let mut lines = Vec::new();
    let mut truncated = false;
    for entry in std::fs::read_dir(&dir).map_err(|err| format!("无法列出目录: {err}"))? {
        if lines.len() == MAX_ENTRIES {
            truncated = true;
            break;
        }
        let Ok(entry) = entry else {
            continue;
        };
        let kind = if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            "dir"
        } else {
            "file"
        };
        lines.push(format!("{kind}\t{}", entry.file_name().to_string_lossy()));
    }
    lines.sort();
    let mut text = lines.join("\n");
    if truncated {
        text.push_str("\n…");
    }
    Ok(text)
}

fn normalize_path_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

fn resolve_existing_path(
    workspace_root: Option<&Path>,
    data_dir: &Path,
    raw: &str,
) -> Result<PathBuf, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("缺少路径".to_string());
    }
    let path = PathBuf::from(raw);
    let target = match workspace_root {
        Some(ws) => {
            if path.is_absolute() {
                path
            } else {
                ws.join(path)
            }
        }
        None => path,
    };
    let full = std::fs::canonicalize(&target).map_err(|err| format!("无法解析路径: {err}"))?;

    if let Some(ws) = workspace_root {
        let ws_canon =
            std::fs::canonicalize(ws).map_err(|err| format!("无法解析工作区目录: {err}"))?;
        let full_norm = normalize_path_prefix(&full);
        let ws_norm = normalize_path_prefix(&ws_canon);
        if !full_norm.starts_with(&ws_norm) {
            return Err("安全策略拦截：路径越出受控工作区范围".to_string());
        }
    }

    reject_protected_dirs(data_dir, &full)?;
    Ok(full)
}

fn resolve_write_path(
    workspace_root: Option<&Path>,
    data_dir: &Path,
    raw: &str,
) -> Result<PathBuf, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("缺少路径".to_string());
    }
    let path = PathBuf::from(raw);
    let name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "路径无效".to_string())?
        .to_os_string();

    let target = match workspace_root {
        Some(ws) => {
            if path.is_absolute() {
                path
            } else {
                ws.join(path)
            }
        }
        None => path,
    };

    reject_protected_dirs(data_dir, &existing_ancestor(&target))?;
    let parent = target.parent().filter(|item| !item.as_os_str().is_empty());
    let parent_dir = match parent {
        Some(parent) => {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|err| format!("无法创建目录: {err}"))?;
            }
            std::fs::canonicalize(parent).map_err(|err| format!("无法解析路径: {err}"))?
        }
        None => match workspace_root {
            Some(ws) => {
                std::fs::canonicalize(ws).map_err(|err| format!("无法解析工作区目录: {err}"))?
            }
            None => std::env::current_dir().map_err(|err| format!("无法解析路径: {err}"))?,
        },
    };

    let full = parent_dir.join(name);

    if let Some(ws) = workspace_root {
        let ws_canon =
            std::fs::canonicalize(ws).map_err(|err| format!("无法解析工作区目录: {err}"))?;
        let parent_norm = normalize_path_prefix(&parent_dir);
        let ws_norm = normalize_path_prefix(&ws_canon);
        if !parent_norm.starts_with(&ws_norm) {
            return Err("安全策略拦截：写入目标越出受控工作区范围".to_string());
        }
    }

    reject_protected_dirs(data_dir, &full)?;
    Ok(full)
}

fn existing_ancestor(path: &Path) -> PathBuf {
    let mut cursor = path.to_path_buf();
    if !cursor.exists() {
        while !cursor.exists() && cursor.pop() {}
    }
    std::fs::canonicalize(&cursor).unwrap_or(cursor)
}

fn reject_protected_dirs(data_dir: &Path, path: &Path) -> Result<(), String> {
    let root = std::fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());
    let path_norm = normalize_path_prefix(path);
    let root_norm = normalize_path_prefix(&root);
    if path_norm.starts_with(&root_norm) {
        return Err("不能访问应用数据目录".to_string());
    }
    let path_str = path_norm.to_string_lossy().to_ascii_lowercase();
    if path_str.contains("/.ssh")
        || path_str.contains("\\.ssh")
        || path_str.contains("/windows/")
        || path_str.contains("\\windows\\")
        || path_str.contains("start menu/programs/startup")
        || path_str.contains("start menu\\programs\\startup")
        || path_str.contains("/etc/shadow")
        || path_str.contains("/etc/passwd")
    {
        return Err("安全策略拦截：禁止访问系统保护或凭据目录".to_string());
    }
    Ok(())
}

fn sanitize_audit_arguments(args_json: &str) -> String {
    let Ok(mut val) = serde_json::from_str::<Value>(args_json) else {
        return clip(args_json, 200);
    };
    redact_sensitive_values(&mut val);
    serde_json::to_string(&val).unwrap_or_else(|_| clip(args_json, 200))
}

fn redact_sensitive_values(val: &mut Value) {
    match val {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                let lower = k.to_ascii_lowercase();
                if lower.contains("key")
                    || lower.contains("token")
                    || lower.contains("secret")
                    || lower.contains("password")
                    || lower.contains("auth")
                {
                    *v = Value::String("[REDACTED]".to_string());
                } else {
                    redact_sensitive_values(v);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                redact_sensitive_values(item);
            }
        }
        _ => {}
    }
}

fn clip(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut end = text.len();
    let limit = max_chars.saturating_sub(1);
    for (count, (index, _)) in text.char_indices().enumerate() {
        if count == limit {
            end = index;
            break;
        }
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_tools_stay_out_of_the_data_directory() {
        let root = std::env::temp_dir().join(format!("aiwb-agent-{}", uuid::Uuid::new_v4()));
        let data = root.join("data");
        let work = root.join("work");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        let note = work.join("note.txt");
        assert_eq!(
            write_file(None, &data, note.to_str().unwrap(), "hello").unwrap(),
            "已写入"
        );
        assert_eq!(
            read_file(None, &data, note.to_str().unwrap()).unwrap(),
            "hello"
        );
        assert!(list_directory(None, &data, work.to_str().unwrap())
            .unwrap()
            .contains("note.txt"));
        let blocked = data.join("secret.txt");
        assert_eq!(
            write_file(None, &data, blocked.to_str().unwrap(), "no").unwrap_err(),
            "不能访问应用数据目录"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_root_bounds_and_blocks_traversal() {
        let root = std::env::temp_dir().join(format!("aiwb-workspace-{}", uuid::Uuid::new_v4()));
        let data = root.join("data");
        let ws = root.join("workspace");
        let outside = root.join("outside");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        // 1. 在工作区内部正常读写
        let in_file = ws.join("sub").join("file.txt");
        assert_eq!(
            write_file(
                Some(&ws),
                &data,
                in_file.to_str().unwrap(),
                "content inside"
            )
            .unwrap(),
            "已写入"
        );
        assert_eq!(
            read_file(Some(&ws), &data, in_file.to_str().unwrap()).unwrap(),
            "content inside"
        );

        // 2. 尝试向工作区外部写文件 (路径遍历 ../) 必须被拦截
        let escape_write = ws.join("../outside/escaped.txt");
        let err_write =
            write_file(Some(&ws), &data, escape_write.to_str().unwrap(), "leak").unwrap_err();
        assert!(
            err_write.contains("越出受控工作区范围"),
            "Expected workspace escape error, got: {err_write}"
        );

        // 3. 尝试向工作区外部读文件 (绝对路径越界) 必须被拦截
        let outside_file = outside.join("secret_external.txt");
        std::fs::write(&outside_file, "external secret").unwrap();
        let err_read = read_file(Some(&ws), &data, outside_file.to_str().unwrap()).unwrap_err();
        assert!(
            err_read.contains("越出受控工作区范围"),
            "Expected workspace escape error, got: {err_read}"
        );

        // 4. 尝试向工作区外部删除文件必须被拦截
        let err_del = delete_file(Some(&ws), &data, outside_file.to_str().unwrap()).unwrap_err();
        assert!(
            err_del.contains("越出受控工作区范围"),
            "Expected workspace escape error, got: {err_del}"
        );

        // 5. 尝试列出外部目录必须被拦截
        let err_list = list_directory(Some(&ws), &data, outside.to_str().unwrap()).unwrap_err();
        assert!(
            err_list.contains("越出受控工作区范围"),
            "Expected workspace escape error, got: {err_list}"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ssrf_protection_blocks_cloud_metadata_and_private_networks() {
        // 1. 云元数据服务 IP 必须拦截
        let meta_ip: IpAddr = "169.254.169.254".parse().unwrap();
        assert!(is_ssrf_blocked_ip(meta_ip, false).is_err());
        assert!(is_ssrf_blocked_ip(meta_ip, true).is_err()); // 即使开启可信本地，元数据也严禁访问

        // 2. 私网 IP 拦截
        let priv_10: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(is_ssrf_blocked_ip(priv_10, false).is_err());
        let priv_192: IpAddr = "192.168.1.1".parse().unwrap();
        assert!(is_ssrf_blocked_ip(priv_192, false).is_err());
        let priv_172: IpAddr = "172.16.0.1".parse().unwrap();
        assert!(is_ssrf_blocked_ip(priv_172, false).is_err());

        // 3. 本地回环地址受控例外
        let loopback: IpAddr = "127.0.0.1".parse().unwrap();
        assert!(is_ssrf_blocked_ip(loopback, false).is_err());
        assert!(is_ssrf_blocked_ip(loopback, true).is_ok());

        // 4. 域名级黑名单拦截
        let meta_url =
            reqwest::Url::parse("http://metadata.google.internal/computeMetadata/v1").unwrap();
        assert!(validate_http_target(&meta_url, false).is_err());
        assert!(validate_http_target(&meta_url, true).is_err());

        let internal_url = reqwest::Url::parse("http://service.corp/api").unwrap();
        assert!(validate_http_target(&internal_url, false).is_err());
    }

    #[test]
    fn audit_log_redaction_sanitizes_credentials() {
        let sensitive_json = r#"{"tool": "fetch", "api_key": "sk-secret-123456", "nested": {"password": "pwd-secret-999", "token": "token-secret-888"}, "safe": "public"}"#;
        let sanitized = sanitize_audit_arguments(sensitive_json);
        assert!(!sanitized.contains("sk-secret-123456"));
        assert!(!sanitized.contains("pwd-secret-999"));
        assert!(!sanitized.contains("token-secret-888"));
        assert!(sanitized.contains("[REDACTED]"));
        assert!(sanitized.contains("public"));
    }

    #[tokio::test]
    async fn wildcard_confirmation_does_not_permit_high_risk_tools() {
        let flag = AtomicBool::new(false);
        let provider = crate::domain::ProviderRecord {
            id: "test".to_string(),
            name: "test".to_string(),
            provider_type: ProviderType::OpenaiCompatible,
            base_url: "http://127.0.0.1:9".to_string(),
            credential_id: None,
            headers: std::collections::BTreeMap::new(),
            timeout_ms: 1000,
            enabled: true,
            created_at: String::new(),
            updated_at: String::new(),
        };
        let ctx = AdapterContext::new(reqwest::Client::new(), provider, None, None, None, None);
        let dummy_dir = std::env::temp_dir();
        let turn = AgentTurn {
            ctx: &ctx,
            provider_type: ProviderType::OpenaiCompatible,
            data_dir: &dummy_dir,
            workspace_root: None,
            allow_trusted_local: false,
            flag: &flag,
            mcp: Vec::new(),
            pool: None,
            // 传入 "*" 泛通配符
            confirmed_tools: vec!["*".to_string()],
        };
        let tools = vec![
            spec("run_command").unwrap(),
            spec("read_file").unwrap(),
            spec("list_directory").unwrap(),
        ];

        // 1. 高危工具 run_command 必须被拦截，不能被 "*" 放行
        let call_cmd = ToolCall {
            id: "call_cmd".to_string(),
            name: "run_command".to_string(),
            arguments: json!({"command": "echo test"}).to_string(),
        };
        let mut turn = turn;
        let log_cmd = run_call(&mut turn, &tools, &call_cmd).await;
        assert!(!log_cmd.ok);
        assert!(log_cmd.result.contains("需要人工确认"));

        // 2. 高危工具 write_file 必须被拦截，不能被 "*" 放行
        let tools_write = vec![spec("write_file").unwrap()];
        let call_write = ToolCall {
            id: "call_write".to_string(),
            name: "write_file".to_string(),
            arguments: json!({"path": "test.txt", "content": "leak"}).to_string(),
        };
        let log_write = run_call(&mut turn, &tools_write, &call_write).await;
        assert!(!log_write.ok);
        assert!(log_write.result.contains("需要人工确认"));

        // 3. 显式确认高危工具后，不再报权限确认拦截
        let mut turn_explicit = AgentTurn {
            ctx: &ctx,
            provider_type: ProviderType::OpenaiCompatible,
            data_dir: &dummy_dir,
            workspace_root: None,
            allow_trusted_local: false,
            flag: &flag,
            mcp: Vec::new(),
            pool: None,
            confirmed_tools: vec![json!({
                "name": "run_command",
                "arguments": {"command": "echo test"}
            })
            .to_string()],
        };
        let log_cmd_explicit = run_call(&mut turn_explicit, &tools, &call_cmd).await;
        assert!(!log_cmd_explicit.result.contains("需要人工确认"));

        // 一次确认只能消费一次，重复调用即使参数完全相同也必须重新确认。
        let log_cmd_repeated = run_call(&mut turn_explicit, &tools, &call_cmd).await;
        assert!(log_cmd_repeated.result.contains("需要人工确认"));

        // 确认必须绑定完整参数，参数变化时不能复用。
        let mut turn_mismatched = AgentTurn {
            confirmed_tools: vec![json!({
                "name": "run_command",
                "arguments": {"command": "echo test"}
            })
            .to_string()],
            ..turn_explicit
        };
        let changed_call = ToolCall {
            id: "call_changed".to_string(),
            name: "run_command".to_string(),
            arguments: json!({"command": "echo changed"}).to_string(),
        };
        let log_cmd_mismatched = run_call(&mut turn_mismatched, &tools, &changed_call).await;
        assert!(log_cmd_mismatched.result.contains("需要人工确认"));

        // 无 workspace 时，默认 Safe 的文件读取必须回落到逐次 Confirm。
        let external_file =
            dummy_dir.join(format!("agent-confirmation-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&external_file, "protected").unwrap();
        let read_call = ToolCall {
            id: "call_read".to_string(),
            name: "read_file".to_string(),
            arguments: json!({"path": external_file}).to_string(),
        };
        let mut turn_without_confirmation = AgentTurn {
            confirmed_tools: Vec::new(),
            ..turn_mismatched
        };
        let log_read = run_call(&mut turn_without_confirmation, &tools, &read_call).await;
        assert!(!log_read.ok);
        assert!(log_read.result.contains("需要人工确认"));

        let list_call = ToolCall {
            id: "call_list".to_string(),
            name: "list_directory".to_string(),
            arguments: json!({"path": dummy_dir}).to_string(),
        };
        let log_list = run_call(&mut turn_without_confirmation, &tools, &list_call).await;
        assert!(!log_list.ok);
        assert!(log_list.result.contains("需要人工确认"));
        let _ = std::fs::remove_file(external_file);
    }
}
