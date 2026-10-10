# ModelDeck 生产版本可复现发布清单 (Release Checklist)

> 本文档规范 ModelDeck 桌面客户端（Windows x64）从版本冻结、门禁验证、打包到正式发布的全生命周期操作规范，确保构建过程 100% 可复现、产物可校验、旧版可平滑迁移与回滚。

---

## 阶段一：版本号一致性与分支冻结 (Version & Branch Freeze)

- [ ] **分支状态检查**：确认当前分支干净，无未暂存或未提交的修改 (`git status`)。
- [ ] **多文件版本号强一致性校验**：
  运行统一校验脚本，确保各处声明完全对齐：
  ```bash
  npm run check:version
  ```
  核对下列三处版本号严格一致：
  - `package.json` -> `"version"`
  - `src-tauri/Cargo.toml` -> `[package]` 区块下的 `version`
  - `src-tauri/tauri.conf.json` -> `"version"`
- [ ] **Release Tag 预核验**：
  若拟发布版本为 `v1.0.1`，验证预期标签与当前代码版本号一致：
  ```bash
  node scripts/check-version-consistency.cjs --expected 1.0.1
  ```
  *注：严禁发布 Tag 与内部版本号不一致的构建产物。*

---

## 阶段二：自动化质量门禁验证 (Quality Gates)

在全新拉取或执行发布前，必须在本地依次执行下列命令，**所有门禁项必须 100% 绿灯通过，出现任何警告或失败即阻断发布**：

```bash
# 1. 前端类型检查 (TypeScript)
npm run typecheck

# 2. 前端静态代码规范检查 (Biome)
npm run lint

# 3. 前端单元测试 (Vitest)
npm run test

# 4. 前端生产包编译
npm run build

# 5. Rust 代码格式规范检查
cargo fmt --check --manifest-path src-tauri/Cargo.toml

# 6. Rust Clippy 严格静态检查 (零警告门禁)
cargo clippy --all-targets --manifest-path src-tauri/Cargo.toml -- -D warnings

# 7. Rust 全量单元测试、集成测试与契约测试
cargo test --manifest-path src-tauri/Cargo.toml
```

- [ ] TypeScript 编译 0 错误
- [ ] Biome Linter 0 错误与警告
- [ ] Vitest 22+ 项前端单元测试全部通过
- [ ] `cargo fmt` 格式化规范检查通过
- [ ] `cargo clippy --all-targets -- -D warnings` 零警告通过
- [ ] `cargo test` 161+ 项 Rust 单元与集成测试全部通过（包含版本号一致性测试、崩溃恢复状态机测试、Provider 契约测试、安全快照与凭据双轨迁移测试）

---

## 阶段三：数据一致性、迁移与凭据保护审查 (Data & Security)

- [ ] **SQLite 数据库迁移**：
  - 增量迁移脚本序列（`001` 至 `022`）按序执行无冲突；
  - 核心状态字段（`unknown_submission`, `billing_risk`, `retryable`, `remote_job_id`）完整落地。
- [ ] **安全快照与完整性校验**：
  - SQLite WAL 机制下备份前自动执行 `wal_checkpoint(TRUNCATE)` 并采用 `VACUUM INTO` 生成紧凑快照；
  - 备份文件通过 `PRAGMA integrity_check` 和 `PRAGMA foreign_key_check` 校验。
- [ ] **系统凭据平滑双轨兼容**：
  - 主服务名 `local.modeldeck` 与历史兼容服务名 `local.aiworkbench` 正常回退读取与透明迁移升级；
  - 保证旧版升级用户现有 API 凭据不丢失。
- [ ] **配置导出脱敏**：
  - 导出配置绝不包含 API Key / Bearer 密钥，请求头使用 `{{apiKey}}` 占位符替代，杜绝明文或截断前缀泄露。
- [ ] **资源与网络边界**：
  - Tauri `assetProtocol.scope` 白名单有效限制于应用与合法个人媒体目录；
  - Agent / MCP 工具执行受到工作区边界限制与 SSRF 白名单拦截。

---

## 阶段四：生产产物构建与校验和生成 (Production Packaging & Checksums)

