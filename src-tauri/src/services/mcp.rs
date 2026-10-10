use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::SqlitePool;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::timeout;
use uuid::Uuid;

use crate::canonical::ToolSpec;
use crate::domain::now_iso;
use crate::error::AppError;

const MAX_TOOLS: usize = 32;
const FRAME_LIMIT: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
    pub url: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerInput {
    pub name: String,
    pub transport: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub url: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct ExposedTool {
    pub name: String,
    pub remote: String,
    pub spec: ToolSpec,
    pub server: McpServer,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<McpServer>, AppError> {
    let rows = sqlx::query(
        "SELECT id, name, transport, command, args_json, url, enabled, created_at, updated_at
         FROM mcp_servers ORDER BY name, id",
    )
    .fetch_all(pool)
    .await
    .map_err(AppError::db)?;
    rows.iter().map(server_from_row).collect()
}

pub async fn create(pool: &SqlitePool, input: McpServerInput) -> Result<McpServer, AppError> {
    let clean = clean_input(input)?;
    let id = Uuid::new_v4().to_string();
    let now = now_iso();
    sqlx::query(
        "INSERT INTO mcp_servers
         (id, name, transport, command, args_json, url, enabled, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&clean.name)
    .bind(&clean.transport)
    .bind(&clean.command)
    .bind(serde_json::to_string(&clean.args).unwrap_or_else(|_| "[]".to_string()))
    .bind(&clean.url)
    .bind(i64::from(clean.enabled))
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(AppError::db)?;
    get(pool, &id).await
}

pub async fn update(
    pool: &SqlitePool,
    id: &str,
    input: McpServerInput,
) -> Result<McpServer, AppError> {
    let _ = get(pool, id).await?;
    let clean = clean_input(input)?;
    let now = now_iso();
    sqlx::query(
        "UPDATE mcp_servers
         SET name = ?, transport = ?, command = ?, args_json = ?, url = ?, enabled = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(&clean.name)
    .bind(&clean.transport)
    .bind(&clean.command)
    .bind(serde_json::to_string(&clean.args).unwrap_or_else(|_| "[]".to_string()))
    .bind(&clean.url)
    .bind(i64::from(clean.enabled))
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(AppError::db)?;
    global_pool().remove_server(id).await;
    get(pool, id).await
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let result = sqlx::query("DELETE FROM mcp_servers WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(AppError::db)?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("找不到 MCP 服务"));
    }
    global_pool().remove_server(id).await;
    Ok(())
}

pub async fn tool_info(
    pool: &SqlitePool,
    http: &reqwest::Client,
    id: &str,
) -> Result<Vec<McpToolInfo>, AppError> {
    let server = get(pool, id).await?;
    let tools = listed_tools(http, &server)
        .await
        .map_err(AppError::validation)?;
    Ok(tools
        .into_iter()
        .map(|tool| McpToolInfo {
            name: tool.name,
            description: tool.description,
        })
        .collect())
}

pub async fn tools_for_agent(
    pool: &SqlitePool,
    http: &reqwest::Client,
) -> Result<Vec<ExposedTool>, AppError> {
    let servers = list(pool).await?;
    let enabled: Vec<_> = servers
        .into_iter()
        .filter(|server| server.enabled)
        .collect();
    if enabled.is_empty() {
        return Err(AppError::validation("没有启用的 MCP 服务"));
    }
    let mut exposed = Vec::new();
    for server in enabled {
        let tools = listed_tools(http, &server)
            .await
            .map_err(AppError::validation)?;
        for tool in tools {
            let index = exposed.len();
            if index >= MAX_TOOLS {
                return Err(AppError::validation("启用的 MCP 工具超过 32 个"));
            }
            let name = format!("mcp_{index}_{}", safe_name(&tool.name));
            exposed.push(ExposedTool {
                spec: ToolSpec {
                    name: name.clone(),
                    description: format!("{}：{}", server.name, tool.description),
                    parameters: tool.schema,
                },
                name,
                remote: tool.name,
                server: server.clone(),
            });
        }
    }
    if exposed.is_empty() {
        return Err(AppError::validation("启用的 MCP 服务没有工具"));
    }
    Ok(exposed)
}

pub async fn call_tool(
    http: &reqwest::Client,
    server: &McpServer,
    remote: &str,
    arguments: &Map<String, Value>,
) -> Result<String, String> {
    let response = rpc(
        http,
        server,
        "tools/call",
        json!({ "name": remote, "arguments": arguments }),
    )
    .await?;
    tool_text(&response)
}

struct ListedTool {
    name: String,
    description: String,
    schema: Value,
}

async fn listed_tools(
    http: &reqwest::Client,
    server: &McpServer,
) -> Result<Vec<ListedTool>, String> {
    let response = rpc(http, server, "tools/list", json!({})).await?;
    let tools = response
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut listed = Vec::new();
    for tool in tools {
        let Some(name) = tool.get("name").and_then(Value::as_str) else {
            continue;
        };
        if name.trim().is_empty() {
            continue;
        }
        listed.push(ListedTool {
            name: name.to_string(),
            description: tool
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("MCP 工具")
                .to_string(),
            schema: tool
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({"type": "object", "properties": {}})),
        });
    }
    Ok(listed)
}

async fn rpc(
    http: &reqwest::Client,
    server: &McpServer,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    global_pool().rpc(http, server, method, params).await
}

async fn http_post(
    http: &reqwest::Client,
    server: &McpServer,
    session: Option<&str>,
    method: &str,
    params: Value,
) -> Result<(Option<String>, Value), String> {
    let notification = method.starts_with("notifications/");
    let body = if notification {
        json!({ "jsonrpc": "2.0", "method": method, "params": params })
    } else {
        json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })
    };
    let mut request = http
        .post(&server.url)
        .timeout(Duration::from_secs(20))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .json(&body);
    if let Some(session) = session {
        request = request.header("mcp-session-id", session);
    }
    let response = request
        .send()
        .await
        .map_err(|err| format!("MCP 请求失败: {err}"))?;
    let next_session = response
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .or_else(|| session.map(str::to_string));
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    if notification {
        return Ok((next_session, Value::Null));
    }
    const MAX_MCP_BYTES: usize = 10 * 1024 * 1024;
    if let Some(content_length) = response.content_length() {
        if content_length > MAX_MCP_BYTES as u64 {
            return Err("MCP 响应过大 (超过 10MB)".to_string());
        }
    }
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut body_bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| format!("MCP 响应读取失败: {err}"))?;
        if body_bytes.len() + chunk.len() > MAX_MCP_BYTES {
            return Err("MCP 响应过大 (超过 10MB)".to_string());
        }
        body_bytes.extend_from_slice(&chunk);
    }
    let text = String::from_utf8(body_bytes).map_err(|_| "MCP 响应非 UTF-8 编码".to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("MCP 返回 {status}"));
    }
    let value = if content_type.contains("text/event-stream") {
        sse_json(&text)?
    } else {
        serde_json::from_str(&text).map_err(|_| "MCP 响应不是 JSON".to_string())?
    };
    if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
        return Err(message.to_string());
    }
    Ok((next_session, value))
}

