# AI Workbench Roadmap

本文件用于规划 PRD v1.0 完成后的后续演进。

原则：

- PRD 负责当前版本交付。
- ROADMAP 负责后续方向。
- 不因为 Roadmap 中存在某功能，就提前污染当前架构。
- 每个阶段开始前重新评估需求、现有代码和模型生态，不机械执行旧规划。
- 优先增强已有核心能力，而不是不断增加一级功能。

# Phase 1 — Core Hardening

目标：

> 把 v1 从“能用”提升到“稳定长期可用”。

重点检查和改进：

## Provider / Model

- Provider 配置导入导出
- Model Definition 导入导出
- Provider 连通性诊断
- Model 自动发现
- 模型能力自动探测
- Provider Endpoint 自动识别
- OpenAI Compatible 兼容性增强
- Provider 超时 / 重试 / Rate Limit 策略
- Provider 独立代理配置

## Parameter Engine

增强 Schema：

- minItems / maxItems
- dependencies
- conditional visibility
- dynamic options
- nested object
- array object
- parameter groups
- presets
- provider-specific override

目标：

> 尽可能不修改 UI 代码即可支持新模型。

## Task System

增强：

- Retry
- Resume
- Cancel
- Detached Task
- Task Queue
- Concurrent Limit
- Priority
- Timeout
- Failed Task Recovery

避免视频等长任务因软件关闭而完全丢失。

# Phase 2 — Model Registry

目标：

> 新模型出现后，不需要发布新版 EXE。

建立独立 Model Registry。

结构：

```
registry/

providers/
models/
templates/
schemas/
```

Registry 内容：

- Provider Definition
- Model Definition
- Capabilities
- Parameters
- Constraints
- Endpoint Mapping
- Recommended Defaults

支持：

```
Update Registry
```

远程 Registry 建议通过 GitHub Release / Raw 文件维护。

软件应记录：

```
registry_version
```

并支持：

- 在线更新
- 本地缓存
- 回滚
- 自定义 Registry
- 禁止自动覆盖用户修改

# Phase 3 — Advanced Text

完善文本工作台。

## Compare

支持同一 Prompt 同时发送给：

```
GPT
Claude
Gemini
DeepSeek
Local Model
```

并排展示：

- Output
- Latency
- Tokens
- Cost

支持：

```
Compare 2–4 Models
```

不要无限并发。

## Prompt Preset

增加轻量 Prompt Preset：

- System Prompt
- User Template
- Parameters
- Model

不建设 Prompt 商店。

## Context Management

加入：

- Context Token Monitor
- Sliding Window
- Auto Summary
- Context Compaction
- Important Message Pin
- Manual Context Selection

## Responses / Reasoning API

针对支持高级接口的模型增加：

- reasoning effort
- reasoning tokens
- structured output
- tool choice
- parallel tools
- response continuation

仍通过 Capability 控制。

# Phase 4 — Image Workspace 2.0

目标：

> 从单纯 API 调用升级为真正好用的图像生成工作台。

增加：

## Reference Manager

每张参考图片拥有：

- Role
- Weight
- Description
- Crop
- Enable / Disable

Role：

```
Character
Style
Pose
Composition
Structure
General
```

## Basic Canvas

保持轻量，只加入：

- Crop
- Resize
- Mask Brush
- Eraser
- Invert Mask
- Expand Canvas

不做 Photoshop。

## Variation

生成结果支持：

```
Use as Source
Use as Reference
Edit
Variation
Upscale
```

形成连续创作流程。

## Batch Generation

支持：

```
Prompt × Seed × Model
```

组合生成。

必须设置并发上限。

# Phase 5 — ComfyUI Deep Integration

ComfyUI 从普通 Provider 升级为本地多媒体核心后端。

支持：

## Workflow Import

导入：

```
workflow_api.json
```

自动分析可能输入：

- Prompt
- Negative Prompt
- Seed
- Image
- Mask
- Width
- Height
- Frames

用户手动确认映射。

## Workflow Parameter Mapping

例如：

