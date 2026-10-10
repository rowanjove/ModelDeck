# 轻量多模态 AI 模型工作台 PRD v1.0

## 1. 项目定位

开发一个 Windows 本地优先的轻量 AI 模型调用工具，用于统一配置、管理和调用：

- 文本模型
- 图像模型
- 视频模型
- 音频模型
- Agent

软件本身不承载大模型推理，不内置大型模型文件，只作为统一的：

> 模型配置器 + API 客户端 + 多模态生成工作台 + Agent 会话工具

核心目标：

> 用一个轻量 EXE 管理云端 API、本地 OpenAI Compatible 服务、Ollama、ComfyUI 及其他自定义 AI 服务。

# 2. 产品原则

必须遵循以下原则。

## 2.1 本地优先

所有：

- Provider 配置
- 模型配置
- 对话
- Agent
- 生成历史
- 参数预设
- 文件索引

默认保存在本机。

不建设强制云端账户体系。

## 2.2 EXE 不承载模型

不得：

- 内置 LLM
- 内置 Stable Diffusion
- 内置视频模型
- 内置 TTS 大模型
- 打包 Python 环境

本地模型通过服务方式连接：

```
LM Studio
Ollama
llama.cpp
vLLM
ComfyUI
F5-TTS
CosyVoice
其他 HTTP 服务
```

## 2.3 Provider 与 Model 分离

Provider 表示连接方式。

例如：

```
OpenRouter
http://openrouter.ai/...
API Key
```

Model 表示：

```
GPT
Claude
Gemini
Flux
Seedance
Wan
```

一个 Provider 可以拥有多个 Model。

## 2.4 Capability 驱动 UI

禁止为每个模型单独硬编码界面。

模型必须声明：

```
支持哪些能力
支持哪些输入
支持哪些参数
参数限制是什么
参数之间有什么互斥关系
```

前端根据声明自动生成 UI。

## 2.5 Canonical Request + Adapter

应用内部使用统一的数据结构。

例如：

```
ImageGenerationRequest
```

再由 Provider Adapter 转换成：

```
OpenAI Request
ComfyUI Workflow
Generic HTTP Request
```

## 2.6 永远保留逃生口

所有 Provider / Model 均应允许高级用户使用：

```
Extra JSON
Raw Request
Custom Headers
Generic HTTP
```

避免某个模型新增参数后必须等待软件更新。

# 3. 非目标

v1 不做：

- RAG 知识库
- 云端同步
- 多人协作
- AI 社区
- Prompt 市场
- 模型下载器
- 本地模型运行环境
- 工作流编排平台
- Photoshop 级图片编辑器
- Premiere 级视频编辑器
- 大型 Multi-Agent 系统

原则：

> 先把“调用模型”做好，而不是制造第二个 Dify。

# 4. 一级导航

左侧主导航：

```
＋ New

Text
Image
Video
Audio

────────

Agents
History

────────

Models
Settings
```

默认进入：

```
Text
```

# 5. Text

Text 分三种模式：

```
Generate
Chat
Agent
```

# 6. Text / Generate

用于无上下文单次模型调用。

界面：

```
Model

System Prompt

Prompt

Parameters

Generate
```

支持：

```
temperature
top_p
max_tokens
seed
stop

frequency_penalty
presence_penalty

reasoning_effort
response_format
```

生成完成后显示：

```
结果

Tokens
耗时
Provider
Model
```

支持：

```
复制
重新生成
保存
导出
查看原始请求
查看原始响应
```

# 7. Text / Chat

用于连续上下文对话。

数据结构：

```
Conversation
 ├─ System
 ├─ User
 ├─ Assistant
 ├─ User
 └─ Assistant
```

支持：

- 流式输出
- 编辑历史消息
- 删除消息
- 从某条消息重新生成
- 切换模型继续聊天
- 上下文 Token 显示
- 对话标题
- 自动生成标题
- 搜索历史

上下文策略：

```
All
Recent N
Token Limit
Summary
```

# 8. Agent