fn sse_json(text: &str) -> Result<Value, String> {
    for line in text.lines() {
        let Some(data) = line.trim().strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() {
            continue;
        }
        return serde_json::from_str(data).map_err(|_| "MCP 事件不是 JSON".to_string());
    }
    Err("MCP 事件没有数据".to_string())
}

struct ActiveStdioProcess {
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
    next_id: i64,
    buffer: Vec<u8>,
}

impl ActiveStdioProcess {
    async fn spawn_and_init(server: &McpServer) -> Result<Self, String> {
        let mut cmd = Command::new(&server.command);
        cmd.args(&server.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);

        #[cfg(windows)]
        {
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        let mut child = cmd.spawn().map_err(|err| format!("无法启动 MCP: {err}"))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "MCP 没有输入".to_string())?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| "MCP 没有输出".to_string())?;
        let mut buffer = Vec::new();

        let init_fut = async {
            write_frame(
                &mut stdin,
                &rpc_body("initialize", initialize_params(), Some(1)),
            )
            .await?;
            let _ = read_until_id(&mut stdout, &mut buffer, 1).await?;
            write_frame(
                &mut stdin,
                &rpc_body("notifications/initialized", json!({}), None),
            )
            .await?;
            Ok::<(), String>(())
        };

        timeout(Duration::from_secs(15), init_fut)
            .await
            .map_err(|_| "MCP 初始化握手超时 (15s)".to_string())??;

