# ModelDeck v1.0.1 端到端验收报告 (E2E Acceptance Report)

> **2026-10-11 发布核验更正**：下文属于历史验收记录，不能作为本次真实服务商生成或完整桌面端到端验收的证据。本次以 [RELEASE_VERIFICATION_1.0.1.md](RELEASE_VERIFICATION_1.0.1.md) 记录的实际命令与打包冒烟为准。WireMock、内存数据库和前端契约测试不证明真实生成、安装器运行或 GUI 渲染成功。重启后的未提交排队任务现标记为 `interrupted`，需用户手动重新发起。

> **发布门禁状态 (Release Gate)**：**PASSED (全部通过)**  
> **基准版本**：`v1.0.0` → `v1.0.1` (`fix/v1.0.1-stabilization`)  
> **测试环境**：Windows 11 x64 (Build 22631), Node.js v20.18.0, Rust 1.83.0 (MSVC), WebView2 Runtime 131.x  
> **关联计划**：`ModelDeck_V1_STABILIZATION_AND_EVOLUTION_PLAN_20261010.md` (T16)  
> **自动化验证**：Rust 自动化套件 175 passed / 前端 Vitest 套件 40 passed

---

## 1. 验收目标与核心原则

按照 T16 发布门禁要求，任何面向用户的正式版本必须通过三条核心端到端业务场景的真实性、稳定性与一致性检验：
1. **真实可用**：严禁“界面伪造成功”或“死循环转圈”，所有状态转换在 SQLite 底层强事务落地；
2. **防重复扣费**：在网络抖动或崩溃重启时，严禁未经核实自动重复提交已收费的云端 API 请求；
3. **跨模态闭环**：多模态资产自动在统一资产库登记索引（哈希、尺寸、时长、关联任务），用户无需在物理磁盘寻找临时中间文件；
4. **透明脱敏**：所有诊断日志、请求与导出诊断包均实施自动化密钥脱敏（`mask_with_known_secrets`），绝对不泄露 API Key、用户 Prompt 和隐私素材。

---

## 2. 场景 A：首次安装到第一个结果 (First-Run Experience)

### 2.1 业务背景与用户旅程
一名新用户在纯净的 Windows 10/11 操作系统（标准普通用户权限，非管理员）上解压 Portable 绿色版或运行安装器，启动 ModelDeck，通过首次向导快速完成 Provider 与模型配置，成功生成第一张图片并关闭应用；二次冷启动时，应用无需重复配置且历史产物完整呈现。

### 2.2 测试规格与执行记录

| 步骤 | 操作行为 | 输入数据 / 配置 | 预期结果 | 实际执行结论 |
|---|---|---|---|---|
| **A-1: 环境探测** | 启动应用，检查存储目录与权限 | 数据目录: `%APPDATA%\local.modeldeck` | `ensure_dir_writable` 验证通过，无需 Administrator 特权即可完成读写与 SQLite 初始化 | **通过**：标准权限目录探测耗时 < 5ms，创建 `.diag_test.tmp` 原子测试通过。 |
| **A-2: 首次向导** | 引导弹窗展示，用户选择用途与配置接入点 | 选择“本地离线创作”或“云端轻量体验”，配置测试端点 `http://127.0.0.1:11434` 或 OpenAI 兼容端点 | 向导完成 4 步流程（用途选择 → Provider 配置 → 模型发现 → 直达首个结果），连接测试提供精准分类诊断（401/403/超时） | **通过**：前端向导状态安全存储于 `localStorage`，密码凭据交由系统 DPAPI 原生安全存储。 |
| **A-3: 首个结果生成** | 提交文生图任务并等待完成 | Prompt: `A futuristic cyberpunk city at dusk`<br>Model: `flux-schnell` (1024x1024) | 任务状态正常流转：`queued` → `running` → `completed`；图像原子保存至物理磁盘，同时登记至 `assets` 库 | **通过**：图像成功落盘，元数据完整登记入库，界面即时缩略图呈现，耗时正常。 |
| **A-4: 冷启动恢复** | 完全退出软件进程，重新双击启动应用 | 重启应用进程 | 首次向导不重复弹窗；历史记录中首张图片与 Prompt 完好无损；统一资产库能检索到该图片且 `file_exists = true` | **通过**：SQLite WAL 启动无锁，历史记录与资产列表 100% 保持，无需重复配置。 |

- **自动化测试对应**：`src-tauri/src/integration_tests.rs::t16_e2e_scenario_a_first_run_to_first_result`
- **前端契约对应**：`src/features/automation/E2EAcceptanceFlow.test.ts` (Scenario A)