Agent 是 Chat 的增强模式。

Agent 包含：

```
Name

Model

System Prompt

Tools

Memory

Context Strategy

Max Steps

Parameters
```

v1 Agent 重点：

```
上下文
System Prompt
Tool Calling
循环执行
最大执行次数
工具调用日志
```

暂不实现复杂 Multi-Agent。

# 9. Agent 执行模型

基本循环：

```
User
 ↓
LLM
 ↓
需要 Tool？
 ├─ No → 返回
 └─ Yes
      ↓
   执行 Tool
      ↓
   Tool Result
      ↓
      LLM
```

需要防止无限循环。

参数：

```
max_steps = 10
```

达到上限后停止并提示。

# 10. Agent Tools

第一版内置少量 Tool：

```
HTTP Request
Current Time
Read File
Write File
List Directory
```

后续加入：

```
MCP
Web Search
Shell
Browser
```

MCP 不属于 MVP 强制功能。

# 11. Image

Image 支持：

```
Text to Image

Image to Image

Image Edit

Inpainting

Reference Image
```

模型根据 Capability 自动决定显示哪些模式。

# 12. Image 工作区

建议布局：

```
┌────────────────────────────────┐
│ Model                     ▼    │
├───────────────────────┬────────┤
│                       │参数    │
│ Prompt                │        │
│                       │Size    │
│ References            │Ratio   │
│                       │Seed    │
│ Output                │Steps   │
│                       │CFG     │
│                       │...     │
└───────────────────────┴────────┘
```

# 13. Image 输入

统一支持：

```
Prompt

Negative Prompt

Source Image

Reference Images[]

Mask
```

Reference Images 必须设计成数组。

不得设计成：

```
reference_image
```

必须：

```
reference_images[]
```

因为未来模型普遍会支持多参考图。

# 14. Reference Image

每张参考图可以拥有：

```
Role
Weight
Description
```

Role：

```
General
Character
Style
Pose
Composition
Structure
Control
```

示例：

```
Ref 1

Role:
Character

Weight:
0.8
```

如果 Provider 不支持 Role，则 Adapter 忽略。

# 15. Image 参数

通用参数：

```
width
height
aspect_ratio

quality
resolution

seed
batch_size

output_format
```

扩散模型：

```
steps
cfg_scale
sampler
scheduler

denoise_strength
guidance
```

参考控制：

```
reference_strength
style_strength
```

高级：

```
background
compression
safety
watermark
```

# 16. Image Mask

v1 提供轻量 Mask 编辑。

只需要：

```
Brush
Erase
Clear
Invert
```

禁止第一版开发复杂 Canvas 编辑器。

# 17. Video

Video 支持：

```
Text to Video

Image to Video

First Frame → Video

First + Last Frame

Reference → Video
```

# 18. Video Input

统一内部结构：

```
interface VideoInput {
  prompt: string

  negativePrompt?: string

  firstFrame?: Asset
  lastFrame?: Asset

  referenceImages?: Asset[]
  referenceVideos?: Asset[]
  referenceAudios?: Asset[]

  sourceVideo?: Asset
}
```

# 19. Video 参数

标准参数：

```
duration

width
height

resolution
aspect_ratio

fps

seed

quality

motion_strength
camera_motion
```

高级：

```
guidance
steps

style
creativity

audio
loop

watermark
```

不同模型只显示自己支持的参数。

# 20. Video Reference

必须原生支持：

```
referenceImages[]

referenceVideos[]

referenceAudios[]
```

每个模型可以声明数量限制。

例如：

```
{
  "referenceImages": {
    "enabled": true,
    "max": 9
  }
}
```

UI 自动限制上传数量。

# 21. First / Last Frame

模型能力：

```
supportsFirstFrame
supportsLastFrame
```

如果：

```
Last Frame = false
```

则 UI 不显示尾帧。

# 22. 参数约束

Model Schema 必须支持 Constraint。

例如：

```
首尾帧
```

和：

```
Reference Video
```

某模型不允许同时存在。

