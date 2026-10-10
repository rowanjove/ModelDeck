# ModelDeck Provider 契约测试与兼容性矩阵 (Provider Compatibility Matrix)

> **版本**：v1.0.1-stabilization  
> **更新日期**：2026-10-10  
> **状态**：规范落地 / 已完成全量 Mock 契约覆盖与能力门禁测试

---

## 1. 兼容性验证等级定义 (Verification Levels)

为纠正过往文档与界面中可能存在的“全厂商完美兼容”等不严谨表述，ModelDeck 对所有支持的 Provider、模态及功能实行严格的等级划分：

- **L1 静态定义 (Static Defined)**：代码存在、数据结构与参数已建模，但未在自动化用例或实机中验证。
- **L2 Mock 契约覆盖 (Mock Contract Covered)**：在自动化测试集（`src-tauri/src/integration_tests.rs`）中通过 WireMock 建立了端到端协议交互用例，包含鉴权头、请求体、正常响应、4xx/5xx 错误、SSE 分片、思考链或轮询下载。
- **L3 本地实机验证 (Local Verified)**：在干净 Windows 环境下通过本地守护进程（如 Ollama、ComfyUI、F5-TTS）完成了完整任务生命周期验证。
- **L4 真实 API 生产通过 (Live API Verified)**：开发者使用合法真实密钥针对特定厂商端点完成了真实网络调用、产物落盘及结果确认（CI 默认不配置真实密钥，仅运行 L2 Mock 契约）。

> [!IMPORTANT]
> **真实性声明**：未达到 L3/L4 的服务商与模型仅作为“实验性 / 社区配置 (Experimental / Community)”，界面中不得虚标为“完全适配”或“官方推荐”。

---

## 2. 全厂商能力兼容性总表 (Compatibility Matrix)

下表记录了 ModelDeck 内置的 9 类 `ProviderType` 在文本、图像、视频、音频四大模态中的能力覆盖状态。

| Provider 类型 | 适配器模块 | 文本 (Chat / Stream / Reasoning) | 图像 (Txt2Img / Img2Img / Mask) | 视频 (Txt2Vid / Img2Vid / LastFrame) | 音频 (TTS / STT / Clone) | 认证协议与传输规范 |
|---|---|---|---|---|---|---|
| **OpenAI** | `adapters/openai.rs`<br>`adapters/openai_image.rs`<br>`adapters/openai_audio.rs` | **Mock 已覆盖 + 真 API 已通过**<br>(支持 SSE 流式、Token 计量) | **Mock 已覆盖 + 真 API 已通过**<br>(DALL·E 3 / DALL·E 2 遮罩编辑) | **不支持**<br>(OpenAI 官方未开放通用视频 API) | **Mock 已覆盖 + 真 API 已通过**<br>(TTS-1 朗读、Whisper-1 转录) | `Authorization: Bearer <key>`<br>JSON / SSE / Multipart 表单 |
| **OpenAI Compatible** | `adapters/openai.rs` | **Mock 已覆盖 + 真 API 已通过**<br>(DeepSeek-R1 / V3、Qwen、Moonshot；已覆盖 `reasoning_content` 思考链提取与计量) | **未验证**<br>(依赖中转商是否实现 `/v1/images/generations`) | **不支持**<br>(通用协议未标准化视频) | **Mock 已覆盖**<br>(支持第三方兼容的 Whisper / TTS 接口) | `Authorization: Bearer <key>`<br>兼容自定义 `base_url` |
| **Anthropic** | `adapters/anthropic.rs` | **Mock 已覆盖 + 真 API 已通过**<br>(Claude 3.5 Sonnet / Haiku；支持 Messages API、`x-api-key`、Thinking Tokens 计量) | **不支持**<br>(原生协议无独立图像生成接口) | **不支持** | **不支持** | `x-api-key: <key>`<br>`anthropic-version: 2023-06-01` |
| **Gemini** | `adapters/gemini.rs` | **Mock 已覆盖 + 真 API 已通过**<br>(Gemini 1.5 Pro / Flash；支持 `generateContent`、Thoughts Token 统计) | **未验证**<br>(Imagen 3 接口尚未原生适配) | **不支持** | **不支持** | `x-goog-api-key: <key>` 或 Query 参数 |
| **Ollama** | `adapters/ollama.rs` | **Mock 已覆盖 + 本地验证通过**<br>(`/api/chat`、NDJSON/SSE 流式、`/api/tags` 模型探测、多模态 Vision 图像上传) | **不支持** | **不支持** | **不支持** | 无认证 / 本地回环 `http://127.0.0.1:11434` |
| **ComfyUI** | `adapters/comfyui.rs` | **不支持** | **Mock 已覆盖 + 本地验证通过**<br>(JSON 工作流模板绑定、提示词参数注入、单并发队列锁) | **Mock 已覆盖 + 本地验证通过**<br>(首帧上传、工作流驱动视频生成、WebSocket 进度监听) | **不支持** | 无认证 / 本地回环 `http://127.0.0.1:8188`<br>WebSocket + REST API |
| **Replicate** | `adapters/replicate.rs` | **不支持** | **Mock 已覆盖**<br>(Predictions API 轮询、Flux / SDXL 图像输出) | **Mock 已覆盖**<br>(视频预测轮询、超时与取消状态跟踪) | **未验证** | `Authorization: Bearer <token>`<br>异步创建 + 轮询 GET |
| **fal.ai** | `adapters/fal.rs` | **不支持** | **Mock 已覆盖**<br>(Queue API、Flux 队列生成、安全 CDN 下载) | **Mock 已覆盖**<br>(队列轮询与视频资源提取) | **未验证** | `Authorization: Key <key>`<br>异步队列模式 |
| **Generic HTTP** | `adapters/generic_http.rs`<br>`adapters/simple.rs` | **Mock 已覆盖**<br>(通用 HTTP 接口) | **Mock 已覆盖**<br>(HTTP 模板生图) | **Mock 已覆盖**<br>(自定义 HTTP 轮询驱动、支持首尾帧与参考视频注入) | **Mock 已覆盖**<br>(通用音频接口与原始二进制流下载) | 自定义 Header 模板、JSON/Binary 结果提取路径 |

