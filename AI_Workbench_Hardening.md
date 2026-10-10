# AI Workbench Hardening Plan

> 用于 `PRD.md` 与 `ROADMAP.md` 完成后的性能、稳定性、健壮性、安全性与发布质量提升。  
> 核心原则：**功能基本冻结，优先消除崩溃、数据损坏、任务丢失、状态错乱、资源泄漏、性能退化和 Provider 兼容失败。**

---

## 1. 阶段定位

当 `PRD.md` 与 `ROADMAP.md` 的核心功能完成后，项目进入：

```text
Feature Complete
↓
Bug Bash
↓
Stability Hardening
↓
Performance Optimization
↓
Reliability Engineering
↓
Security Audit
↓
Release Engineering
↓
UX Polish
```

此阶段原则上不主动堆新功能。

只有以下类型的新能力值得优先考虑：

- 配置备份 / 恢复
- 诊断中心
- 崩溃恢复
- 配置 / Schema 迁移
- 脱敏诊断包导出

---

# 2. 总体目标

重点消除以下风险：

- Provider 兼容性互相干扰
- 异步任务丢失或状态错误
- ComfyUI / 视频任务状态不一致
- SQLite 数据量增长后明显变慢
- Provider 异常拖死整个应用
- API 返回格式变化导致崩溃
- 大图片 / 视频导致内存暴涨
- 重试、取消、恢复逻辑存在边界 Bug
- React 与 Rust 状态不同步
- 配置格式升级后旧数据无法兼容
- 日志泄露 API Key 或用户敏感数据
- 软件异常退出后无法恢复任务

---

# 3. 性能基线

性能优化前必须先测量，不允许凭感觉重构。

建立：

```text
PERFORMANCE_BASELINE.md
```

至少记录：

- 冷启动时间
- 空闲内存占用
- 空闲 CPU 占用
- SQLite 查询耗时
- History 10 / 1k / 10k / 100k 条性能
- Asset 1k / 10k 条性能
- Provider 请求延迟
- SSE 首 Token 延迟
- 大文件上传内存
- 大视频下载内存
- 多任务并发性能
- 安装包 / Portable 体积

建议目标：

```text
冷启动 < 1.5 ~ 2 秒
普通页面交互 < 100 ms
空闲状态无持续高 CPU
历史数据增加后无明显线性卡顿
```

---

# 4. 启动性能

启动时只加载必要数据：

- Settings
- Provider 基础 metadata
- 最近少量会话
- 必要数据库状态

禁止启动时：

- 加载全部 History
- 加载全部 Asset
- 扫描全部 Provider
- 自动探测全部本地服务
- 初始化所有页面资源

其他内容全部 Lazy Load。

---

# 5. SQLite 优化

## 5.1 索引

重点检查类似索引：

```sql
messages(conversation_id, created_at)
generation_tasks(status, created_at)
assets(task_id)
models(provider_id)
conversations(updated_at)
```

根据真实查询计划补充索引，不机械添加。

## 5.2 分页

禁止：

```sql
SELECT * FROM messages;
```

History、Messages、Tasks、Assets 均必须分页。

优先逐步使用 Cursor Pagination，避免大 OFFSET。

## 5.3 避免 N+1

禁止：

```text
查询 100 个任务
→ 每个任务再查询一次 Asset
```

应使用：

```sql
JOIN
```

或批量：

```sql
WHERE task_id IN (...)
```

## 5.4 SQLite 配置

评估：

```text
WAL
foreign_keys = ON
busy_timeout
migration transaction
periodic checkpoint
```

诊断中心提供：

```sql
PRAGMA integrity_check;
```

---

# 6. 大文件与内存

## 6.1 禁止整文件进入前端内存

尤其视频文件禁止：

```text
read file
→ Vec<u8>
→ Base64
→ React
```

优先：

```text
disk
→ Rust stream
→ HTTP body
```

## 6.2 尽量避免 Base64