Schema：

```
{
  "constraints": [
    {
      "type": "mutuallyExclusive",
      "fields": [
        "firstFrame",
        "referenceVideos"
      ]
    }
  ]
}
```

# 23. Audio

Audio 分为：

```
Text to Speech

Speech to Text

Speech to Speech

Voice Clone
```

# 24. TTS

参数：

```
Text

Voice

Language

Speed

Pitch

Volume

Emotion

Style

Format

Sample Rate
```

# 25. Reference Audio

必须支持：

```
Reference Audio

Reference Transcript

Target Text
```

用于：

```
Zero-shot TTS
Voice Clone
Style Transfer
```

# 26. Voice Library

建立本地 Voice Library。

例如：

```
Yuna

reference.wav

reference transcript

tags
```

以后生成时：

```
Voice
Yuna ▼
```

即可调用。

# 27. Speech to Text

标准：

```
Audio

Language

Prompt

Timestamp

Speaker Detection
```

输出：

```
Plain Text

Subtitle

JSON
```

后期支持：

```
SRT
VTT
```

# 28. Provider

Provider 类型第一版支持：

```
OpenAI Compatible

OpenAI

Ollama

ComfyUI

Generic HTTP
```

后续：

```
Anthropic

Gemini

Replicate

fal.ai
```

# 29. Provider 数据模型

```
interface Provider {
  id: string

  name: string

  type:
    | "openai"
    | "openai-compatible"
    | "ollama"
    | "comfyui"
    | "generic-http"

  baseUrl: string

  credentialId?: string

  headers?: Record<string, string>

  enabled: boolean
}
```

# 30. Provider 设置

界面：

```
Provider Name

Provider Type

Base URL

API Key

Custom Headers

Timeout

Proxy

Test Connection
```

Test Connection 显示：

```
Connected

Latency
92ms
```

或具体错误。

# 31. Model

Model 不绑定 UI。

统一定义：

```
interface ModelDefinition {
  id: string

  name: string

  providerId: string

  modelId: string

  modality:
    | "text"
    | "image"
    | "video"
    | "audio"

  capabilities: CapabilityDefinition

  parameters: ParameterDefinition[]

  constraints?: ConstraintDefinition[]

  transport?: TransportDefinition
}
```

# 32. Capability

例如 Text：

```
{
  "streaming": true,
  "vision": true,
  "tools": true,
  "reasoning": true
}
```

Image：

```
{
  "generate": true,
  "imageToImage": true,
  "edit": true,
  "mask": true,
  "referenceImages": true
}
```

Video：

```
{
  "textToVideo": true,
  "imageToVideo": true,
  "firstFrame": true,
  "lastFrame": true,
  "referenceImages": true,
  "referenceVideos": false,
  "referenceAudio": true
}
```

# 33. 参数 Schema

核心：

```
interface ParameterDefinition {
  key: string

  label: string

  type:
    | "boolean"
    | "number"
    | "range"
    | "select"
    | "text"
    | "textarea"
    | "image"
    | "images"
    | "video"
    | "videos"
    | "audio"
    | "audios"

  default?: unknown

  min?: number
  max?: number
  step?: number

  options?: Array<{
    label: string
    value: string | number
  }>

  required?: boolean

  advanced?: boolean
}
```

# 34. 动态 UI

例如：

```
{
  "key": "steps",
  "label": "Steps",
  "type": "range",
  "min": 1,
  "max": 50,
  "default": 28
}
```

UI 自动渲染：

```
Steps

────────●──────

28
```

新增模型参数无需修改页面代码。

# 35. Preset

用户可以保存：

```
Model Preset
```

例如：

```
Flux · 高质量头像

1024×1024
Steps 28
CFG 3.5
Seed Random
```

下次直接选择。

# 36. Generic HTTP

这是关键高级功能。

允许用户配置任意 HTTP AI 服务。

包含：

```
Method

URL

Headers

Body Template

Response Parser
```

# 37. Body Template

例如：