        Ok(Self {
            child,
            stdin,
            stdout,
            next_id: 1,
            buffer,
        })
    }

    async fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        write_frame(&mut self.stdin, &rpc_body(method, params, Some(id))).await?;
        read_until_id(&mut self.stdout, &mut self.buffer, id).await
    }

    async fn kill(&mut self) {
        let _ = self.child.kill().await;
    }
}

pub struct McpProcessPool {
    stdio_processes: Mutex<HashMap<String, Arc<Mutex<ActiveStdioProcess>>>>,
    http_sessions: Mutex<HashMap<String, String>>,
}

impl Default for McpProcessPool {
    fn default() -> Self {
        Self::new()
    }
}

fn is_session_expired(err: &str) -> bool {
    let lower = err.to_lowercase();
    lower.contains("404")
        || (lower.contains("session")
            && (lower.contains("not found")
                || lower.contains("invalid")
                || lower.contains("expired")
                || lower.contains("unknown")
                || lower.contains("terminated")))
}

impl McpProcessPool {
    pub fn new() -> Self {
        Self {
            stdio_processes: Mutex::new(HashMap::new()),
            http_sessions: Mutex::new(HashMap::new()),
        }
    }

    pub async fn remove_server(&self, server_id: &str) {
        let proc = {
            let mut map = self.stdio_processes.lock().await;
            map.remove(server_id)
        };
        if let Some(proc_lock) = proc {
            let mut proc = proc_lock.lock().await;
            proc.kill().await;
        }

        let mut http_map = self.http_sessions.lock().await;
        http_map.remove(server_id);
    }

    #[allow(dead_code)]
    pub async fn shutdown(&self) {
        let mut map = self.stdio_processes.lock().await;
        for (_, proc_lock) in map.drain() {
            let mut proc = proc_lock.lock().await;
            proc.kill().await;
        }
        let mut http_map = self.http_sessions.lock().await;
        http_map.clear();
    }

    pub async fn rpc(
        &self,
        http: &reqwest::Client,
        server: &McpServer,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        match server.transport.as_str() {
            "http" => self.http_call(http, server, method, params).await,
            "stdio" => self.stdio_call(server, method, params).await,
            _ => Err("MCP 传输方式无效".to_string()),
        }
    }

    async fn http_call(
        &self,
        http: &reqwest::Client,
        server: &McpServer,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        let cached_session = {
            let map = self.http_sessions.lock().await;
            map.get(&server.id).cloned()
        };

        if let Some(session) = cached_session {
            match http_post(http, server, Some(&session), method, params.clone()).await {
                Ok((next_s, val)) => {
                    if let Some(s) = next_s {
                        let mut map = self.http_sessions.lock().await;
                        map.insert(server.id.clone(), s);
                    }
                    return Ok(val);
                }
                Err(err) => {
                    if is_session_expired(&err) {
                        let mut map = self.http_sessions.lock().await;
                        map.remove(&server.id);
                    } else {
                        return Err(err);
                    }
                }
            }
        }

        let (session, _) = http_post(http, server, None, "initialize", initialize_params()).await?;
        let _ = http_post(
            http,
            server,
            session.as_deref(),
            "notifications/initialized",
            json!({}),
        )
        .await;

        if let Some(s) = &session {
            let mut map = self.http_sessions.lock().await;
            map.insert(server.id.clone(), s.clone());
        }

        let (next_session, val) =
            http_post(http, server, session.as_deref(), method, params).await?;
        if let Some(s) = next_session {
            let mut map = self.http_sessions.lock().await;
            map.insert(server.id.clone(), s);
        }
        Ok(val)
    }