Base64 会增加约三分之一体积。

视频、大音频、大图优先：

- multipart/form-data
- file stream
- local path
- binary response

## 6.3 缩略图

History / Asset Library 不直接加载原文件。

图片生成：

```text
256px WebP thumbnail
```

视频生成：

```text
preview frame
```

列表只加载缩略图，打开详情时才加载原资源。

---

# 7. 并发控制

建立统一：

```text
TaskScheduler
```

避免用户连续 Generate 造成无限并发。

示例：

```text
Text: 6
Image: 3
Video: 2
Download: 3
```

Provider 可设置独立限制：

```text
OpenAI: 4
ComfyUI: 1
```

目标：

- 防止 429
- 防止 GPU 显存爆炸
- 防止网络拥塞
- 防止内存暴涨
- 防止大量任务同时轮询

---

# 8. 统一任务状态机

任务状态必须严格定义。

推荐：

```text
queued
↓
submitting
↓
running
↓
downloading
↓
completed
```

异常：

```text
failed
cancelled
detached
interrupted
```

必须定义合法状态转换。

例如：

```text
completed → running
```

必须禁止。

所有状态变化应经过统一 Task State Machine，不允许各页面自行改 status。

---

# 9. Crash Recovery

任务关键状态必须及时持久化：

- provider
- model
- remote task id
- status
- request metadata
- output path
- timestamps

启动后查询：

```sql
SELECT *
FROM generation_tasks
WHERE status IN (
  'submitting',
  'running',
  'downloading'
);
```

Provider 支持远端查询：

```text
恢复查询远端状态
```

Provider 不支持：

```text
标记 interrupted
```

不能静默丢失任务。

---

# 10. Retry 策略

不得简单：

```text
失败 → retry 3 次
```

## 可重试

- Timeout
- Connection Reset
- 429
- 502
- 503
- 504
- 临时 DNS / Network Error

使用：

```text
Exponential Backoff + Jitter
```

例如：

```text
1s
2s
4s
8s
```

## 不应自动重试

- 400
- 401
- 403
- Invalid Parameter
- Unsupported Model
- Unsupported Capability

---

# 11. Circuit Breaker

Provider 连续异常时建立断路器。

例如：

```text
连续失败 5 次
↓
OPEN
↓
暂时停止请求
↓
30 秒后 HALF OPEN
↓
发送探测请求
↓
成功 → CLOSED
失败 → OPEN
```

避免单个 Provider 故障拖累整个应用。

---

# 12. Timeout 分层

禁止单一：

```text
timeout = 60s
```

应至少拆分：

```text
connect_timeout
request_timeout
idle_timeout
download_timeout
task_poll_timeout
```

文本请求与视频生成必须采用不同策略。

---

# 13. Cancel 机制

Cancel 必须贯穿：

```text
React
↓
Tauri Command
↓
Rust Task
↓
HTTP / WebSocket / Polling
```

不能只在 UI 上隐藏任务。

Rust 侧建议统一使用 Cancellation Token。

Provider 支持远端取消：

```text
调用 Provider Cancel API
```

不支持：

```text
停止本地轮询
→ 标记 detached / cancelled
```

---

# 14. Rust 稳定性

重点审查：

```rust
unwrap()
expect()
panic!()
```

以下路径原则上不得依赖 panic：

- 网络
- Provider 返回
- SQLite
- 文件系统
- 用户输入
- Model Registry
- Generic HTTP
- Asset 处理

统一使用结构化错误传播。

---

# 15. 错误体系

建立统一错误层级：

```text
ProviderError
├── Auth
├── Network
├── Timeout
├── RateLimit
├── InvalidRequest
├── UnsupportedCapability
├── RemoteTaskFailed
└── ParseError
```

同时包括：

```text
DatabaseError
StorageError
ConfigError
SchemaError
TaskError
```

用户界面显示可理解错误。

Developer Mode 显示：

- Cause Chain
- Raw Provider Response
- Request ID
- HTTP Status