```
{
  "model": "{{model}}",
  "prompt": "{{prompt}}",
  "seed": "{{seed}}"
}
```

运行时自动替换。

# 38. Response Parser

支持 JSONPath 风格配置：

```
$.data[0].url
```

或：

```
$.output.video
```

输出类型：

```
Text
Image URL
Video URL
Audio URL
JSON
```

# 39. Raw Request

所有生成页面增加：

```
Advanced

Raw Request
```

显示最终请求。

用户允许追加：

```
{
  "some_new_parameter": true
}
```

Adapter 最终：

```
Standard Parameters
+
Extra JSON
```

# 40. Request Inspector

每次请求支持查看：

```
Request

Response

Headers

Status

Latency

Task ID

Token Usage

Estimated Cost
```

并支持：

```
Copy JSON

Copy cURL
```

API Key 必须自动脱敏。

# 41. ComfyUI

ComfyUI 作为：

> 本地图像 / 视频模型通用运行后端。

Provider：

```
Name
ComfyUI

Base URL
http://127.0.0.1:8188
```

# 42. ComfyUI Workflow

支持：

```
Import Workflow JSON
```

用户给 Workflow 定义输入映射。

例如：

```
Prompt
→ node 6 / text

Seed
→ node 3 / seed

Image
→ node 14 / image
```

应用修改 JSON 后提交给 ComfyUI。

# 43. Workflow Preset

保存：

```
Flux Dev

Wan I2V

Hunyuan Video
```

为本地“模型”。

用户无需看到复杂节点。

# 44. 本地服务发现

Settings：

```
Local Services
```

扫描：

```
localhost:11434
localhost:1234
localhost:8188
localhost:8000
```

识别：

```
Ollama

LM Studio

ComfyUI

OpenAI Compatible
```

显示：

```
Ollama

localhost:11434

● Online
```

允许：

```
Add Provider
```

# 45. 统一任务系统

定义：

```
interface GenerationTask {
  id: string

  modality:
    | "text"
    | "image"
    | "video"
    | "audio"

  providerId: string

  modelId: string

  status:
    | "queued"
    | "running"
    | "completed"
    | "failed"
    | "cancelled"

  progress?: number

  createdAt: string

  completedAt?: string

  error?: string

  outputAssets: Asset[]
}
```

# 46. 异步 API

视频 API 常见：

```
POST
↓
Task ID
↓
Poll
↓
Processing
↓
Completed
```

Adapter 应统一转换为：

```
GenerationTask
```

前端完全不需要知道厂商差异。

# 47. Assets

所有媒体统一定义：

```
interface Asset {
  id: string

  type:
    | "image"
    | "video"
    | "audio"
    | "file"

  path?: string

  url?: string

  mimeType?: string

  width?: number

  height?: number

  duration?: number

  metadata?: Record<string, unknown>
}
```

# 48. 文件存储

默认：

```
AppData/

AIWorkbench/

data/

images/

videos/

audio/

files/
```

数据库只存：

```
path
metadata
```

不要把大型二进制文件放入 SQLite。

# 49. History

所有生成进入 History。

筛选：

```
All

Text

Image

Video

Audio
```

支持：

```
Provider

Model

Date

Status
```

# 50. History 详情

保存：

```
Prompt

Input Assets

Parameters

Model

Provider

Raw Request

Output

Created At
```

支持：

```
Run Again
```

直接恢复完整生成状态。

# 51. SQLite

数据库：

```
app.db
```

# 52. 数据表

## providers

```
id
name
type
base_url
credential_id
headers_json
enabled
created_at
updated_at
```

## models

```
id

provider_id

name

model_id

modality

capabilities_json

parameters_json

constraints_json

transport_json

enabled

created_at

updated_at
```

## presets

```
id

name

model_id

parameters_json

created_at

updated_at
```

## conversations

```
id

title

mode

model_id

system_prompt

created_at

updated_at
```

## messages

```
id

conversation_id

role

content_json

token_count

created_at
```

## agents

```
id

name

model_id

system_prompt

tools_json

memory_json

settings_json

created_at

updated_at
```