---

## 3. 模态功能与契约测试用例映射

ModelDeck 在 `src-tauri/src/integration_tests.rs` 中为每一项核心能力提供了 WireMock 契约测试：

### 3.1 文本模态 (Text Modality)
1. **OpenAI 兼容文本请求与鉴权头隔离**：
   - 用例：`text_generate_reaches_openai_compatible_provider`
   - 验证：正确发送 Authorization 头，请求完成后密钥不在 SQLite 数据库或审计明文中留存。
2. **长上下文滑动窗口裁剪与流式中断**：
   - 用例：`streaming_chat_keeps_context_bounded_and_truncates_on_regenerate`
   - 验证：超出上下文 Token 上限时按策略裁剪旧消息；重新生成时正确截断后续分支。
3. **DeepSeek-R1 / o1 思考链解析 (Reasoning Extraction)**：
   - 用例：`test_openai_compatible_reasoning_content_contract`
   - 验证：正确提取 `choices[0].message.reasoning_content`，并在 `usage.reasoning_tokens` 中记录思考用量。
4. **Ollama 本地大模型通信契约**：
   - 用例：`test_ollama_provider_contract_and_connection`
   - 验证：`GET /api/tags` 连通性探测正常解析；`POST /api/chat` 正确发送 model 与 messages 结构并提取 assistant 回复。
5. **Anthropic Claude Messages 契约**：
   - 用例：`anthropic_messages_return_text_without_storing_the_key`
   - 验证：发送 `x-api-key` 与系统提示词，正确解析 `content[0].text`。
6. **Gemini generateContent 契约**：
   - 用例：`gemini_generate_content_returns_text`
   - 验证：发送 `x-goog-api-key`，解析 `candidates[0].content.parts[0].text` 与 `usageMetadata`。

### 3.2 图像模态 (Image Modality)
1. **文生图落盘与元数据解耦**：
   - 用例：`image_generate_saves_a_file_and_keeps_bytes_out_of_sqlite`
   - 验证：图像二进制原子写入 `%APPDATA%\local.modeldeck\images` 目录，SQLite 仅存相对路径与元数据，无大字段污染。
2. **图生图与遮罩上传 (Inpainting)**：
   - 用例：`image_edits_upload_source_and_can_download_a_url`
   - 验证：正确将源图与 Mask 处理为合规 Payload，支持远端 CDN 图片拉取。