```
prompt
→ Node 6 / text

seed
→ Node 12 / seed
```

保存为 Workflow Model Definition。

## Workflow Library

展示：

```
Flux Dev
SDXL
Wan I2V
Hunyuan Video
Custom
```

用户只看到正常参数界面，不直接接触节点图。

## ComfyUI Queue

显示：

- Queue
- Running
- Progress
- Cancel
- History

# Phase 6 — Video Workspace 2.0

完善视频能力。

支持：

## Reference System

- First Frame
- Last Frame
- Multiple Images
- Reference Video
- Reference Audio

每个资源允许：

- role
- weight
- description

## Shot Preset

增加标准化参数：

```
Camera
Motion
Shot
Duration
FPS
Resolution
```

仅在模型支持时显示。

## Continuation

允许：

```
Generated Video
→ Last Frame
→ Next Generation
```

实现轻量连续视频生成。

## Storyboard

后期增加轻量 Storyboard：

```
Shot 01
Shot 02
Shot 03
```

每个 Shot：

- Prompt
- Reference
- First Frame
- Last Frame
- Model
- Duration

注意：

不发展成专业视频剪辑软件。

# Phase 7 — Audio Workspace 2.0

支持：

## TTS

完善：

- Voice
- Emotion
- Style
- Speed
- Pitch
- Language

## Voice Library

完善：

- Multiple References
- Transcript
- Language
- Tags
- Notes
- Preview

## Voice Clone

统一：

```
Reference Audio
Reference Transcript
Target Text
```

## STT

支持：

- Whisper compatible
- Timestamp
- Word Timestamp
- SRT
- VTT
- Speaker Detection

## Speech-to-Speech

支持：

```
Source Audio
↓
Target Voice
↓
Output
```

# Phase 8 — Agent 2.0

在基础 Agent 稳定后继续扩展。

## MCP

增加 MCP Client：

- stdio
- SSE / HTTP

支持：

- Server 管理
- Tool 列表
- Enable / Disable
- Permission

## Agent Memory

区分：

```
Conversation Context
Session Memory
Long-term Memory
```

第一版 Long-term Memory 只使用简单本地存储。

不要直接建设复杂向量数据库体系。

## Context Compaction

长对话：

```
Recent Messages
+
Summary
+
Pinned Context
```

## Tool Permission

工具分级：

```
Safe
Confirm
Restricted
```

涉及：

- 写文件
- Shell
- 删除
- 网络请求

允许用户配置。

# Phase 9 — Automation / Workflow

此阶段再考虑轻量工作流。

不要复制 ComfyUI / n8n。

采用简单 Pipeline：

```
Input
↓
Model
↓
Transform
↓
Model
↓
Output
```

例如：

```
文章
↓
LLM 摘要
↓
Image Prompt
↓
Image Model
```

节点控制在少量核心类型。

目标是：

> AI 模型串联。

而不是通用自动化平台。

# Phase 10 — Plugin / Adapter SDK

当内置 Provider 数量开始快速增加后，再做插件机制。

允许第三方实现：

```
Provider Adapter
Model Definition
Response Parser
```

但 Plugin API 必须严格限制。

不要允许插件随意访问：

- Credential
- 全磁盘
- 任意系统命令

# Phase 11 — Usage & Cost

加入本地 Usage 数据。

记录：

```
Requests
Tokens
Generation Count
Latency
Estimated Cost
```

可以查看：

```
Today
7 Days
30 Days
```

按：

```
Provider
Model
Modality
```

统计。

不做复杂商业 Dashboard。

# Phase 12 — Developer Mode

项目本质上也可以成为 AI API 调试器。

增加 Developer Mode：

- Raw Request
- Raw Response
- Headers
- SSE Events
- WebSocket Events
- Copy cURL
- Request Replay
- Schema Inspector

支持保存：

```
API Test Preset
```

形成：

> AI 领域的小型 Postman。

# Phase 13 — Local Service Manager

仍然不直接内置模型。

但可以增强：

```
Local Services
```

识别：