## generation_tasks

```
id

modality

provider_id

model_id

status

input_json

parameters_json

raw_request_json

raw_response_json

error

created_at

completed_at
```

## assets

```
id

task_id

type

path

url

mime_type

metadata_json

created_at
```

## voices

```
id

name

reference_audio_path

reference_text

metadata_json

created_at
```

# 53. Credential

禁止：

```
API Key
```

明文写入 SQLite。

使用：

```
OS Credential Storage
```

或：

```
Tauri Stronghold
```

SQLite 只存：

```
credential_id
```

# 54. 请求日志安全

日志自动处理：

```
Authorization: Bearer sk-123456
```

转换：

```
Authorization: Bearer sk-••••••
```

URL 中的敏感参数也必须脱敏。

# 55. 技术栈

桌面：

```
Tauri 2
```

前端：

```
React

TypeScript

Vite
```

状态：

```
Zustand
```

数据请求：

```
TanStack Query
```

UI：

建议自行设计基础组件。

可以使用：

```
Radix primitives
```

但不要套默认模板味明显的后台 UI。

# 56. Rust Core

Rust：

```
reqwest

tokio

serde

serde_json

sqlx
```

职责：

```
HTTP

SSE

WebSocket

SQLite

文件

下载

任务轮询

Credential

Provider Adapter
```

# 57. 前后端职责

React：

```
UI

State

Editor

Parameter Renderer
```

Rust：

```
Network

Storage

Security

Adapter

Task

Asset
```

禁止 API Key 长期保存在 React 状态。

# 58. 项目目录

建议：

```
src/

components/

features/

  text/

  image/

  video/

  audio/

  agents/

  history/

  models/

components/

  parameters/

  assets/

  common/

stores/

types/

lib/
```

Tauri：

```
src-tauri/

src/

  commands/

  providers/

    openai/

    openai_compatible/

    ollama/

    comfyui/

    generic_http/

  adapters/

  tasks/

  database/

  assets/

  credentials/

  services/

  models/

  errors/

migrations/
```

# 59. Provider Trait

Rust：

```
trait ProviderAdapter {

    async fn test_connection(...);

    async fn list_models(...);

    async fn execute(...);

    async fn cancel(...);

}
```

根据 Request 类型分发。

# 60. Canonical Requests

内部建立：

```
TextRequest

ImageRequest

VideoRequest

AudioRequest
```

例如：

```
interface ImageRequest {

  prompt: string

  negativePrompt?: string

  sourceImage?: Asset

  referenceImages?: Asset[]

  mask?: Asset

  parameters: Record<string, unknown>

  extra?: Record<string, unknown>
}
```

# 61. Adapter

流程：

```
Canonical Request

↓

Provider Adapter

↓

Provider Request

↓

HTTP

↓

Provider Response

↓

Canonical Response
```

UI 永远不直接处理厂商 API。

# 62. Model Registry

内置：

```
registry/
```

包含常见模型定义。

例如：

```
openai.json

ollama.json

comfyui.json
```

以后可以通过 GitHub 更新：

```
model-registry.json
```

无需更新 EXE 就能新增：

```
Model

Capabilities

Parameters
```

# 63. 自定义模型

Models：

```
Add Model
```

选择：

```
Provider

Model ID

Modality
```

然后配置：

```
Capabilities
```

和：

```
Parameters
```

# 64. Import / Export

支持导出：

```
Provider

Model

Preset

Agent
```

为 JSON。

敏感信息默认不导出。

例如：

```
API Key
```

自动排除。

# 65. 错误系统

错误分类：

```
AuthenticationError

NetworkError

TimeoutError

RateLimitError

InvalidRequestError

UnsupportedCapabilityError

ProviderError

TaskFailedError
```

UI 不要只显示：

```
Request failed.
```

应该：

```
429

请求频率超过限制。

Provider:
OpenRouter

Model:
xxx
```

高级：

```
Show Raw Error
```

# 66. Cancel