3. **Replicate Predictions 异步轮询**：
   - 用例：`replicate_prediction_polls_and_saves_the_image`
   - 验证：创建 prediction 后进入轮询循环（starting → processing → succeeded），自动下载产物并写入历史。
4. **fal.ai 队列下载与安全校验**：
   - 用例：`fal_queue_downloads_the_image_without_sending_the_key_to_the_file`
   - 验证：提交队列后监听状态，下载图像文件时确保请求头不泄露。

### 3.3 视频模态 (Video Modality)
1. **ComfyUI 工作流视频生成**：
   - 用例：`comfyui_video_uploads_the_first_frame_and_saves_the_view`
   - 验证：首帧图片通过 `/upload/image` 预先上传，节点输入自动映射为上传后的文件名。
2. **Generic HTTP 异步轮询与取消**：
   - 用例：`generic_http_polls_a_video_and_keeps_the_key_out_of_sqlite`
   - 验证：按配置的 `poll.interval_ms`、`status_path` 进行轮询，检测到成功态后拉取最终视频文件。

### 3.4 音频模态 (Audio Modality)
1. **OpenAI TTS 语音合成**：
   - 用例：`openai_tts_saves_audio_and_keeps_the_key_out_of_sqlite`
   - 验证：向 `/v1/audio/speech` 发送指定文本与音色，流式接收二进制音频并落盘。
2. **OpenAI Whisper 语音转录 (STT)**：
   - 用例：`openai_transcription_saves_the_transcript_in_history`
   - 验证：Multipart 表单上传音频文件至 `/v1/audio/transcriptions`，转录结果写入数据库与资产库。
3. **声音克隆与参考音频关联**：
   - 用例：`voice_clone_uses_reference_audio_and_saves_audio_in_history`
   - 验证：在模型具备 `voiceClone` 能力时绑定参考样本文件。

---

## 4. 容错与异常注入契约 (Fault Injection & Resilience)

在生产环境中，网络抖动和服务商故障极易导致客户端挂起或数据丢失。ModelDeck 在适配器层统一实现以下契约保障：

```mermaid
flowchart TD
    Req[客户端发起调用] --> CB{断路器检查 (Circuit Breaker)}
    CB -- 熔断开路 --> ErrCB[立即报错: 连续失败过多熔断保护中]
    CB -- 闭路正常 --> Exec[发送 HTTP 请求]
    Exec --> Resp{响应状态码}
    Resp -- 200 OK --> Success[解析响应 & 落盘 & 扣费记录]
    Resp -- 401/403 --> AuthErr[统一转换为 AUTH 错误 (脱敏 API Key)]
    Resp -- 429 --> Retry[指数退避重试 (Exponential Backoff)]
    Retry -- 重试成功 --> Success
    Retry -- 超出上限 --> Err429[记录失败 & 提醒速率限制]
    Resp -- 5xx --> IncrFail[断路器失败计数 +1]
    IncrFail --> Err5xx[友好提示: 服务商暂时不可用]
    Resp -- 畸形 JSON/502 HTML --> Malformed[捕获解析异常 -> 记录失败任务 -> 清理磁盘孤儿文件]
```

### 4.1 认证与凭据错误 (401 / 403)
- **契约要求**：当服务商返回 401 或 403 时，系统必须转义为统一的 `AppError::auth`，错误信息呈现给用户以说明凭据无效，**严禁将包含真实 API Key 的请求头或 URL 打印到错误消息或前端界面中**。
- **验证用例**：`test_anthropic_and_gemini_error_contract`、`auth_and_schema_errors_do_not_leak_the_key`。

### 4.2 速率限制与重试 (429 Rate Limit)
- **契约要求**：遇到 429 时，客户端执行指数退避重试（最大重试次数有上限），并在日志中记录 `http_retry` 事件；重试成功后单调推进状态。
- **验证用例**：`fault_injection_rate_limit_backoff_and_recovery`。

### 4.3 服务端崩溃与断路器 (5xx & Circuit Breaker)
- **契约要求**：当目标 Provider 连续发生 5 次 5xx 严重异常时，断路器自动跳闸，后续请求在本地直接被快速失败（Fast Fail）拦截，阻止无效重试导致服务商封禁或界面无限转圈。
- **验证用例**：`fault_injection_circuit_breaker_trips_on_consecutive_server_errors`。

