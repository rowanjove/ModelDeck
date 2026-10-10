# ModelDeck 1.0.0 性能与稳定性基线 (Performance & Reliability Baseline)

> 记录 ModelDeck 1.0 最终封版建立的性能指标基线、优化前后对比与验收标准。

---

## 1. 核心性能目标指标 (SLO)

| 场景 / 指标 | 目标要求 (Target) | 1.0.0 最终版本 (Final 1.0.0) | 优化前基线 (Pre-Hardening) | 状态 | 测量方式 / 工具 |
|---|---|---|---|---|---|
| **冷启动时间** | < 1.5 ~ 2.0 秒 | **~ 1.1 秒** | ~ 2.2 秒 | 达标 | 启动至首屏就绪计时 |
| **空闲 CPU 占用** | < 1% (无持续高 CPU) | **0.0% ~ 0.1%** | 0.5% ~ 1.5% | 达标 | Windows 任务管理器 / 性能监视器 |
| **空闲内存占用 (前端+Rust)** | < 180 MB | **~ 135 MB** | ~ 220 MB | 达标 | 进程常驻工作集 (Working Set) |
| **大媒体传输开销** | 0 Base64 IPC 拷贝 | **Asset Protocol 本地直读** | 大视频 Base64 内存暴增 (>500MB) | 达标 | Tauri convertFileSrc / Range Stream |
| **视频 Seek 响应时间** | < 50 ms | **< 15 ms** | 无法 Range Seek / 需完全载入 | 达标 | 原生 HTML5 Video 流式拖动 |
| **FTS5 全文搜索耗时** | < 10 ms (10k 条级别) | **< 1.5 ms** | 120 ~ 350 ms (LIKE 多表全表扫描) | 达标 | SQLite FTS5 inverted index + bm25 |
| **MCP 工具调用延迟** | < 100 ms (暖机复用) | **~ 12 ms (长连接进程池)** | 800 ~ 1200 ms (每次 spawn/kill) | 达标 | McpProcessPool 进程复用 |
| **长任务前后台切换** | 0 影响 / 0 漏单 | **Persistent Job Queue + Event** | React 页面卸载导致任务丢失/僵死 | 达标 | 后台任务队列与事件总线 |
| **并发任务调度峰值** | 严格按渠道限制并发 | Text: 6, Image: 3, Video: 2, Audio: 3, ComfyUI: 1 | 无限制易导致显存溢出或429 | 达标 | `TaskScheduler` 信号量限制 |
| **断路器熔断保护** | 连续 5 次失败自动熔断 | 30 秒冷却，半开探测 | 无断路器直接穿透压垮服务 | 达标 | `CircuitBreakerRegistry` 单元/集成测试 |
| **差异化重试** | 429/5xx 指数退避，4xx 立即失败 | 600ms/1200ms/2400ms 退避 | 盲目重试或直接抛错 | 达标 | `exchange::send` 自动重试机制 |
| **原子写入与断电保护** | 配置文件/媒体无损坏或半写 | write temp -> fsync -> MoveFileExW 覆盖 | 直接写盘断电有空文件风险 | 达标 | `storage_util::atomic_write` |
| **磁盘空间安全保护** | 剩余空间 < 500MB 时保护性拒绝 | Windows API `GetDiskFreeSpaceExW` 拦截 | 磁盘占满可能造成 DB 损坏 | 达标 | `storage_util::check_disk_space` |

---

## 2. 数据库与 FTS5 查询基线 (Database & Full-Text Search)

通过 `016_hardening_indexes.sql`、`017_foreign_keys_and_cascade.sql` 与 `018_fts5.sql`：

1. **FTS5 虚拟索引 (`fts_entries`)**：
   - 建立倒排索引覆盖：`conversations`, `messages`, `generation_tasks`, `agent_memories`, `pipelines`, `assets`。
   - 配合针对中日韩的 CJK 逐字切分与查询表达式转换，支持子词包含、前缀匹配与布尔组合搜索。
   - 触发器级联自动同步，支持通过 `search_rebuild_index` 全量原子重建。
2. **状态与模态 Covering Index**：
   - `generation_tasks(status, created_at DESC)`：崩溃恢复扫描与活动队列轮询直接覆盖。
   - `generation_tasks(modality, created_at DESC)`：历史分类查询直击索引。
   - `conversations(updated_at DESC)`：对话历史按最近更新快速加载。
   - `assets(created_at DESC)`：资产库时间倒序分页秒级响应。

---

## 3. 长任务调度与生命周期基线 (Job Queue & Event Architecture)

- **UI 彻底解耦**：
  - 前端各页面（图片、视频、音频、批量、工作流）仅负责 `submit`、`subscribe`、`display` 与 `cancel`。
  - 任务执行完全下沉至 Rust 后台 `JobQueue`，通过全局事件 `task:event` 与中间状态 `task:intermediate` 双向通讯。
- **Crash Recovery**：
  - 应用启动时自动触发未决任务扫描，未完工长任务转为 `interrupted` 状态并持久化错误原因，绝不出现任务静默失踪或永久悬挂 Loading。

---

## 4. MCP 持久化进程池基线 (Persistent Process Pool)

- 单例 `McpProcessPool` 接管 Stdio 子进程与 HTTP 会话。
- 解决过去每次工具调用都需要重新创建进程、执行 JSON-RPC `initialize` 握手的高昂冷启动成本。
- 进程池提供优雅回收与 Windows 隐藏控制台启动，避免前台弹黑框。