- [ ] **执行 Tauri 生产构建**：
  ```bash
  # 纯便携二进制构建
  npm run tauri build -- --no-bundle
  
  # 或完整安装包构建（需系统已安装 WiX / NSIS 构建依赖）
  npm run tauri build
  ```
- [ ] **运行产物归档与哈希生成脚本**：
  ```bash
  node scripts/finalize-release.cjs
  ```
  该脚本会自动在 `dist/windows` 下生成：
  - `ModelDeck-Portable.exe` / `ModelDeck-${version}-Portable.exe`
  - `ModelDeck-Setup.exe` / `ModelDeck-${version}-Setup.exe` (NSIS)
  - `checksums.sha256`（标准 GNU 格式 SHA-256 校验清单）
  - `release-metadata.json`（包含产物大小、Git Commit、构建时间戳元数据）
- [ ] **手动或自动校验文件哈希**：
  ```powershell
  # PowerShell 校验示例
  Get-FileHash -Algorithm SHA256 "dist/windows/ModelDeck-Portable.exe"
  ```
  核对计算所得的哈希与 `checksums.sha256` 中的记录严格一致。

---

## 阶段五：真实 Windows 环境冒烟测试 (Smoke Testing)

运行自动化 Windows 发布环境验证套件：
```bash
node scripts/verify-windows-release.cjs
```

需验证通过的项目：
- [ ] **安装版 / 便携版文件存在性与大小合法**
- [ ] **中文与带空格路径启动**：Portable 位于类似 `D:\测试 目录 (Portable)\` 下能正常启动运行
- [ ] **移动目录适应性**：将 Portable 移动到其他任意目录后启动，能够正常重用已有的系统数据
- [ ] **只读属性目录运行**：EXE 所在目录设置为只读时，程序正常启动且不向 EXE 目录写入任何数据
- [ ] **系统运行时目录生成**：在 `%APPDATA%\local.modeldeck` 正常生成 `app.db` 以及 `images/videos/audio/files` 存储子目录
- [ ] **离线启动测试**：断开网络连接启动，界面能够正常进入并显示离线可用模型/功能，无白屏崩溃
- [ ] **WebView2 依赖检测**：确认具备 Microsoft Edge WebView2 Runtime

---

## 阶段六：预发布候选 (RC) 与正式打标规则 (Release Protocol)

1. **Release Candidate (RC) 预发布**：
   - 重要版本升级必须先制作候选版本标签（如 `v1.0.1-rc.1`）；
   - 在测试机上完成全功能链路冒烟确认无严重回归。
2. **正式打标 (Tagging)**：
   - 绝不强制覆盖已有正式版本 Tag（如既有的 `v1.0.0`）；
   - 打标命令：
     ```bash
     git tag -a v1.0.1 -m "Release v1.0.1: stabilization and bug fixes"
     git push origin v1.0.1
     ```
3. **Release Notes 必填要素**：
   - **本次变更亮点** (Highlights)
   - **Bug 修复清单** (Bug Fixes)
   - **升级与数据迁移指引** (Migration Guide，明确告知旧凭据与数据库无缝兼容)
   - **系统环境要求** (System Requirements: Windows 10/11 64-bit, WebView2)
   - **已知限制与未覆盖边界** (Known Limitations)
   - **完整二进制校验清单** (SHA-256 Checksums)

---

## 阶段七：故障应急与回滚预案 (Rollback Plan)

若正式发布后发现不可接受的 P0 级严重缺陷：
1. **立即停止分发**：在 GitHub Releases / 官网将该版本标记为 `Pre-release` 或在发布说明顶部置顶已知问题警告。
2. **数据降级说明**：
   - 因 SQLite 迁移保持向下兼容原则，用户若需回退至上一稳定版本，指引用户备份当前 `%APPDATA%\local.modeldeck\app.db`；
   - 凭据服务具备 `local.modeldeck` 与 `local.aiworkbench` 双轨兼容，回退版本不会损坏系统凭据。
3. **热修复机制**：检出上一个稳定 Tag，创建 `fix/hotfix-vX.Y.Z` 分支，严格遵循本清单重新走门禁测试后发布递增修复版本。