---

# 16. Provider 返回数据验证

永远不要假设远端响应合法。

可能出现：

```json
{
  "data": null
}
```

甚至：

```html
502 Bad Gateway
```

Adapter 必须统一：

```text
validate
↓
parse
↓
normalize
↓
Canonical Response
```

不得直接访问未经验证的深层字段。

---

# 17. Schema Versioning

以下数据必须拥有：

```json
{
  "schemaVersion": 1
}
```

适用：

- Model Definition
- Registry
- Preset
- Agent
- Workflow
- Export Config

提供显式迁移：

```text
v1 → v2 → v3
```

禁止修改 Schema 后让旧配置直接失效。

---

# 18. Atomic Write

配置、Registry Cache、Workflow、Preset 等文件禁止直接：

```text
open
truncate
write
```

使用：

```text
write temp
↓
flush / fsync
↓
atomic rename
```

避免崩溃导致文件清空或半写状态。

---

# 19. 磁盘空间保护

生成 / 下载大文件前检查剩余空间。

设置最低安全阈值，例如：

```text
500MB / 1GB
```

磁盘不足时：

- 停止新下载
- 给出明确提示
- 不允许写到一半才失败

---

# 20. Asset Garbage Collection

增加：

```text
Storage Maintenance
```

检查：

- Orphan Files
- Missing Files
- Temporary Files
- Failed Download Fragments
- 无数据库引用文件
- 数据库引用但实际不存在文件

支持安全清理。

---

# 21. React 稳定性

## Error Boundary

至少为主要模块建立局部 Error Boundary：

- Text
- Image
- Video
- Audio
- Agent
- History

单个组件异常不能导致整个应用白屏。

## Async Race

重点处理：

```text
打开任务 A
↓
请求详情
↓
切换任务 B
↓
A 请求返回
↓
覆盖 B 状态
```

使用：

- AbortController
- Query Key
- Request Identity
- Cancellation

---

# 22. React 性能

禁止无依据地大量添加：

```text
useMemo
useCallback
memo
```

只有 Profiler 证明存在明显重复渲染时再优化。

重点优先：

- 数据量
- 大文件
- 图片解码
- 列表虚拟化
- SQLite 查询
- 跨 Tauri IPC 数据量

---

# 23. 结构化日志

建议字段：

```text
timestamp
level
module
request_id
task_id
provider_id
model_id
event
```

示例：

```json
{
  "level": "error",
  "task_id": "...",
  "provider_id": "openrouter",
  "event": "request_failed"
}
```

日志必须默认脱敏。

---

# 24. Correlation ID

每次请求生成：

```text
request_id
```

贯穿：

```text
UI
→ Canonical Request
→ Adapter
→ HTTP
→ Task
→ Download
→ Asset
```

所有相关日志都携带同一 Request ID。

---

# 25. 测试体系

建立四层测试。

## Unit Tests

覆盖：

- Schema
- Constraint
- Parser
- Mapping
- Sanitization
- Template

## Integration Tests

覆盖：

- SQLite
- Adapter
- Task Manager
- File Storage
- Migration
- Recovery

## Contract Tests

针对 Provider：

- Request Mapping
- Response Mapping
- Error Mapping
- Capability Mapping

尤其加强 OpenAI Compatible。

## E2E

只覆盖核心路径：

```text
Add Provider
↓
Add Model
↓
Generate
↓
History
```

以及：

```text
启动
↓
恢复任务
↓
查看结果
```

无需追求数量，重点覆盖关键链路。

---

# 26. Fault Injection / Chaos

主动模拟：

- 网络断开
- Timeout
- 429
- 500 / 502 / 503
- Malformed JSON
- SSE 中断
- WebSocket 关闭
- 下载中断
- Provider Offline
- ComfyUI Offline
- SQLite Locked
- 磁盘满
- 文件权限错误
- 应用强制退出
- 重启后任务恢复

检查是否出现：