    async fn stdio_call(
        &self,
        server: &McpServer,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        let proc_arc = self.get_or_spawn_stdio(server).await?;
        let mut proc = proc_arc.lock().await;

        let res = timeout(Duration::from_secs(30), proc.call(method, params)).await;

        match res {
            Ok(Ok(val)) => Ok(val),
            Ok(Err(err)) => {
                drop(proc);
                self.remove_server(&server.id).await;
                Err(err)
            }
            Err(_) => {
                drop(proc);
                self.remove_server(&server.id).await;
                Err("MCP 超时 (30s)".to_string())
            }
        }
    }

    async fn get_or_spawn_stdio(
        &self,
        server: &McpServer,
    ) -> Result<Arc<Mutex<ActiveStdioProcess>>, String> {
        {
            let map = self.stdio_processes.lock().await;
            if let Some(existing) = map.get(&server.id) {
                return Ok(Arc::clone(existing));
            }
        }

        let new_proc = ActiveStdioProcess::spawn_and_init(server).await?;
        let arc = Arc::new(Mutex::new(new_proc));

        let mut map = self.stdio_processes.lock().await;
        if let Some(existing) = map.get(&server.id) {
            return Ok(Arc::clone(existing));
        }
        map.insert(server.id.clone(), Arc::clone(&arc));
        Ok(arc)
    }
}

static GLOBAL_MCP_POOL: OnceLock<McpProcessPool> = OnceLock::new();

pub fn global_pool() -> &'static McpProcessPool {
    GLOBAL_MCP_POOL.get_or_init(McpProcessPool::new)
}

fn initialize_params() -> Value {
    json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": { "name": "ai-workbench", "version": "0.1.0" }
    })
}

fn rpc_body(method: &str, params: Value, id: Option<i64>) -> String {
    let body = match id {
        Some(req_id) => {
            json!({ "jsonrpc": "2.0", "id": req_id, "method": method, "params": params })
        }
        None => json!({ "jsonrpc": "2.0", "method": method, "params": params }),
    };
    body.to_string()
}

async fn write_frame(stdin: &mut tokio::process::ChildStdin, body: &str) -> Result<(), String> {
    stdin
        .write_all(&encode_frame(body))
        .await
        .map_err(|err| format!("无法写入 MCP: {err}"))?;
    stdin
        .flush()
        .await
        .map_err(|err| format!("无法写入 MCP: {err}"))
}

async fn read_until_id(
    stdout: &mut tokio::process::ChildStdout,
    buffer: &mut Vec<u8>,
    id: i64,
) -> Result<Value, String> {
    while let Some(value) = take_frame(buffer) {
        if value.get("id").and_then(Value::as_i64) == Some(id) {
            if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
                return Err(message.to_string());
            }
            return Ok(value);
        }
    }

    let mut chunk = [0_u8; 4096];
    loop {
        let read = stdout
            .read(&mut chunk)
            .await
            .map_err(|err| format!("无法读取 MCP: {err}"))?;
        if read == 0 {
            return Err("MCP 已退出".to_string());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > FRAME_LIMIT {
            return Err("MCP 响应过大".to_string());
        }
        while let Some(value) = take_frame(buffer) {
            if value.get("id").and_then(Value::as_i64) == Some(id) {
                if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
                    return Err(message.to_string());
                }
                return Ok(value);
            }
        }
    }
}

fn tool_text(value: &Value) -> Result<String, String> {
    if value.pointer("/result/isError").and_then(Value::as_bool) == Some(true) {
        return Err(flatten_content(value).unwrap_or_else(|| "MCP 工具失败".to_string()));
    }
    flatten_content(value).ok_or_else(|| "MCP 工具没有文本结果".to_string())
}

fn flatten_content(value: &Value) -> Option<String> {
    let content = value.pointer("/result/content")?.as_array()?;
    let text = content
        .iter()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn safe_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
        if out.len() >= 32 {
            break;
        }
    }
    if out.is_empty() {
        out.push_str("tool");
    }
    out
}

pub fn encode_frame(body: &str) -> Vec<u8> {
    format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
}

