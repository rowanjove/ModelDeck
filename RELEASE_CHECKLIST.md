# ModelDeck 1.0.0 正式发布前自检清单 (Release Checklist)

> 用于 ModelDeck 1.0.0 封版交付与正式发布的最终工程自检清单。
>
> **提示**：ModelDeck 持续稳定与演进版本的通用可复现发布规范清单请参阅 [docs/dev/RELEASE_CHECKLIST.md](docs/dev/RELEASE_CHECKLIST.md)。

---

## 1. 代码质量与类型安全 (Code Quality & Types)

- [x] `npm run typecheck`：TypeScript 0 错误通过
- [x] `npm run lint`：Biome / Linter 0 错误通过 (78 个文件)
- [x] `npm run format`：代码格式化一致
- [x] `cargo check`：Rust 0 编译错误与警告
- [x] `cargo clippy`：0 warnings，无高危未捕获 panic / unwrap
- [x] `cargo fmt -- --check`：Rust 代码格式规范 100% 一致

---

## 2. 自动化测试套件 (Test Suites)

- [x] `npm run test`：前端 Vitest 单元测试全部绿灯通过 (22/22)
- [x] `cargo test`：Rust 单元、集成与输入边界回归测试全部通过 (118/118)
- [x] 断路器与重试策略回归测试通过 (`resilience::tests`, `fault_injection_circuit_breaker`, `fault_injection_rate_limit`)
- [x] 原子写入与断电保护测试通过 (`storage_util::tests`)
- [x] 任务状态机转移矩阵合法性测试通过 (`task_scheduler::tests`)
- [x] Fuzz 极限边界测试通过（100KB+ 超长字段防御拦截、Unicode/Emoji/特殊字符、畸变 JSON）
- [x] MCP 进程池生命周期测试通过 (`mcp_process_pool_lifecycle`, `mcp_http_tool_is_called_from_the_agent`)
- [x] SQLite FTS5 与 CJK 词切分及重建测试通过 (`test_cjk_segment_and_fts5`, `test_rebuild_index_and_cascade_triggers`)

---

## 3. 数据完整性与持久化 (Data Integrity & Sqlx Transaction)

- [x] SQLite Schema 迁移正常执行，无语法或依赖断裂 (`001` ~ `020_task_output_and_tombstones.sql`)
- [x] `PRAGMA integrity_check` 返回通过
- [x] 对话、消息、任务删除与回滚纳入原子 `sqlx::Transaction`
- [x] 数据库事务配合磁盘物理文件的延迟删除与失败补偿保护
- [x] 导出配置完全脱敏，不含 API Key / Bearer Token
- [x] 配置文件 / 媒体文件落盘使用 `storage_util::atomic_write`，避免半写或清空文件
- [x] 磁盘空间低于 500MB 时安全拦截新下载，避免磁盘溢出
- [x] Schema Versioning 版本声明与校验已落地

---

## 4. 后台任务系统与事件解耦 (Persistent Job Queue & Tauri Events)

- [x] Image / Video / Audio / Batch / Pipeline 任务完全与 React 页面生命周期解耦
- [x] 后台队列持久维护 `TaskState`，通过 `tauri::Emitter` 向全局广播 `task:event` 与中间状态 `task:intermediate`
- [x] 前端切换 Tab、关闭页面或后台挂起不影响生成进度
- [x] 异常关闭或强制退出后重启，未完成任务自动标记为 `interrupted`，绝不静默丢失
- [x] 取消机制从 UI 贯穿至底层网络请求，能够立即中断连接与轮询
- [x] 并发控制器 (`TaskScheduler` / `job_queue`) 有效限制并发通道（Text: 6, Image: 3, Video: 2, Audio: 3, ComfyUI: 1）
- [x] 本地 ComfyUI 具有单并发队列保护，防止 GPU 显存溢出

---

## 5. 大媒体与资产流式加载 (Tauri Asset Protocol)

- [x] 启用 `tauri = { version = "2", features = ["protocol-asset"] }` 与安全作用域配置
- [x] 前端音频、视频、图片预览全面使用 `convertFileSrc` 本地协议流式播放
- [x] 彻底移除视频与大音频的 Base64 IPC 传输，消除内存二次暴增
- [x] 本地视频播放器支持原生 Seek / Range 请求

---

## 6. MCP 进程池与长连接复用 (Persistent MCP Process Manager)

- [x] Stdio MCP 进程由单例 `McpProcessPool` 持久化管理，避免频繁 spawn / kill
- [x] 启动即握手 `initialize` 与 `notifications/initialized`，跨轮次调用长连接复用
- [x] HTTP MCP 会话通过 `mcp-session-id` 进行持久会话复用
- [x] MCP 配置更新或删除时，进程池精准淘汰并安全回收子进程

---