所有长任务必须支持：

```
Cancel
```

能真正取消的 Provider：

调用 Cancel API。

不能真正取消：

停止本地轮询并标记：

```
Detached
```

# 67. Proxy

Settings：

```
Network
```

支持：

```
System Proxy

No Proxy

Custom HTTP Proxy

Custom SOCKS5
```

Provider 可以：

```
Use Global

Override
```

# 68. UI 风格

整体：

```
浅色优先

高可读性

少边框

大间距

低信息噪声
```

不要：

```
大量卡片

满屏渐变

玻璃拟态

无意义 Dashboard

每个模块一堆统计数字
```

更接近：

```
IDE

API Playground

专业创作工具
```

而不是：

```
AI SaaS 首页
```

# 69. 参数面板

右侧参数默认只显示：

```
核心参数
```

例如：

```
Size

Ratio

Quality

Seed
```

下面：

```
Advanced
```

展开：

```
Steps

CFG

Sampler

Raw JSON
```

避免页面被几十个参数淹没。

# 70. Asset Input UX

图片：

```
拖入

粘贴

选择文件
```

音频：

```
拖入

选择
```

每个媒体支持：

```
Preview

Remove

Replace
```

# 71. 快捷操作

支持：

```
Ctrl + Enter
Generate

Ctrl + N
New

Ctrl + K
Model Switcher
```

# 72. 模型切换器

全局快速搜索：

```
GPT

Claude

Flux

Wan
```

搜索结果按：

```
Text

Image

Video

Audio
```

分类。

# 73. MVP

第一版只做以下功能。

## Text

```
OpenAI Compatible

Ollama

Generate

Chat

Streaming

Context
```

## Agent

```
System Prompt

Context

Tool Calling

Max Steps
```

## Image

```
OpenAI Image

OpenAI Compatible

ComfyUI

Text to Image

Image to Image

Reference Images

Mask

Dynamic Parameters
```

## Video

```
ComfyUI

Generic HTTP

Text to Video

Image to Video

First Frame

Last Frame

Reference Images
```

## Audio

```
OpenAI-compatible TTS

Generic HTTP

Reference Audio

Voice Library
```

## Core

```
Provider

Model

Capability

Dynamic Parameter

Preset

History

Task

Asset

Raw JSON

Request Inspector
```

# 74. MVP 不做

第一版明确禁止扩大：

```
RAG

Knowledge Base

Multi-Agent

云同步

账户体系

插件商城

Prompt 商店

自动安装模型

复杂 Canvas

视频编辑

音频编辑

完整 MCP 平台
```

# 75. 第二阶段

加入：

```
Anthropic

Gemini

Replicate

fal.ai

MCP

Speech to Text

Speech to Speech

Voice Clone

ComfyUI Workflow Mapping
```

# 76. 第三阶段

加入：

```
Model Registry 在线更新

Provider Plugin

Agent Memory

Context Compaction

模型成本统计

多模型 Compare

Batch Generation
```

# 77. 性能要求

目标：

```
冷启动 < 2 秒
```

普通页面：

```
< 100 ms
```

不要因为 History 数量增加导致明显卡顿。

历史使用：

```
SQLite pagination
```

Asset：

```
Lazy Load
```

# 78. EXE 目标

Windows：

```
Portable EXE
```

和：

```
Installer
```

两种发布形式。

优先保证：

```
Portable
```

真正免安装。

# 79. 自动更新

不是 MVP 强制功能。

后续加入：

```
GitHub Releases
```

检查：

```
New Version
```

用户手动确认后更新。

# 80. 验收标准

MVP 完成必须满足：

### Provider

可以创建：

```
OpenAI Compatible
Ollama
ComfyUI
Generic HTTP
```

并测试连接。

### Text

可以通过：

```
LM Studio
OpenRouter
Ollama
```

至少完成：

```
Generate

Chat

Streaming
```

### Agent

Agent 可以：

```
保持上下文

调用 Tool

读取 Tool Result

继续推理

达到 Max Steps 自动停止
```