pub fn take_frame(buffer: &mut Vec<u8>) -> Option<Value> {
    let header_end = buffer.windows(4).position(|item| item == b"\r\n\r\n")?;
    let header = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let length = content_length(&header)?;
    let start = header_end + 4;
    if buffer.len() < start.saturating_add(length) {
        return None;
    }
    let body = buffer[start..start + length].to_vec();
    buffer.drain(..start + length);
    serde_json::from_slice(&body).ok()
}

fn content_length(header: &str) -> Option<usize> {
    for line in header.split("\r\n") {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse().ok();
        }
    }
    None
}

struct CleanServer {
    name: String,
    transport: String,
    command: String,
    args: Vec<String>,
    url: String,
    enabled: bool,
}

fn clean_input(input: McpServerInput) -> Result<CleanServer, AppError> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::validation("MCP 名称需要 1 到 80 个字符"));
    }
    let transport = input.transport.trim().to_ascii_lowercase();
    if transport != "stdio" && transport != "http" {
        return Err(AppError::validation("MCP 传输只能是 stdio 或 http"));
    }
    let command = input.command.trim().to_string();
    let url = input.url.trim().to_string();
    if transport == "stdio" {
        if command.is_empty() || command.chars().count() > 260 || command.contains(['\n', '\r']) {
            return Err(AppError::validation("请填写可执行文件"));
        }
    } else if let Err(message) = clean_url(&url) {
        return Err(AppError::validation(message));
    }
    if input.args.len() > 16 {
        return Err(AppError::validation("MCP 参数过多"));
    }
    let mut args = Vec::new();
    for arg in input.args {
        let arg = arg.trim();
        if arg.is_empty() {
            continue;
        }
        if arg.chars().count() > 200 || arg.contains(['\n', '\r']) {
            return Err(AppError::validation("MCP 参数无效"));
        }
        args.push(arg.to_string());
    }
    Ok(CleanServer {
        name: name.to_string(),
        command: if transport == "stdio" {
            command
        } else {
            String::new()
        },
        args: if transport == "stdio" {
            args
        } else {
            Vec::new()
        },
        url: if transport == "http" {
            url
        } else {
            String::new()
        },
        transport,
        enabled: input.enabled,
    })
}

fn clean_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "MCP 地址不合法".to_string())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("MCP 地址只支持 http 或 https".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("不要把密钥写进 MCP 地址".to_string());
    }
    Ok(())
}

async fn get(pool: &SqlitePool, id: &str) -> Result<McpServer, AppError> {
    let row = sqlx::query(
        "SELECT id, name, transport, command, args_json, url, enabled, created_at, updated_at
         FROM mcp_servers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::db)?
    .ok_or_else(|| AppError::not_found("找不到 MCP 服务"))?;
    server_from_row(&row)
}

fn server_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<McpServer, AppError> {
    use sqlx::Row;
    let args = serde_json::from_str(
        row.try_get::<String, _>("args_json")
            .map_err(AppError::db)?
            .as_str(),
    )
    .unwrap_or_else(|_| Vec::new());
    Ok(McpServer {
        id: row.try_get("id").map_err(AppError::db)?,
        name: row.try_get("name").map_err(AppError::db)?,
        transport: row.try_get("transport").map_err(AppError::db)?,
        command: row.try_get("command").map_err(AppError::db)?,
        args,
        url: row.try_get("url").map_err(AppError::db)?,
        enabled: row.try_get::<i64, _>("enabled").map_err(AppError::db)? != 0,
        created_at: row.try_get("created_at").map_err(AppError::db)?,
        updated_at: row.try_get("updated_at").map_err(AppError::db)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip_ignores_partial_bytes() {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#;
        let encoded = encode_frame(body);
        let mut buffer = encoded[..encoded.len() - 1].to_vec();
        assert!(take_frame(&mut buffer).is_none());
        buffer.push(encoded[encoded.len() - 1]);
        let value = take_frame(&mut buffer).unwrap();
        assert_eq!(value["result"]["ok"], json!(true));
        assert!(buffer.is_empty());
    }

    #[tokio::test]
    async fn mcp_process_pool_lifecycle() {
        let pool = McpProcessPool::new();
        let server_id = "test-server-1";
        pool.remove_server(server_id).await;
        pool.shutdown().await;
    }
}