## 7. 全文检索性能优化 (SQLite FTS5 & CJK Tokenizer)

- [x] 引入 `fts_entries` FTS5 虚拟表，涵盖对话、消息、任务、记忆、资产、工作流
- [x] 针对中文/CJK 场景引入分词与 MATCH 查询转换，支持子词包含、前缀匹配与布尔组合
- [x] 数据库触发器自动保证增删改实时同步与级联清理
- [x] 提供 `search_rebuild_index` 安全全量重建索引命令

---

## 8. 安全合规审查 (Security Audit)

- [x] 系统日志与 Raw Request/Response 默认对 Authorization / API Key 进行脱敏（修复 mask_embedded 嵌入式密钥全面过滤）
- [x] 诊断中心导出的 `diagnostic.json` 完全排除用户 Prompt 与密钥信息
- [x] 本地文件读取路径穿越保护有效（UUID 格式校验与规范化物理路径校验）
- [x] 凭据存储启用 Windows 原生安全存储后端（`keyring` 的 `windows-native`）
- [x] 工具权限查询遇错阻断不放行，拒绝静默降级
- [x] 不硬编码任何真实凭据

---

## 9. 最终加固与防退化闭环 (Hardening & Data Protection Audit)

- [x] **路径逃逸防御**：统一校验 UUID，写入前规范化路径必须严格处于指定应用目录内
- [x] **消息原子替换**：聊天重生成在生成成功后再于单事务内原子替换，失败或取消保留完整旧消息
- [x] **资产引用计数**：删除历史物理文件前跨表检查 assets/voices/messages/tasks 引用，杜绝误删共享素材
- [x] **存储清理时间保护**：设置 30 分钟写入保护窗口，结合二次引用校验，彻底消除 GC 竞态误删
- [x] **持久入队与 CAS 状态机**：入队前事务保存任务 payload，全流程状态更新基于 CAS 终态不可逆保证
- [x] **取消贯穿与隔离**：取消 flag 贯穿 Batch、Pipeline 与流式等待，ComfyUI 精确匹配 prompt_id 剔除，绝不误杀他人任务
- [x] **网络代理热更新**：先测试连接再落库，同时热更新 State 与 JobQueue 的 HTTP Client
- [x] **大响应流式限额**：Provider 文本/JSON 响应最多 16 MiB，调试预览最多保留 5 MiB；图像/视频/音频按类型限制为 15/80/40 MiB，MCP 响应正文限制 10 MiB
- [x] **MCP 握手锁外与幂等重试**：初始化脱离全局锁并施加 15s 超时，会话失效外禁止盲目重放非幂等工具
- [x] **流式截断精确检测**：检查协议结束标志，过早 EOF 明确标记为 interrupted，杜绝截断假成
- [x] **前端会话隔离与草稿持久化**：流式状态按会话隔离，切换会话清空草稿；分镜故事板草稿本地持久化自动恢复
- [x] **配置导入防逃逸**：统一复用校验器，修改地址时断开旧凭据绑定，避免密钥错发
- [x] **FTS 索引统一**：增量写入与全量重建索引统一 CJK 分词格式，彻底消除中文短语检索不一致
- [x] **大数据性能约束**：`list_messages` 去除未使用的 raw payload、统计加载 LIMIT 5000、素材库支持分页

---

## 10. 生产构建与发布产物 (Production Release & Packaging)

- [x] `npm run build` 前端构建产物生成无误（代码分块优化，主包体积减少 50%）
- [x] 生产版本号统一升级至 `1.0.0`（`package.json`, `tauri.conf.json`, `Cargo.toml`）
- [x] 118 个 Rust 单元/集成测试 100% 通过（118/118 pass）
- [x] 22 个前端 Vitest 测试 100% 通过（22/22 pass）
- [x] `cargo clippy --all-targets -- -D warnings` 零警告、零错误
- [x] `npx tsc --noEmit` 与 `npx biome check src` 零类型错误、零 lint 错误

---

## 11. 未验证边界声明 (Explicit Unverified Boundaries)

以下场景由于当前处于纯离线开发与模拟测试环境，尚未在真实物理生产环境中进行端到端全量测量：
1. **真实硬件系统凭据持久化跨设备导出**：已启用 Windows 原生凭据后端并通过 Mock/集成测试，但在多用户权限隔离企业域环境下的真实 Windows Credential Manager 跨用户持久化有待实际打包机验证。
2. **极端断电/进程强杀恢复**：已通过模拟崩溃测试验证了 `interrupted` 状态转换，但未针对正在进行物理写盘瞬间的硬件掉电进行物理破坏性断电测试。
3. **真实外部 Provider/ComfyUI 极端网络抖动**：已通过 Mock 服务验证了中断隔离与重试策略，真实远端高延时或半连接中断依赖网络环境实际回归。