### Image

模型 Schema 可以动态控制：

```
参考图

Mask

Ratio

Size

Seed

Steps
```

不需要为具体模型写页面。

### Video

能够：

```
Prompt

First Frame

Last Frame

Reference Images

参数

Submit

Polling

下载结果
```

### Audio

能够：

```
Text

Voice

Reference Audio

Generate
```

### ComfyUI

能够：

```
连接

提交 Workflow

监听任务

下载结果
```

### History

退出软件重新打开后：

```
Conversation

Generation

Provider

Model

Preset
```

全部仍然存在。

### Security

数据库中不得发现：

```
明文 API Key
```

# 81. 开发优先级

严格按照：

```
P0 Core

Provider
Model
Capability
Parameter Schema
SQLite
Task
Asset

↓

P1 Text

Generate
Chat
Streaming

↓

P2 Image

Dynamic Parameter
Reference
ComfyUI

↓

P3 Video

Async Task
Reference
Frames

↓

P4 Audio

TTS
Reference Audio

↓

P5 Agent

Tool Calling
Context
```

不要同时开发五个模块。

# 82. 第一阶段实际开发任务

建议 Codex 第一轮只做：

```
Tauri 2 初始化

React 基础布局

SQLite migration

Provider CRUD

Model CRUD

Credential Storage

Model Capability Schema

Dynamic Parameter Renderer
```

先证明：

```
一个 JSON Model Definition
```

能够自动生成：

```
Text Input

Select

Slider

Boolean

Image Upload

Multiple Images
```

这个能力一旦成立，项目最关键的技术风险就已经解决。

# 83. 第二轮

实现：

```
OpenAI Compatible Adapter

Text Generate

Streaming

Conversation

History
```

然后拿：

```
LM Studio

OpenRouter

DeepSeek API
```

实际测试。

# 84. 第三轮

实现：

```
ImageRequest

Image Capability

Image Dynamic UI

Reference Images

OpenAI Image Adapter

ComfyUI Adapter
```

# 85. 第四轮

实现：

```
GenerationTask

Async Polling

VideoRequest

First Frame

Last Frame

References

Generic HTTP
```

# 86. 第五轮

实现：

```
AudioRequest

TTS

Reference Audio

Voice Library
```

# 87. 第六轮

最后实现：

```
Agent

Tools

Max Steps

Tool Logs
```

原因是：

Agent 本质建立在 Text Core 已成熟之后。

# 88. 项目最重要的技术验证

第一版开发前优先验证三个 PoC。

## PoC 1

JSON：

```
Model Definition
```

能否动态生成完整参数面板。

## PoC 2

同一个：

```
ImageRequest
```

能否分别通过：

```
OpenAI Adapter

ComfyUI Adapter
```

成功生成图片。

## PoC 3

同一个：

```
GenerationTask
```

能否统一表示：

```
同步图片任务

异步视频任务
```

只要三个 PoC 成功，架构基本成立。

# 89. 核心设计底线

开发过程中如出现架构冲突，优先遵循：

```
Provider ≠ Model

UI ≠ Provider API

Model → Capability

Capability → Parameter UI

Request → Adapter

Adapter → Provider

Provider Response → Canonical Response
```

任何功能如果要求 UI 大量判断：

```
if model == xxx
```

应优先检查架构是否设计错误。

# 90. 最终产品形态

最终软件不是：

```
聊天机器人
```

也不是：

```
本地模型管理器
```

而是：

> 一个轻量、可配置、协议无关的 AI 模型工作台。

用户只需要：

```
添加 Provider
↓
添加 / 发现 Model
↓
软件识别模型能力
↓
自动生成操作界面
↓
调用
↓
查看结果
```

底层模型来自：

```
OpenAI
OpenRouter
DeepSeek
Gemini
Anthropic

LM Studio
Ollama
vLLM

ComfyUI

F5-TTS
CosyVoice

任意兼容 HTTP API
```

但对于用户而言，它们全部存在于同一个简单的桌面应用里。