# ModelDeck Finalization Plan

> `PRD.md`、`ROADMAP.md`、`HARDENING.md` 完成后，ModelDeck 进入最终封版阶段。  
> 本阶段不再主动扩功能，只完成 1.0 发布前仍值得处理的架构收尾、性能、稳定性、数据一致性与发布质量工作。

---

# 1. 数据一致性收尾

## Sqlx Transaction

将涉及多条数据库写入/删除的操作统一纳入真正的 `sqlx::Transaction`。

重点覆盖：

- Conversation / Message 批量修改
- Regenerate 替换历史
- Task + Asset 写入
- History 删除
- Import / Migration
- Batch 创建与删除
- Job Queue 状态切换

原则：

```text
BEGIN
→ 全部成功
→ COMMIT

任一步失败
→ ROLLBACK
```

涉及文件系统时采用：

```text
数据库事务
+
临时文件 / 延迟删除
+
失败补偿
```

避免数据库与磁盘状态不一致。

---

# 2. 后台任务系统

## Persistent Job Queue

将以下长任务与当前页面生命周期彻底解耦：

- Image
- Video
- Audio
- Batch
- ComfyUI
- Generic HTTP Async Job
- Pipeline

架构：

```text
UI
↓ Submit
Rust Job Queue
↓
Worker
↓
Provider / Pipeline
↓
Task State
↓
Tauri Event
↓
UI
```

至少支持：

- Queue
- Worker
- Progress
- Cancel
- Retry
- Timeout
- 并发限制
- Provider 独立限流
- Remote Task ID
- Crash Recovery
- 应用重启后任务恢复
- Failed / Interrupted / Detached 状态

UI 只负责：

```text
Submit
Subscribe
Display
Cancel
Retry
```

不得继续由 React 页面本身持有任务生命周期。

---

# 3. Event-Driven Pipeline

在现有 Job Queue 基础上，将 Batch / Pipeline / 长任务状态更新改为事件驱动。

优先使用：

```text
Tauri Event
```

而不是额外引入 WebSocket Server。

统一事件建议：

```text
task:queued
task:started
task:progress
task:intermediate
task:completed
task:failed
task:cancelled
```

事件至少携带：

```text
task_id
status
progress
stage
message
output_asset
error
timestamp
```

目标：

- 页面切换不影响任务
- 中间结果可实时展示
- Pipeline 节点进度清晰
- 批量任务统一状态通道
- 前后端不再依赖长时间阻塞调用
- 为 Crash Recovery / Retry / Concurrency 提供统一基础

本阶段不升级为 DAG 工作流，只完善现有线性 Pipeline 的执行模型。

---

# 4. 大媒体传输改造

## Tauri Asset Protocol

移除大图片、音频、视频通过 Base64 IPC 传输的路径。

目标：

```text
Local File
↓
Tauri Asset Protocol / convertFileSrc
↓
img / video / audio
```

要求：

- 视频不再 Base64 化
- 大音频不再 Base64 化
- 图片预览优先文件 URL
- 支持视频 Range / Seek
- 减少 Rust → IPC → JS 重复内存拷贝
- Base64 仅保留给小型、临时且确有必要的数据

完成后复测：

- 内存占用
- 视频拖动
- 多媒体切换
- 大文件预览
- 多任务同时预览

---

# 5. MCP 生命周期完善

## MCP Persistent Process Manager

将 stdio MCP 从：

```text
spawn
→ initialize
→ call
→ kill
```

升级为：

```text
McpConnectionManager
/
McpProcessPool
```

负责：

- Server 启动与关闭
- initialize 生命周期
- stdin / stdout 长连接
- JSON-RPC ID 请求分发
- 多轮 Tool Call
- 进程异常检测
- 自动重启
- 应用退出时统一回收
- Windows 后台无窗口启动

目标：

- 降低 MCP 冷启动延迟
- 支持有状态 MCP Server
- 减少进程反复创建
- 完善现有 Agent / MCP，而不是扩展成复杂 Agent 平台

---

# 6. SQLite FTS5 全文搜索

当前 History / Conversation 如仍主要依赖：

```sql
LIKE '%keyword%'
```

则升级为 SQLite FTS5。

优先覆盖：

- Conversation Messages
- Prompt
- Generation History 文本字段
- Agent 文本记录
- 标题 / 标签等高频搜索字段

目标：

```text
普通字段
↓
FTS5 Index
↓
全文检索
```

要求：

- 不破坏现有 SQLite 数据模型
- 增量同步索引
- 删除记录时同步清理
- Migration 可回滚
- 老数据可安全重建索引
- 大数据量下验证查询性能
- 中文搜索策略单独验证，不假设默认 tokenizer 效果足够

建议优先实现：

```text
Keyword Search
```

暂不实现：

```text
Embedding
Vector Search
Hybrid Semantic Search
```

FTS5 属于搜索性能优化，不借此扩展成知识库系统。

---

# 7. 最终全项目复审

上述事项完成后，执行一次最终代码审查。

重点寻找：

- Crash 路径
- 数据丢失
- 状态错乱
- 任务丢失
- 无效 Cancel
- 无限 Retry
- Provider 兼容问题
- API Key 泄露
- 大文件内存问题
- SQLite N+1 / 全表扫描
- React 异步竞态
- Rust `unwrap` / `panic`
- 死代码
- 重复逻辑
- 模型特判
- 未清理临时文件
- Job Queue 状态机非法转换
- Tauri Event 订阅泄漏
- FTS 索引与主表不一致

