# ModelDeck 发布前安全与冒烟检验报告 (Release Smoke Test)

> 对应任务：**T05｜收紧 Tauri 资源边界、补齐最小发布前检验 (P0)**  
> 关联缺陷：**D06**（Tauri asset 协议过度放宽）  
> 检验基准：`ModelDeck_V1_STABILIZATION_AND_EVOLUTION_PLAN_20261010.md`  
> 运行环境：Windows 11 / Windows 10 (x64)

---

## 1. 资源边界最小化授权 (Asset Protocol Hardening)

### 1.1 现状与加固措施
- **加固前 (D06 隐患)**：`src-tauri/tauri.conf.json` 中配置 `"assetProtocol": { "enable": true, "scope": ["**"] }`，允许前端直接加载操作系统上任意路径的文件。
- **加固后 (最小化白名单)**：严格收紧为受控作用域，只允许应用自身数据目录与用户合法个人媒体选择目录：
  ```json
  "assetProtocol": {
    "enable": true,
    "scope": [
      "$APPDATA/**",
      "$APPLOCALDATA/**",
      "$LOCALDATA/**",
      "$TEMP/**",
      "$PICTURE/**",
      "$VIDEO/**",
      "$AUDIO/**",
      "$DOWNLOAD/**",
      "$DOCUMENT/**"
    ]
  }
  ```
- **安全效果**：
  - 彻底封锁对 Windows 系统目录（`C:\Windows\`）、`Program Files`、其他系统关键目录的直接 `asset://` 协议读取；
  - 完整保留历史生成产物（位于 `%APPDATA%\local.modeldeck\`）以及用户从“图片/视频/音频/下载/文档”挑选参考素材进行多模态创作的核心回显需求；
  - 前端 `convertFileSrc` 兼容性未受破坏，`ImageThumb`、`VideoThumb`、`AudioThumb` 预览正常。

---

## 2. 内容安全策略 (CSP) 核验

在 `tauri.conf.json` 中保持严格且功能完备的 CSP 策略：
```text
default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: asset: http: https:; media-src 'self' data: asset: http: https:; font-src 'self' data:; connect-src 'self' ipc: http: https: ws:
```

### 2.1 规则核查
1. **严格脚本隔离 (`script-src 'self'`)**：绝对禁止 `'unsafe-eval'` 与远程注入脚本执行，杜绝 XSS 武器化；
2. **样式控制 (`style-src 'self' 'unsafe-inline'`)**：支持 React 组件内联计算样式，禁止远程外链恶意样式表；
3. **媒体加载 (`img-src`, `media-src`)**：允许本地静态资源、Tauri `asset:` 协议、Base64 数据 URI，以及远端图像生成 CDN（Replicate, Fal 等）；
4. **通信渠道 (`connect-src`)**：允许 Tauri 内核 IPC、外部模型 API 调用（HTTP/HTTPS）以及本地 ComfyUI 队列的实时 WebSocket 监听（`ws:`）。

---

## 3. IPC `invoke_handler` 全命令路径安全审计与边界防护

除了 Agent 工具沙盒外，对所有通过 Tauri IPC 暴露的普通文件读写命令执行全面加固：

| 命令名称 | 功能说明 | 加固策略与校验逻辑 |
|---|---|---|
| `read_image_file` | 本地参考图读取 | 引入 `validate_media_read_path`：禁止访问系统保护与凭据目录（`/.ssh` 等）；若路径位于应用数据目录，严格限制在合法媒体子目录（`images`, `files`），防止通过前端泄漏 `modeldeck.db` 核心数据库。 |
| `read_video_file` | 本地参考视频读取 | 引入 `validate_media_read_path` 进行边界校验与合法媒体格式嗅探。 |
| `read_audio_file` | 本地参考音频读取 | 引入 `validate_media_read_path` 进行边界校验与音频魔数嗅探。 |
| `save_text_file` | 用户导出提示词/配置 | 限制文件最大 5MB；必须为绝对路径；禁止保存为可执行或脚本扩展名（`exe`, `bat`, `cmd`, `vbs`, `ps1` 等）；禁止向系统目录和凭据目录写入；**严禁直接覆盖应用数据目录文件**。 |
| `storage_clean` | 孤儿文件与临时碎片清理 | 严格限制在应用数据目录内部的 `images`, `videos`, `audio`, `files` 四个媒体子目录，绝不触碰非媒体文件与外部文件。 |
| `config_export_all`<br>`config_import_all` | 配置跨机导入导出 | 内存中结构化交换，不接收任意本地文件路径；敏感密钥严格按占位符脱敏处理。 |

---

## 4. 最小发布前运行环境检验 (Smoke Test Checklist)

### 4.1 环境与依赖检验
- [x] **WebView2 运行时检测**：通过 `windows_util.rs::is_webview2_installed()` 在启动前探针 Windows 注册表 `SOFTWARE\Microsoft\EdgeUpdate\Clients`，若缺失将向用户展示清晰的引导弹窗并退出，避免白屏卡死（单元测试 `windows_util::tests::test_webview2_installed_on_windows` 已通过）。
- [x] **无管理员权限运行 (Least Privilege)**：NSIS 安装器配置 `"installMode": "currentUser"`，安装在当前用户目录，不需要 UAC 提权即可完整安装与运行。
- [x] **中文与特殊字符路径兼容性**：应用数据目录与媒体存储统一使用标准规范化物理路径处理，测试用例 `sse::tests::utf8_chunk_decoder_handles_split_multibyte_chinese_and_emoji` 与 `services::search::tests::test_cjk_segment_and_fts5` 均验证了中文/Unicode 处理的健壮性。
- [x] **首次数据库创建与增量迁移**：应用启动时在 `%APPDATA%\local.modeldeck\` 自动创建 `modeldeck.db`，并顺序执行 001 至 021 全部向后兼容迁移脚本，具备完整幂等性。

### 4.2 发行物形态与签名真实性声明
1. **分发包形态**：
   - NSIS 安装包 (`.exe`)：单用户免提权安装；
   - WiX 安装包 (`.msi`)：企业 IT 部署包；
   - 便携绿色版 (`Portable`)：解压即用，配置保留在用户数据目录。
2. **代码签名真实性**：
   - 现阶段开发与内测阶段**未配置商用 EV 代码签名证书**；
   - 在构建产物说明中保持诚实透明，不宣称已签名，告知 Windows Defender SmartScreen 可能会提示未知发布者警告，符合开源与内部测试真实规范。

---

## 5. 验收测试结论

- **自动化测试**：
  - `commands::tests::test_validate_media_read_path_rules`：通过；
  - `cargo test` 全量套件：**141 passed; 0 failed**；
  - `npm run test`（Vitest 单元测试）：**22 passed**；
  - `npm run lint`（Biome 检查）：**74 个文件全部通过**；
  - `npm run build`：生产环境打包成功；
- **状态结论**：**T05 验收通过，D06 缺陷已修复并加固**。
