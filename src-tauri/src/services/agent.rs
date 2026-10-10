use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

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
    turn: AgentTurn<'_>,
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
            let log = run_call(&turn, &tools, call).await;
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

async fn run_call(turn: &AgentTurn<'_>, tools: &[ToolSpec], call: &ToolCall) -> ToolLog {
    let arguments = clip(&call.arguments, MAX_ARG_CHARS);
    let outcome = if arguments.ends_with('…') && call.arguments.chars().count() > MAX_ARG_CHARS {
        Err("参数过长".to_string())
    } else if !tools.iter().any(|tool| tool.name == call.name) {
        Err("工具未启用".to_string())
    } else {
        match args_object(&arguments) {
            Ok(args) => {
                let effective_level = match turn.pool {
                    Some(pool) => crate::services::tool_permissions::get_level(pool, &call.name)
                        .await
                        .map_err(|err| {
                            format!("读取工具「{}」权限失败: {err}，已禁止执行。", call.name)
                        }),
                    None => Ok(crate::services::tool_permissions::default_level_for(
                        &call.name,
                    )),
                };

                match effective_level {
                    Err(err_msg) => Err(err_msg),
                    Ok(crate::services::tool_permissions::ToolPermissionLevel::Restricted) => Err(
                        format!("工具「{}」权限级别为 Restricted，当前已被禁用。", call.name),
                    ),
                    Ok(crate::services::tool_permissions::ToolPermissionLevel::Confirm) => {
                        let is_confirmed = turn
                            .confirmed_tools
                            .iter()
                            .any(|t| t == &call.name || t == "*");
                        if !is_confirmed {
                            Err(format!(
                                "⚠️ 工具「{}」属于高风险操作，需要人工确认 (Confirm)。请在前端确认允许执行本次调用，或在工具权限中将其设置为 Safe。",
                                call.name
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
    let (ok, result) = match outcome {
        Ok(text) => (true, clip(&text, MAX_RESULT_CHARS)),
        Err(message) => (false, clip(&message, MAX_RESULT_CHARS)),
    };
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
        "http_request" => http_request(&turn.ctx.http, timeout, args).await,
        "read_file" => read_file(turn.data_dir, required(args, "path")?),
        "write_file" => write_file(
            turn.data_dir,
            required(args, "path")?,
            required(args, "content")?,
        ),
        "delete_file" => delete_file(turn.data_dir, required(args, "path")?),
        "run_command" => {
            let command_str = required(args, "command")?;
            let cwd = args.get("cwd").and_then(Value::as_str);
            run_command(turn.data_dir, command_str, cwd).await
        }
        "list_directory" => list_directory(turn.data_dir, required(args, "path")?),
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

async fn http_request(
    http: &Client,
    timeout: Duration,
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

fn read_file(data_dir: &Path, path: &str) -> Result<String, String> {
    let file = resolve_existing(data_dir, path)?;
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

fn write_file(data_dir: &Path, path: &str, content: &str) -> Result<String, String> {
    if content.chars().count() > MAX_FILE_BYTES {
        return Err("内容超过 64KB".to_string());
    }
    let file = resolve_write(data_dir, path)?;
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

fn delete_file(data_dir: &Path, raw: &str) -> Result<String, String> {
    let file = resolve_existing(data_dir, raw)?;
    if !file.is_file() {
        return Err("目标不是常规文件，无法删除".to_string());
    }
    std::fs::remove_file(&file).map_err(|err| format!("无法删除文件: {err}"))?;
    Ok(format!("文件已成功删除: {}", file.display()))
}

async fn run_command(
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
        resolve_existing(data_dir, dir_str)?
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

fn list_directory(data_dir: &Path, path: &str) -> Result<String, String> {
    let dir = resolve_existing(data_dir, path)?;
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

fn resolve_existing(data_dir: &Path, raw: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(raw.trim());
    if path.as_os_str().is_empty() {
        return Err("缺少路径".to_string());
    }
    let full = std::fs::canonicalize(&path).map_err(|err| format!("无法解析路径: {err}"))?;
    reject_data_dir(data_dir, &full)?;
    Ok(full)
}

fn resolve_write(data_dir: &Path, raw: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(raw.trim());
    let name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "路径无效".to_string())?;
    reject_data_dir(data_dir, &existing_ancestor(&path))?;
    let parent = path.parent().filter(|item| !item.as_os_str().is_empty());
    let parent = match parent {
        Some(parent) => {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|err| format!("无法创建目录: {err}"))?;
            }
            std::fs::canonicalize(parent).map_err(|err| format!("无法解析路径: {err}"))?
        }
        None => std::env::current_dir().map_err(|err| format!("无法解析路径: {err}"))?,
    };
    let full = parent.join(name);
    reject_data_dir(data_dir, &full)?;
    Ok(full)
}

fn existing_ancestor(path: &Path) -> PathBuf {
    let mut cursor = path.to_path_buf();
    if !cursor.exists() {
        while !cursor.exists() && cursor.pop() {}
    }
    std::fs::canonicalize(&cursor).unwrap_or(cursor)
}

fn reject_data_dir(data_dir: &Path, path: &Path) -> Result<(), String> {
    let root = std::fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());
    if path.starts_with(&root) {
        return Err("不能访问应用数据目录".to_string());
    }
    let path_str = path.to_string_lossy().to_ascii_lowercase();
    if path_str.contains("/.ssh")
        || path_str.contains("\\.ssh")
        || path_str.contains("/windows/")
        || path_str.contains("\\windows\\")
        || path_str.contains("start menu/programs/startup")
        || path_str.contains("start menu\\programs\\startup")
    {
        return Err("安全策略拦截：禁止访问系统保护或凭据目录".to_string());
    }
    Ok(())
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
            write_file(&data, note.to_str().unwrap(), "hello").unwrap(),
            "已写入"
        );
        assert_eq!(read_file(&data, note.to_str().unwrap()).unwrap(), "hello");
        assert!(list_directory(&data, work.to_str().unwrap())
            .unwrap()
            .contains("note.txt"));
        let blocked = data.join("secret.txt");
        assert_eq!(
            write_file(&data, blocked.to_str().unwrap(), "no").unwrap_err(),
            "不能访问应用数据目录"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