- Crash
- 数据丢失
- 卡死
- 状态错误
- 无限重试
- UI 永久 Loading

---

# 27. Fuzz Testing

重点对象：

- Generic HTTP Template
- JSON Parser
- Model Definition
- Registry
- Constraint
- Response Parser

测试：

- 异常 JSON
- 超长字段
- 缺失字段
- 错误类型
- 极端数组大小
- Unicode
- 中文路径
- 特殊字符

---

# 28. 安全审查

重点检查：

- API Key 明文
- Authorization 日志
- URL Token
- Generic HTTP Header
- 导出文件
- Crash Report
- Clipboard
- Proxy
- 本地文件读取
- Agent Tool 权限
- 路径穿越
- 任意文件覆盖

配置导出默认排除 Credential。

诊断包必须脱敏。

---

# 29. 依赖治理

定期检查：

```text
cargo audit
npm audit
```

但禁止机械升级全部依赖。

必须判断：

- 是否影响实际生产路径
- 是否仅 Dev Dependency
- 是否可实际利用
- 是否存在 Breaking Change

同时检查：

- 重复 npm 包
- 不必要 Rust Feature
- 大体积依赖
- 已废弃依赖

---

# 30. Release Build

生产版本重点检查：

- Debug Logging
- Source Map
- Panic Handling
- Bundle Size
- Symbols
- Credential
- Dev Endpoint
- Test Feature Flag

Rust Release Profile 可评估：

```toml
[profile.release]
lto = true
codegen-units = 1
strip = true
```

必须基于实际体积和启动性能决定，不机械开启。

---

# 31. 诊断中心

新增轻量 Diagnostics：

检查：

- App Version
- OS
- SQLite
- Schema Version
- Data Directory
- Registry
- Credential Store
- Provider Connectivity
- Local Services
- Disk Space
- Task Queue

支持：

```text
Run Diagnostics
```

---

# 32. Diagnostic Bundle

允许用户导出：

```text
diagnostic.zip
```

包含：

- App Version
- OS
- Schema Version
- Provider 类型
- 脱敏日志
- Error Stack
- Task Metadata
- Diagnostic Result

默认不包含：

- API Key
- Authorization
- 用户 Prompt
- 私人媒体内容
- 完整对话
- Voice Reference

除非用户明确选择加入。

---

# 33. Backup / Restore

支持备份：

- Provider 配置
- Model Definition
- Preset
- Agent
- Settings
- Registry Override

默认不备份 API Key。

允许可选：

```text
Encrypted Credential Backup
```

但不是首要目标。

---

# 34. Migration 系统

所有数据库和配置迁移必须：

- 有版本号
- 可测试
- 使用事务
- 失败可回滚
- 不静默丢数据

升级前可创建轻量 Backup。

---

# 35. Bug Bash

正式 Hardening 前进行一次系统 Bug Bash。

覆盖：

```text
Provider
Model
Text
Chat
Agent
Image
Video
Audio
ComfyUI
Generic HTTP
History
Task
Asset
Settings
Import / Export
```

重点不是 UI 小瑕疵，而是：

- Crash
- 数据损坏
- 功能失效
- 状态错误
- 无法恢复
- 安全问题

---

# 36. Hardening 优先级

Roadmap 完成后严格按以下顺序推进：

```text
P0 Bug Bash
↓
P0 Data Integrity
↓
P0 Task Recovery
↓
P0 Provider Resilience
↓
P1 Performance Baseline
↓
P1 SQLite Optimization
↓
P1 Asset / Memory Optimization
↓
P1 Startup Optimization
↓
P1 Error / Logging
↓
P1 Test Expansion
↓
P2 Fault Injection
↓
P2 Security Audit
↓
P2 Release Engineering
↓
P3 UX Polish
↓
再考虑新功能
```

---

# 37. Release Checklist

建议根目录额外建立：

```text
RELEASE_CHECKLIST.md
```

至少包括：