### 4.4 畸形响应与非标准 HTML (Malformed Responses)
- **契约要求**：若代理网关在 502/504 时返回 `<!DOCTYPE html>...` 等非 JSON 格式内容，适配器不得产生 panic，而应解析为非预期响应错误，将任务安全置为 `failed`，并清理可能生成的临时残存文件。
- **验证用例**：`fault_injection_malformed_json_response_handled_gracefully`。

### 4.5 SSE 分片与中文多字节解码 (Streaming Decoder)
- **契约要求**：在 SSE 文本流式传输中，中文字符或 Emoji 可能被切碎在不同的网络数据包中。系统采用 `Utf8ChunkDecoder` + `LineParser` 双阶段解析器，严格保证多字节字符拼接完整性，不产生乱码与丢字。
- **验证用例**：`sse::tests::utf8_chunk_decoder_handles_split_multibyte_chinese_and_emoji`、`sse::tests::parses_sse_split_across_chunks`。

---

## 5. 模型能力防呆拦截门禁 (Feature Capability Guards)

为防止客户端将高级参数（如尾帧控制、语音克隆、参考音频）发送给不支持该功能的模型导致隐性扣费或服务端未知报错，服务层在调用适配器前执行严格的防呆检查：

| 参数 / 资产 | 必需声明的 Capability | 校验未通过时的契约行为 |
|---|---|---|
| **参考音频 (`reference_audio`)** | `capabilities.voiceClone: true` | 立即拦截报错：`"当前模型不接受参考音频"`，且不产生生成任务或写入扣费风险。 |
| **源音频转语音 (`source_audio`)** | `capabilities.speechToSpeech: true` | 立即拦截报错：`"当前模型不支持语音对语音变换"`。 |
| **视频尾帧 (`last_frame`)** | `capabilities.lastFrame: true` | 立即拦截报错：`"该模型没有尾帧能力"`；同时要求尾帧必须伴随首帧输入。 |
| **视频参考素材 (`reference_videos`)** | `capabilities.referenceVideos: true` | 立即拦截报错：`"该模型没有参考视频能力"`。 |
| **图像遮罩 (`mask_base64`)** | `capabilities.mask: true` | 立即拦截报错：`"该模型不支持局部重绘"`。 |

- **验证用例**：
  - `test_feature_capabilities_strict_contract_enforcement`
  - `video_reference_clip_is_sent_and_capability_is_required`

---

## 6. 实机实测记录清单 (Live Smoke Tests)

在发布前由维护团队进行的真机验证基线记录：

| 验证对象 | 接口 / 版本 | 实测模型 ID | 实测输入特征 | 验证结论 | 限制说明 |
|---|---|---|---|---|---|
| **OpenAI 官方** | `/v1/chat/completions` (2026Q1) | `gpt-4o` | 复杂提示词 + 函数工具调用 | **通过** (L4) | 官方调用延迟与流式平稳。 |
| **DeepSeek (兼容)** | `/v1/chat/completions` (v3/r1) | `deepseek-reasoner` | 复杂逻辑推理与数学题 | **通过** (L4) | 成功回显思考链，UI 正确折叠展开。 |
| **Anthropic 官方** | `/v1/messages` (2023-06-01) | `claude-3-5-sonnet-20241022` | 代码生成与多轮问答 | **通过** (L4) | 认证头与角色轮转正常。 |
| **Ollama 本地** | `http://127.0.0.1:11434` (0.3.x) | `llama3.1:8b` | 本地中文多轮对话 | **通过** (L3) | 免配置 Key，连接测试毫秒级响应。 |
| **ComfyUI 本地** | `http://127.0.0.1:8188` | `FLUX.1-schnell` (工作流绑定) | 提示词与随机种子映射 | **通过** (L3) | 单并发锁有效，生成图像原子落盘。 |

> [!NOTE]
> 对于任何未列入上表的第三方聚合服务商、转发站点或社区新模型，用户可使用内置的 **API 调试器 (Developer Mode)** 或 **连接测试** 按钮先行验证，平台不对其 SLA 与特定参数兼容性提供无条件保证。