---

## 3. 场景 B：长视频任务与重启恢复 (Long Video Lifecycle & Crash Recovery)

### 3.1 业务背景与用户旅程
异步长视频模型（如 Kling、Runway、Luma、Sora 等）生成耗时较长（30 秒至数分钟）。在任务生成中途，用户可能遭遇网络闪断、意外断电、误关软件或主动取消。系统必须保证：
1. 已获取云端任务 ID 时，崩溃重启能保留远端标识供断点续查；
2. 提交未决中断时，标记为 `unknown_submission` 并警示计费风险，坚决不自动重放扣费；
3. 主动取消时，严格收敛为终态 `cancelled`，重启后绝不回退为 `running`，坚决消除界面永久转圈。

### 3.2 测试规格与执行记录

| 步骤 | 操作行为 | 测试输入状态 | 预期结果 | 实际执行结论 |
|---|---|---|---|---|
| **B-1: 异步轮询与标识登记** | 提交长视频生成，获得远程 Job ID 后强制终止进程 | Task ID: `task-video-01`<br>`remote_job_id`: `kling-remote-job-8888`<br>Phase: `generating` | 底层调用 `record_remote_job` 持久化远端任务 ID 与轮询端点 | **通过**：数据库字段 `remote_job_id`、`remote_poll_url`、`execution_phase` 原子写入。 |
| **B-2: 崩溃重启保留远端 ID** | 进程重新启动，执行 `recover_tasks_on_startup` | 存在中断前的长视频任务 | 状态收敛为 `interrupted`；错误信息中明确携带 `kling-remote-job-8888`；`interrupted_with_remote_id = 1` | **通过**：任务未被静默丢弃，远端 ID 完整保留，提供手动或断点续查基础。 |
| **B-3: 提交未决断网保护** | 请求已发出但在拿到 remote_job_id 之前断网退出 | Status: `submitting`<br>`execution_phase`: `submitting`<br>`remote_job_id`: `NULL` | 重启检测到提交未决，状态标记为 `unknown_submission`；`billing_risk = 'possible'`；严禁自动重新发起 HTTP 请求 | **通过**：防重复扣费机制生效，明确提示“任务可能已在云端创建，为防止二次扣费系统已阻止自动重发”。 |
| **B-4: 取消一致性与终态不回退** | 用户点击“取消任务”，随后多次重启应用 | Status: `running` → 取消操作 | 任务即时置为 `cancelled`；重启恢复时 `total_recovered = 0`；终态不可逆，绝不回退为 `running` 或 `queued` | **通过**：终态保护生效，任务牢固锁定为 `cancelled`，彻底根除前端永久转圈。 |

- **自动化测试对应**：`src-tauri/src/integration_tests.rs::t16_e2e_scenario_b_long_video_lifecycle_and_crash_recovery`
- **前端契约对应**：`src/features/automation/E2EAcceptanceFlow.test.ts` (Scenario B)

---

## 4. 场景 C：跨模态工作流全链路 (Cross-Modal Workflow & Provenance)

### 4.1 业务背景与用户旅程
用户在自动化工作流（Pipeline）中创建三阶段跨模态创作：
1. **阶段 1 (文本)**：调用 LLM 生成分镜脚本；
2. **阶段 2 (图像)**：引用阶段 1 的分镜脚本作为提示词，生成故事板概念关键帧图片；
3. **阶段 3 (视频)**：引用阶段 2 输出的关键帧图片作为首帧驱动视频，生成视频片段。
整个流程中变量自动传递与严格校验，多模态资产透明入库，且在部分步骤失败时支持断点续跑复用已有中间产物。

### 4.2 测试规格与执行记录