```text
[ ] TypeScript typecheck
[ ] ESLint
[ ] Rust fmt
[ ] Cargo check
[ ] Unit tests
[ ] Integration tests
[ ] E2E critical path
[ ] Production build
[ ] Portable build
[ ] Fresh install test
[ ] Upgrade migration test
[ ] Crash recovery test
[ ] Provider connectivity test
[ ] Credential leak scan
[ ] Log sanitization check
[ ] Large file test
[ ] Disk full test
[ ] Offline test
```

---

# 38. 长期架构底线

无论 Hardening 如何修改实现，都必须保持：

```text
Provider ≠ Model

Model → Capability

Capability → UI

UI → Canonical Request

Canonical Request → Adapter

Adapter → Provider
```

并保持：

```text
Task → State Machine
Asset → Local Storage
Credential → Secure Storage
Provider Response → Validate → Normalize
```

如果修复问题开始要求在大量页面中添加：

```text
if model === xxx
```

应优先判断架构是否已经退化。

---

# 39. 不允许的“优化”

禁止以下 AI 常见行为：

- 为了性能无依据重写大模块
- 大量添加 useMemo / useCallback
- 为未来需求过度抽象
- 因一个 Provider 特例破坏统一 Adapter
- 为减少几 MB 引入高复杂度
- 把错误吞掉来“提高稳定性”
- 用无限 Retry 掩盖 Provider 故障
- 用 Mock 让测试通过
- 删除失败测试而不是修问题
- 把数据库数据一次全部缓存到内存
- 为减少查询制造复杂的全局状态同步系统

所有优化必须有：

```text
问题
→ 测量
→ 修改
→ 对比
→ 验证
```

---

# 40. Hardening 阶段完成标准

项目达到以下状态后，可认为核心 Hardening 完成：

## Stability

- 常规异常不会导致应用崩溃
- Provider 故障不会拖死整个应用
- 异步任务重启后可恢复或明确标记
- Cancel / Retry / Timeout 行为一致
- 数据库迁移可靠

## Performance

- 冷启动达到目标
- 大 History 无明显卡顿
- 大媒体不会进入 JS 内存
- Asset 列表使用缩略图
- 无明显持续 CPU 占用
- 并发受到控制

## Data

- 无已知数据丢失路径
- Atomic Write
- Schema Versioning
- Backup / Migration 可验证
- Asset 与数据库可维护

## Security

- API Key 不明文落盘
- 日志默认脱敏
- Diagnostic Bundle 不泄密
- Generic HTTP / Agent Tool 有明确边界

## Testing

- 核心逻辑有 Unit Test
- 关键模块有 Integration Test
- Provider 有 Contract Test
- 核心路径有 E2E
- Fault Injection 不导致严重状态损坏

## Release

- Production Build 稳定
- Portable / Installer 可验证
- Fresh Install 正常
- Upgrade 正常
- Release Checklist 全部通过

---

# 41. 后续开发原则

Hardening 完成后，不再主动寻找“还能塞什么功能”。

后续需求来源优先级：

```text
真实使用问题
>
稳定性问题
>
高频操作摩擦
>
模型生态变化
>
明确用户需求
>
新功能想象
```

每个新增功能必须回答：

1. 是否解决真实问题？
2. 是否可以由现有 Capability / Adapter / Registry 实现？
3. 是否会破坏轻量定位？
4. 是否值得增加长期维护成本？
5. 是否应该由外部服务完成，而不是内置？

---

# 42. 最终目标

AI Workbench 在功能完成后，不应继续演化成“大而全 AI 平台”。

长期目标保持：

> **轻量、本地优先、协议无关、模型无关、稳定、可恢复、易扩展的多模态 AI 工作台。**

真正的成熟标准不是功能数量，而是：

```text
启动快
↓
调用稳
↓
异常可控
↓
任务不丢
↓
数据不坏
↓
新模型容易接
↓
长期使用不会越来越卡
```