- Ollama
- LM Studio
- llama.cpp
- vLLM
- ComfyUI
- F5-TTS
- CosyVoice

显示：

```
Online
Offline
Version
Endpoint
Models
```

可以：

```
Open Service
Refresh
Add Provider
```

不要承担安装和升级这些服务。

# Phase 14 — Search & Library

当历史数据变多后：

统一搜索：

```
Conversations
Images
Videos
Audio
Tasks
Prompts
Models
```

支持：

- Full Text Search
- Model
- Provider
- Date
- Tag
- Modality

图片和视频提供轻量 Asset Library。

# Phase 15 — UX Refinement

核心功能成熟后，再集中优化 UI。

重点：

- 信息层级
- 参数密度
- 空状态
- 拖放体验
- 快捷键
- Model Switcher
- Command Palette
- Context Menu
- Loading
- Error State
- Task Progress

减少：

- 无意义卡片
- 多层弹窗
- 过度边框
- AI SaaS 风格
- 无意义动画

# 长期功能候选

以下功能暂不确定，未来按实际需求决定。

## 可考虑

- 多模型 Compare
- Batch Generation
- Prompt Variables
- Simple Workflow
- MCP
- Adapter SDK
- Registry Community Sources
- Portable Workspace
- Backup / Restore
- Local Encryption
- Cost Budget
- Generation Metadata Embedding

## 谨慎考虑

- RAG
- Knowledge Base
- Multi-Agent
- Browser Automation
- Shell Agent
- Cloud Sync
- Account System
- Team Collaboration

## 原则上不做

- 自建云推理服务
- 模型训练
- 本地模型下载平台
- 完整节点式工作流平台
- 专业图像编辑器
- 专业视频编辑器
- AI 社交平台
- Prompt 商城

# 版本建议

建议不要使用：

```
v1
v2
v3
```

直接映射功能成熟度。

例如：

```
0.1
核心架构

0.2
Text

0.3
Image

0.4
Video

0.5
Audio

0.6
Agent

0.7
ComfyUI

0.8
Registry

0.9
Hardening

1.0
稳定公开版本
```

1.0 之后：

```
1.1
Text / Agent Enhancement

1.2
Image Workspace

1.3
Video Workspace

1.4
Audio Workspace

1.5
Developer Mode

1.6
Plugin SDK
```

不要为了发布版本强塞功能。

# 每阶段启动规则

开始新的 Roadmap 阶段前，AI 必须先：

1. 阅读当前 PRD。
2. 阅读 ROADMAP。
3. 检查当前完整代码。
4. 检查最近开发记录。
5. 判断 Roadmap 内容是否仍适合当前模型生态。
6. 删除已经过时或没有价值的需求。
7. 确认本阶段最小范围。
8. 再开始开发。

ROADMAP 不是不可修改的需求文档。

如果实际开发证明某方向没有价值，应直接修改 Roadmap，而不是机械实现。

# 长期架构底线

无论项目发展到什么阶段，都保持：

```
Provider ≠ Model

Model → Capability

Capability → UI

UI → Canonical Request

Canonical Request → Adapter

Adapter → Provider
```

新模型原则上应通过：

```
Model Definition
+
Adapter
```

接入。

如果每增加一个模型都必须修改多个页面，则优先判断当前架构是否已经退化。

# 最终愿景

长期目标不是堆最多的 AI 功能。

而是：

> 成为一个轻量、本地优先、协议无关、模型无关的多模态 AI 工作台。

用户面对的是统一体验：

```
配置模型
↓
选择模型
↓
提供输入
↓
调整模型真正支持的参数
↓
生成
↓
继续编辑 / 引用 / 对话 / 调用
```

至于底层使用：

```
OpenAI
Anthropic
Gemini
DeepSeek
OpenRouter
Ollama
LM Studio
ComfyUI
Flux
Wan
Seedance
F5-TTS
CosyVoice
其他未来模型
```

都由 Provider / Adapter / Model Registry 负责消化差异。

项目的核心竞争力应始终是：

**轻、统一、可配置、扩展新模型快。**