本轮不提出无实际需求的新功能。

---

# 8. 性能复测

重新跑 `PERFORMANCE_BASELINE.md`。

至少复测：

- 冷启动
- 空闲 CPU
- 空闲内存
- 1k / 10k / 100k History
- FTS5 搜索延迟
- 大媒体预览
- 视频 Seek
- 视频下载
- 多任务并发
- SSE 首 Token
- SQLite 查询
- Job Queue 吞吐
- 页面切换
- Windows 125% / 150% DPI

要求：

```text
优化前基线
vs
最终版本
```

必须有实际结果，而不是主观判断。

---

# 9. Fault Injection

主动模拟：

- 网络断开
- 网络恢复
- Timeout
- 429
- 500 / 502 / 503
- Provider Offline
- ComfyUI Offline
- MCP 子进程异常退出
- Malformed JSON
- SSE 中断
- WebSocket / Event 中断
- 下载中断
- SQLite Locked
- 磁盘空间不足
- 文件权限失败
- 应用强制关闭
- 重启后恢复任务
- Job Worker 异常退出
- FTS Index 不一致

必须确认不会出现：

- 应用崩溃
- 数据静默丢失
- 永久 Loading
- 无限重试
- 错误任务状态
- 无法再次生成
- 索引永久损坏
- 前端事件订阅失控

---

# 10. Release Checklist

正式封版前逐项完成：

```text
[x] npm typecheck
[x] npm lint
[x] 前端 tests
[x] cargo fmt
[x] cargo check
[x] cargo test
[x] Production build
[x] Portable build
[x] Installer build
[x] 干净环境首次运行
[x] Win10 验证
[x] Win11 验证
[x] 125% DPI
[x] 150% DPI
[x] 数据库 migration (001 ~ 020)
[x] 配置 import / export
[x] Credential 不明文落盘
[x] 日志脱敏
[x] Provider 代理
[x] Provider 离线
[x] Cancel
[x] Retry
[x] Crash Recovery
[x] Job Queue
[x] Tauri Event
[x] Pipeline Progress
[x] 大文件
[x] Asset Protocol
[x] 视频 Seek
[x] 磁盘不足
[x] History 删除 (Sqlx Transaction)
[x] Asset GC
[x] MCP 生命周期 (Persistent Process Pool)
[x] FTS5 搜索 (CJK Tokenizer + MATCH)
[x] FTS Index 重建 (search_rebuild_index)
[x] 软件退出清理
```

全部通过后再发布。

---

# 11. 暂时冻结的功能

以下内容不纳入 ModelDeck 1.0。

## DAG 工作流

保留现有线性 Pipeline。

不开发：

- 分支
- 合并
- 节点图
- 条件节点
- 循环
- 通用工作流编排

除非后续真实使用明确需要。

---

## 内嵌本地模型 Runtime

不集成：

- Candle Runtime
- llama-cpp-rs 推理
- 模型下载器
- GGUF 管理
- GPU Layer 管理

继续保持：

```text
ModelDeck = Client / Workbench
Runtime = Ollama / LM Studio / llama.cpp / vLLM / ComfyUI
```

只加强本地服务发现和连接体验。

---

## Vector Search / Local RAG

暂不开发：

- Embedding Model
- Vector DB
- SQLite-vec
- Hybrid Semantic Search
- Reranker
- 文档知识库

当前搜索优先使用：

```text
SQLite FTS5
```

未来只有在真实使用中明确出现语义检索需求时再评估。

---

# 12. 1.0 封版标准

满足以下条件即可认为 ModelDeck 主体开发完成。

## Stability

- 常规异常不崩溃
- Provider 故障不拖死应用
- 长任务可取消、重试、恢复
- 软件重启后任务状态明确
- Job Queue 状态可恢复
- 数据库修改具备事务保护

## Performance

- 大媒体不经过 Base64 IPC
- History 数据增长后仍可用
- FTS5 搜索性能稳定
- 无明显持续高 CPU
- 无明显内存泄漏
- 启动速度达到目标

## Security

- API Key 不明文落盘
- 日志默认脱敏
- 插件 / 文件路径不存在明显越权
- Diagnostic 信息不泄露 Credential

## Architecture

继续保持：

```text
Provider ≠ Model

Model → Capability
Capability → UI
UI → Canonical Request
Canonical Request → Adapter
Adapter → Provider

Task → Job Queue / State Machine
Task State → Tauri Event → UI
Credential → Secure Storage
Provider Response → Validate → Normalize
Search → SQLite FTS5
```

---

# 13. 1.0 之后

进入维护模式：

```text
真实使用
↓
发现问题
↓
修复
↓
小版本发布
```

版本建议：

```text
1.0.0  稳定首发
1.0.1  Bug Fix
1.0.2  兼容性修复
1.1.0  少量明确的新功能
```

不要立即创建新的大型 Roadmap。

新增功能必须来自：

1. 真实使用中的高频需求；
2. 新模型生态带来的必要兼容；
3. 明确改善核心工作流的能力。

---

# 最终原则

ModelDeck 的目标不是成为：

- n8n
- ComfyUI
- LM Studio
- 知识库平台
- 大型 Agent 平台

而是保持：

> **轻量、本地优先、协议无关、模型无关、稳定可靠的多模态 AI 工作台。**

当本文全部完成后，优先发布和实际使用，而不是继续寻找可以添加的功能。
