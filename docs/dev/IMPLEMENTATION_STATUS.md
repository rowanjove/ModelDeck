# ModelDeck 实施状态与缺陷跟踪矩阵 (Implementation Status & Defect Matrix)

- **基准版本**：v1.0.0 (`fbd20736efe66516ada3cc7cc42c8c7c22e021fa`)
- **分支**：`fix/v1.0.1-stabilization`
- **最新更新日期**：2026-10-10
- **基线报告**：详见 [`docs/dev/BASELINE_20261010.md`](file:///f:/AI/vibecoding/ai%E8%B0%83%E7%94%A8/docs/dev/BASELINE_20261010.md)

---

## 1. 架构映射表 (功能 → 前端组件 → IPC 命令 → Rust 服务 → 适配器 → 测试)

| 功能模块 | 前端核心组件 | IPC 命令 | Rust 后端服务 | 适配器 (Adapters) | 覆盖测试 |
|---|---|---|---|---|---|
| **智能对话 (Chat)** | `src/features/text/TextView.tsx` | `chat_send`<br>`chat_cancel`<br>`conversations_list`<br>`conversations_get`<br>`conversations_update`<br>`conversations_delete`<br>`messages_delete`<br>`messages_toggle_pin`<br>`conversations_compact` | `services/chat.rs`<br>`services/context.rs`<br>`services/memory.rs` | `adapters/openai.rs`<br>`adapters/anthropic.rs`<br>`adapters/gemini.rs`<br>`adapters/ollama.rs`<br>`adapters/generic_http.rs` | `integration_tests::streaming_chat_keeps_context_bounded_and_truncates_on_regenerate`<br>`services::chat::tests::*`<br>`services::context::tests::*`<br>`tokens.test.ts` |
| **Agent 与工具 (Agent & MCP)** | `src/features/text/TextView.tsx`<br>`src/components/Shell.tsx` | `agent_memories_list`<br>`agent_memories_save`<br>`agent_memories_delete`<br>`mcp_servers_*`<br>`tool_permissions_*` | `services/agent.rs`<br>`services/mcp.rs`<br>`services/tool_permissions.rs` | `services/mcp.rs` (Stdio/SSE client) | `integration_tests::phase3_agent_memory_crud_and_prompt_formatting`<br>`integration_tests::phase8_tool_permissions_and_agent_tools`<br>`integration_tests::mcp_http_tool_is_called_from_the_agent`<br>`services::agent::tests::*`<br>`services::mcp::tests::*` |
| **多模态图像 (Image Studio)** | `src/features/image/ImageView.tsx` | `generate_image`<br>`tasks_submit_image`<br>`read_image_file`<br>`tasks_cancel` | `services/image.rs`<br>`services/job_queue.rs`<br>`services/task_scheduler.rs` | `adapters/openai_image.rs`<br>`adapters/fal.rs`<br>`adapters/replicate.rs`<br>`adapters/comfyui.rs` | `integration_tests::image_generate_saves_a_file_and_keeps_bytes_out_of_sqlite`<br>`integration_tests::image_edits_upload_source_and_can_download_a_url`<br>`integration_tests::image_failure_is_recorded_and_validation_is_not`<br>`services::image::tests::*`<br>`ParameterPanel.test.tsx` |
| **多模态视频 (Video Studio)** | `src/features/video/VideoView.tsx` | `generate_video`<br>`tasks_submit_video`<br>`read_video_file`<br>`tasks_cancel` | `services/video.rs`<br>`services/job_queue.rs`<br>`services/task_scheduler.rs` | `adapters/media_job.rs`<br>`adapters/fal.rs`<br>`adapters/replicate.rs`<br>`adapters/generic_http.rs`<br>`adapters/comfyui.rs` | `integration_tests::video_reference_clip_is_sent_and_capability_is_required`<br>`integration_tests::video_failure_is_recorded_and_validation_is_not`<br>`integration_tests::generic_http_polls_a_video_and_keeps_the_key_out_of_sqlite` |
| **多模态音频 (Audio & Voice)** | `src/features/audio/AudioView.tsx` | `generate_audio`<br>`tasks_submit_audio`<br>`read_audio_file`<br>`voices_list`<br>`voices_create`<br>`voices_update`<br>`voices_delete` | `services/audio.rs`<br>`services/voices.rs`<br>`services/job_queue.rs` | `adapters/openai_audio.rs`<br>`adapters/generic_http.rs` | `integration_tests::openai_tts_saves_audio_and_keeps_the_key_out_of_sqlite`<br>`integration_tests::voice_library_copies_and_deletes_the_file`<br>`integration_tests::voice_clone_uses_reference_audio_and_saves_audio_in_history`<br>`services::voices::tests::*`<br>`subtitles.test.ts` |
| **ComfyUI 队列 (ComfyUI)** | `src/features/comfyui/ComfyQueuePanel.tsx` | `comfyui_get_queue`<br>`comfyui_interrupt`<br>`comfyui_clear_queue` | `services/comfyui.rs`<br>`services/task_scheduler.rs` | `adapters/comfyui.rs` | `integration_tests::comfyui_submits_a_bound_workflow_and_saves_the_view`<br>`integration_tests::test_comfyui_queue_control_and_templates`<br>`services::task_scheduler::tests::comfyui_waiters_do_not_exhaust_modality_capacity` |
| **工作流自动化 (Pipeline)** | `src/features/automation/PipelineView.tsx` | `pipelines_list`<br>`pipelines_get`<br>`pipelines_save`<br>`pipelines_delete`<br>`pipelines_presets`<br>`pipelines_run`<br>`tasks_submit_pipeline` | `services/pipeline.rs`<br>`services/job_queue.rs` | 组合调用各模态适配器 | `integration_tests::phase9_automation_workflow_pipeline_test`<br>`workflow.test.ts` |
| **资产与历史搜索 (History & Assets)** | `src/features/history/HistoryView.tsx`<br>`src/features/history/AssetLibraryView.tsx` | `history_list`<br>`history_get`<br>`history_delete`<br>`history_delete_batch`<br>`history_delete_all`<br>`assets_list`<br>`search_unified`<br>`search_rebuild_index` | `services/history.rs`<br>`services/search.rs` | 无 (本地 SQLite + FTS5) | `integration_tests::history_pages_filters_and_keeps_names_after_deletion`<br>`integration_tests::test_history_delete_transactional_and_file_cleanup`<br>`integration_tests::phase14_search_and_asset_library_test`<br>`services::search::tests::*` |
| **用量分析 (Analytics)** | `src/features/analytics/AnalyticsView.tsx` | `analytics_get_summary`<br>`analytics_set_pricing`<br>`analytics_list_pricing` | `services/analytics.rs` | 无 (本地 SQLite 统计) | `integration_tests::phase11_usage_and_cost_analytics_test`<br>`integration_tests::phase3_model_pricing_and_analytics_summary` |
| **API 调试器 (Developer)** | `src/features/developer/DeveloperView.tsx` | `developer_send_request`<br>`developer_list_presets`<br>`developer_save_preset`<br>`developer_delete_preset` | `services/developer.rs` | `services/developer.rs` (底层 HTTP Client) | `integration_tests::phase12_developer_mode_api_debugger_test`<br>`services::developer::tests::*`<br>`curl.test.ts` |
| **模型与注册表 (Models & Registry)** | `src/features/models/ModelsView.tsx`<br>`src/features/settings/ProvidersView.tsx` | `providers_*`<br>`models_*`<br>`presets_*`<br>`registry_*`<br>`plugins_*`<br>`discovery_*` | `services/providers.rs`<br>`services/models.rs`<br>`services/presets.rs`<br>`services/registry.rs`<br>`services/plugins.rs`<br>`services/discovery.rs` | 各 Provider 适配器测试连接与列出模型 | `integration_tests::phase3_registry_catalog_and_import`<br>`integration_tests::phase3_provider_plugins_lifecycle_and_instantiate`<br>`integration_tests::presets_survive_reopen_and_leave_with_the_model`<br>`integration_tests::test_services_local_discovery_and_add`<br>`schema.test.ts` |
| **设置与诊断维护 (Settings & Diagnostics)** | `src/features/settings/SettingsView.tsx`<br>`DiagnosticsView.tsx`<br>`NetworkView.tsx` | `network_*`<br>`config_*`<br>`storage_*`<br>`db_integrity_check`<br>`diagnostics_*`<br>`logs_list` | `services/network.rs`<br>`services/config_io.rs`<br>`services/storage_maintenance.rs`<br>`services/diagnostics.rs`<br>`logging.rs` | 无 (系统网络与本地数据库维护) | `integration_tests::test_config_io_and_network_proxy`<br>`services::network::tests::*`<br>`services::config_io::tests::*`<br>`logging::tests::*` |

---

## 2. 缺陷确认与状态矩阵 (D01—D08)

| 编号 | 缺陷描述 | 判定状态 | 责任模块 | 现状证据与复现结论 |
|---|---|---|---|---|
| **D01** | `registry.rs` 默认 URL 无法访问 | **已修复** | `src-tauri/src/services/registry.rs` | 迁移默认源至仓库维护地址，真实生成 `registry/model-registry.json`；增加 404/离线优雅降级、2MB 流式大小与条目限制、严格 Schema 校验、防版本倒退与原子回滚保护。 |
| **D02** | Portable 数据路径与文档不一致 | **已修复** | `src-tauri/src/lib.rs`<br>`src-tauri/src/state.rs`<br>`src-tauri/src/storage_util.rs`<br>`README.md`<br>`RELEASE_NOTES_1.0.0.md` | 明确落实“方案 A”：Portable 为免安装绿色版，数据与配置安全留存在 Windows 用户数据目录（`%APPDATA%\local.modeldeck`），密钥由系统 DPAPI 保护；修正 README/Release 文案，在启动时增加存储目录读写权限探测与友好提示，前端增加跨设备配置迁移说明。 |
| **D03** | 启动崩溃恢复仅简单置为 `interrupted` | **已修复** | `src-tauri/src/services/task_scheduler.rs`<br>`job_queue.rs`<br>`history.rs`<br>`migrations/021_task_recovery_fields.sql` | 彻底废除一刀切置 `interrupted`；细化状态机引入 `unknown_submission`；新增向后兼容增量迁移 021 记录 `remote_job_id`, `remote_poll_url`, `execution_phase`, `billing_risk`；严格区分本地排队安全重试（无扣费风险）、提交未决防自动重发（防重复扣费）、带远端任务 ID 标识（保留 remote ID 续查）、下载中退出（免重复调用生成 API）与终态不可逆。覆盖全部 7 项恢复场景测试全绿。 |
| **D04** | Agent `run_command` 缺少 OS 沙箱 | **已加固缓解** | `src-tauri/src/services/agent.rs`<br>`src-tauri/src/services/mcp.rs`<br>`src-tauri/src/services/chat.rs` | 针对高危工具（`run_command`、`delete_file`、`write_file`）严禁 `"*"` 通配符放行，强制精准逐次人工确认；MCP 工具建立 `server.id:tool.name` 稳定身份；文件与命令执行引入受控工作区约束（`workspace_root`），通过物理规范化路径严密阻断目录穿越、跨区符号链接与应用数据/系统凭据目录访问；`http_request` 强化 SSRF 防护，拦截云元数据（169.254.x.x 等）、私网网段与内网域名，禁用跨域 302 重定向；记录脱敏结构化审计日志；规范文案消除“绝对安全沙箱”虚假断言。 |
| **D05** | 工作流引擎仅线性循环，非 DAG | **静态确认** | `src-tauri/src/services/pipeline.rs` | `pipeline.rs` 374 行使用 `for (index, step) in input.steps.iter().enumerate()` 顺序遍历执行，无拓扑排序、条件分支或并行调度。 |
| **D06** | Tauri asset 协议范围过度放宽 (`["**"]`) | **已修复** | `src-tauri/tauri.conf.json`<br>`src-tauri/src/commands.rs`<br>`docs/dev/RELEASE_SMOKE_TEST.md` | 收紧 `assetProtocol.scope`，排除系统与敏感根目录，仅允许应用数据目录与用户合法个人媒体目录；IPC 文件读取/保存命令全量增加路径越界与应用数据目录覆盖防护；发布冒烟验证通过。 |
| **D07** | 代码库缺失 GitHub Actions CI 流水线 | **已修复** | `.github/workflows/ci.yml` | 补齐 GitHub Actions 自动化质量门禁与 Windows 发布流水线；包含依赖锁定安装、TypeScript 类型检查、Biome Lint、Vitest 测试、前端构建、Cargo 格式化、Clippy 严格检查 (-D warnings) 与全套测试套件运行。 |
| **D08** | 遗留 `ai-workbench` / `local.aiworkbench` 标识 | **静态确认** | `src-tauri/Cargo.toml`<br>`src-tauri/src/credentials.rs` | `Cargo.toml` 中 `name = "ai-workbench"`，`credentials.rs` 中凭据服务名为 `local.aiworkbench`；直接改名可能破坏用户已保存的系统凭据。 |

---

## 3. 任务跟踪矩阵 (T00—T17)

| 任务 ID | 里程碑 | 优先级 | 任务主题 | 状态 | 关联文件/模块 | 说明 / 验收记录 |
|---|---|---|---|---|---|---|
| **T00** | M0 | P0 (阻塞) | 开发基线、验收矩阵与风险清单 | **DONE** | `docs/dev/BASELINE_20261010.md`<br>`docs/dev/IMPLEMENTATION_STATUS.md` | 本地执行全套基线命令（118 Rust + 22 Vitest 测试全绿），建立全流程映射表与 D01—D08 核验矩阵。Commit: `fbd20736efe66516ada3cc7cc42c8c7c22e021fa`。 |
| **T01** | M1 | P0 | 修复 Registry 默认更新源 | **DONE** | `src-tauri/src/services/registry.rs`<br>`registry/model-registry.json` | 解决默认 URL 404 问题，生成真实合法的仓库维护源文件，实现流式 2MB 限额、Provider/Model/参数多维数量限制、结构校验、版本语义比较防倒退、离线平稳回退及 WireMock 模拟更新/回滚测试（9 项新增测试全绿）。 |
| **T02** | M1 | P0 | 明确并修复 Portable 数据语义 | **DONE** | `src-tauri/src/state.rs`<br>`src-tauri/src/storage_util.rs`<br>`README.md`<br>`RELEASE_NOTES_1.0.0.md`<br>`ConfigIoView.tsx` | 落实方案 A：将 Portable 明确定义为免安装绿色版，消除“配置保存在本地目录随身携带”的错误表述；启动阶段增加存储目录读写权限探测（`ensure_dir_writable`）与磁盘空间底线拦截；设置页引导通过脱敏配置导出进行跨设备迁移。 |
| **T03** | M1 | P0 | 任务崩溃恢复语义与可恢复机制 | **DONE** | `src-tauri/src/services/task_scheduler.rs`<br>`job_queue.rs`<br>`history.rs`<br>`migrations/021_task_recovery_fields.sql`<br>前端状态组件与映射表 | 实现增量迁移 021 记录关键状态字段；细化恢复逻辑为 5 类策略（本地重排队、提交未决、保留远端 ID 续查、下载中断核验、不可续查提示）；引入 `unknown_submission` 状态与防重复扣费保障；覆盖 7 类真实场景测试全绿（Rust 增至 136 项全绿）。 |
| **T04** | M1 | P0 | Agent/MCP 权限与本地资源边界 | **DONE** | `src-tauri/src/services/agent.rs`<br>`src-tauri/src/services/mcp.rs`<br>`src-tauri/src/services/chat.rs` | 实现高危工具通配符 `"*"` 阻断与精准人工确认门禁；受控工作区边界限定与目录穿越/符号链接逃逸阻断；`http_request` 全方位 SSRF 过滤（云元数据、私网、内网域名、禁用跨域重定向）；MCP 稳定身份 `server.id:tool.name`；敏感凭据脱敏审计日志；单元与集成测试 140 项全绿。 |
| **T05** | M1 | P0 | 收紧 Tauri 资源边界、补齐最小发布前检验 | **DONE** | `src-tauri/tauri.conf.json`<br>`src-tauri/src/commands.rs`<br>`docs/dev/RELEASE_SMOKE_TEST.md` | 收紧 `assetProtocol.scope` 白名单作用域；核验 CSP 策略；为普通 IPC 文件读取与保存命令补充严格目录边界保护；形成最小发布前冒烟检验记录；测试增至 141 项全绿。 |
| **T06** | M2 | P1 | GitHub Actions 与自动质量门禁 | **DONE** | `.github/workflows/ci.yml` | 建立 Windows 自动化质量门禁与发布演练工作流，包含类型检查、双端测试、Clippy 严格门禁、构建验证与 SHA256 校验和生成。 |
| **T07** | M2 | P1 | Provider 契约测试及兼容矩阵 | **DONE** | `src-tauri/src/adapters/`<br>`src-tauri/src/integration_tests.rs`<br>`docs/dev/PROVIDER_COMPATIBILITY.md` | 产出详尽的 Provider × 模态 × 功能兼容性矩阵文档；划分 L1~L4 验证等级并消除不实断言；新增 Ollama 通信与连接探测、DeepSeek 思考链 (Reasoning)、Anthropic/Gemini 401/400 鉴权错误脱敏转换、音频与视频模型能力防呆门禁等 4 项核心契约测试（Rust 测试增至 145 项全绿）。 |
| **T08** | M2 | P1 | 任务超时、取消、下载与重试一致性 | **TODO** | `job_queue.rs`<br>`image.rs`<br>`video.rs`<br>`audio.rs` | 统一任务生命周期、超时分类与原子文件下载。 |
| **T09** | M2 | P1 | 数据持久化、凭据及备份恢复 | **TODO** | `src-tauri/src/db.rs`<br>`credentials.rs`<br>`services/config_io.rs` | SQLite WAL 校验、系统凭据兼容平滑迁移。 |
| **T10** | M2 | P1 | 可复现的发布流程 | **TODO** | `RELEASE_CHECKLIST.md`<br>发布工作流与打包脚本 | 统一版本号、产物哈希比对与发布清单。 |
| **T11** | M3 | P2 | 首次使用向导与“第一个结果”路径 | **TODO** | `src/features/settings/`<br>`ModelsView.tsx`<br>`Shell.tsx` | 新用户快速上手体验引导与连通性自测。 |
| **T12** | M3 | P2 | 统一资产元数据、复用与溯源 | **TODO** | `services/history.rs`<br>`services/search.rs`<br>`AssetLibraryView.tsx` | 跨模态资产复用与生成来源追溯。 |
| **T13** | M3 | P2 | 优先完善顺序工作流，不提前重写 DAG | **TODO** | `src-tauri/src/services/pipeline.rs`<br>`PipelineView.tsx` | 类型化输入输出传递、断点重试与变量插值。 |
| **T14** | M3 | P2 | 真实可用的调用成本与模型能力提示 | **TODO** | `services/analytics.rs`<br>`services/registry.rs` | 区分真实消耗与预估消耗，模型限制明示。 |
| **T15** | M3 | P2 | 诊断、代理与体验打磨 | **TODO** | `services/diagnostics.rs`<br>`NetworkView.tsx`<br>`DiagnosticsView.tsx` | 一键自检与敏感信息脱敏导出。 |
| **T16** | M4 | P2 | 三条端到端验收场景 | **TODO** | `docs/dev/E2E_ACCEPTANCE.md` | 安装体验、长视频任务恢复与跨模态生成场景全链条验证。 |
| **T17** | M4 | P3 | 按用户证据选择的增强项目 | **TODO** | 待定 | 根据用户真实痛点在轻量 DAG、批量处理等候选中决策。 |