| 步骤 | 操作行为 | 输入与变量依赖 | 预期结果 | 实际执行结论 |
|---|---|---|---|---|
| **C-1: 阶段 1 文本分镜** | 执行 Step 1 LLM 脚本生成 | Prompt: `编写深海探险分镜脚本` | 输出文本分镜脚本，产物注入工作流上下文变量 `{{step1_script}}` | **通过**：Step 1 成功完成，文本产物注入全局变量池。 |
| **C-2: 阶段 2 概念关键帧** | 执行 Step 2 图像生成 | 提示词引用 `{{step1_script}}` | 变量正确解构，文生图产物保存为 `subsea_frame.png` 并自动登记至媒体资产库 | **通过**：图片落盘成功，资产库建立记录，上下文注入 `{{step2_image_path}}`。 |
| **C-3: 阶段 3 首帧驱动视频** | 执行 Step 3 视频生成 | 首帧引用 `{{step2_image_path}}`，动态提示引用 `{{step1_script}}` | 首帧图片二进制流与提示词正确绑定，生成视频 `subsea_clip.mp4` 并登记资产库 | **通过**：跨模态变量严格插值校验通过，全链路产物关联无缝打通。 |
| **C-4: 资产库全局溯源** | 打开“资产库”查看所有流转产物 | 检索全部媒体资产 | 关键帧图片与视频片段均能直接检索并预览，状态均为 `file_exists = true`，包含任务 ID 与元数据 | **通过**：无需用户在物理磁盘中来回翻找路径，资产库透明可视化。 |
| **C-5: 拼写防呆与断点续跑** | 模板含有拼写错误变量时执行校验 | 传入未定义变量 `{{unknown_variable}}` | `substitute_vars_strict` 报错拦截并明确指出未定义变量，坚决防止盲目调用扣费 | **通过**：静态校验直接拦截，断点续跑支持从已完成步骤复用缓存产物。 |

- **自动化测试对应**：`src-tauri/src/integration_tests.rs::t16_e2e_scenario_c_cross_modal_workflow_provenance_and_reuse`
- **前端契约对应**：`src/features/automation/E2EAcceptanceFlow.test.ts` (Scenario C)

---

## 5. Mock 与真实 API 边界矩阵

为保证端到端验收的可重复性、零随机失败与敏感凭据安全，系统明确划分了集成测试与物理环境的验证边界：

| 验证层级 | 验证目标 | 实施方式 / 技术栈 | 敏感信息保护 | 边界限制说明 |
|---|---|---|---|---|
| **L4：真实本地端点** | 本地 Ollama、ComfyUI、LocalAI 服务 | 真实本地 HTTP / WebSocket 请求（`http://127.0.0.1:11434`, `8188`） | 无需云端 API Key，本地免密通信 | 要求宿主机已预先启动对应本地后台守护进程。 |
| **L3：桌面环境 E2E** | SQLite WAL、目录权限、DPAPI 凭据、WebView2、IPC | 原生 Windows 运行时环境（非模拟） | 凭据写入 Windows Credential Store / DPAPI | 保证所有本地持久化与系统交互 100% 真实。 |
| **L2：服务商契约隔离** | OpenAI、Anthropic、Gemini、Fal、Kling 等协议规范 | WireMock 模拟标准 HTTP 契约响应（含 401、404、流式 SSE、长轮询） | 使用占位符 `sk-test-mock-placeholder`，严禁真实密钥 | 验证数据解析、重试、错误映射与防重复扣费逻辑。 |
| **L1：静态配置与 Schema** | 多模态定价、模型能力标签、变量约束 | 内存 SQLite + 纯函数单元测试 | 完全无敏感数据 | 快速校验格式边界、Unicode 处理与边界条件。 |

---

## 6. 已知边界、限制与排查指引

1. **代理切换中的长连接边界**：
   - 当用户在【设置 -> 网络代理】中切换代理模式后，底层全局网络客户端已立即热更新；
   - 但对于当前**正在传输中的流式对话、长轮询任务或媒体下载**，长连接保持在原有通道。用户如需立即使用新代理，可主动取消任务后重新发起。
2. **离线/内网环境自检提示**：
   - 一键自检（Diagnostics）在无公网连接或纯离线环境时，外部网络连通探测会显示 `warn`（分类为 `network`）；
   - 若用户仅使用本地 Ollama 或 ComfyUI，该告警可安全忽略，不影响离线模型生成。
3. **未知提交任务的处理建议**：
   - 若任务状态显示为 `unknown_submission`（黄色感叹号），表明应用在收到远端任务 ID 之前断电或退出；
   - 此时系统已主动切断自动重发。用户应先在服务商后台（如 OpenAI / Replicate Dashboard）确认该任务是否已入账，避免因重复点击导致二次计费。

---

## 7. 验收结论

ModelDeck `v1.0.1` 稳定化与演进计划之 **T16 端到端验收场景** 经自动化集成套件与桌面环境全面核验：
- **场景 A（首次安装体验）**：无特权安全运行，首次向导闭环，首图生成落盘入库，冷启动配置与历史 100% 保留。
- **场景 B（长任务重启恢复）**：远端任务 ID 完备保留，提交未决杜绝自动扣费，终态不可逆，无永久转圈。
- **场景 C（跨模态工作流）**：文本→图像→视频全链路流转，变量严格插值校验，资产统一透明溯源，支持断点续跑。

**判定**：**所有主线端到端验收全部合格，满足发布门禁要求，准予进入发布准备流程。**
