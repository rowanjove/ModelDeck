# ModelDeck 1.0.1 发布核验

核验日期：2026-10-11（Asia/Hong_Kong）。本记录仅描述本轮实际执行结果；历史验收表不替代当前证据。

## 发布前修复

备份文件覆盖、重启后的永久排队、Provider 地址变更沿用旧密钥、工具确认参数与执行次数边界、旧安装器归档、版本锁文件同步。

保留当前工作树 release profile 的 `opt-level = 3`、`panic = "abort"`，未恢复原来的 LTO/strip 设置。首次完整发布编译出现 LLVM 内存不足；为完成构建，仅对主程序包增加 `opt-level = 1`、`codegen-units = 256` 的覆盖，依赖仍使用优化级别 3，构建并发限制为 1。该调整影响编译与性能优化，不改变功能配置；没有以性能基准证明与原优化级别的等价性。

## 验证

| 检查 | 结果 |
|---|---|
| 多配置与锁文件版本 | 1.0.1，一致 |
| `npm run typecheck` | 通过 |
| `npm run lint` | 81 文件，0 错误 |
| `npm test` | 40 项通过 |
| `npm run build` | 通过 |
| `cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check` | 通过 |
| `cargo clippy --locked --all-targets --manifest-path src-tauri/Cargo.toml -- -D warnings` | 通过 |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --quiet` | 177 项通过 |
| 独立风险终审 | SHIP，无 P0/P1 阻断 |
| 当前版本安装器缺失时的归档保护 | 按预期失败，未采用旧版安装器 |

执行环境：Windows 10 x64、Node.js 24.13.0、Rust/Cargo 1.93.1（MSVC）。打包与冒烟结果在完成后补充。

## 边界

- 协议与业务自动化测试基于模拟服务/测试数据库，不证明真实服务商生成成功。
- 打包冒烟使用 `MODELDECK_SMOKE_DATA_DIR` 指定独立的测试数据库和媒体目录，不接触用户现有数据库。
- 进程存活、目录生成与数据库检查不证明 GUI 渲染、安装向导操作或完整用户业务链。
- 只读 EXE 属性不等同于目录 ACL 禁止写入；断网冷启动尚未验证。
- 本轮不执行真实收费生成，不修改用户 Provider 或系统凭据。
