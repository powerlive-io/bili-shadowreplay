---
title: "持续集成与文档检查"
description: "GitHub Actions 的测试职责、跳过边界和文档发布前检查。"
---

## 自动化检查范围

以 `.github/workflows/` 中的工作流为准，不以历史测试数量或本地某一次运行作为 CI 覆盖范围。

### Tests 工作流

`.github/workflows/test.yml` 在推送到 `main` 和向 `main` 提交 PR 时执行，主要包括：

| 任务 | 验证内容 |
| --- | --- |
| 前端测试 | `yarn test`，自动包含 `scripts/tests/*.test.mjs` |
| Linux FFmpeg 运行库 | 使用发布 SDK 与私有运行库布局，执行 `scripts/test-linux-ffmpeg-runtime.sh` |
| 录制目录 | `recording_catalog` 包测试及所有目标的 Clippy 检查 |
| Rust 测试 | 工作区库测试、recorder 测试，以及应用层录制目录与存储迁移测试 |

Rust 测试任务会检出子模块，准备固定 FFmpeg SDK、系统依赖和 sherpa-onnx，配置 Rust 与编译缓存，然后执行：

```bash
cd src-tauri
cargo test --workspace --lib --exclude whisper-cpp-rs
cargo test --package recorder --tests
cargo test --package bili-shadowreplay --no-default-features --features headless database::catalog --locked
cargo test --package bili-shadowreplay --no-default-features --features headless storage_migration_tests --locked
```

工作区库测试不包含应用二进制目标中的用例，因此目录迁移和存储迁移会单独运行。

### Lint 与提交前检查

`.github/workflows/lint.yml` 在 Rust 构建输入变化时运行默认／无界面两套 `cargo test`；推送到 `main` 时始终运行。它与 Tests 工作流互为补充。

本地 pre-commit 运行空白、文件结尾、Rust 格式和默认／无界面两套 Clippy 检查，**不运行测试**。提交前仍应按改动范围执行[本地测试](running.md)。

### 不应从 CI 通过推断的结果

- `#[ignore]` 用例默认不执行，真实直播间、专用硬件和其他显式选择的测试需要手动运行。
- 可选 CLI 对照可能因缺少工具而跳过；必须检查日志，不能只看退出码。
- CUDA、Whisper 模型、长时录制与交互式 GUI 行为不能假定已由常规 CI 完整覆盖。
- Linux 上通过不代表 Windows/macOS 启动和安装包已验收，见[平台与实机验收](platform-validation.md)。

## 文档站检查

文档站使用独立的依赖与锁文件，CI 使用 Node.js 22。在仓库根目录执行：

```bash
yarn --cwd docs install --frozen-lockfile
yarn docs:build
yarn docs:check
yarn --cwd docs test:export
```

- `docs:build`：构建首页资源和 Next.js 静态站点，产物位于 `docs/out/`。
- `docs:check`：生成文档类型并运行 TypeScript 检查。
- `test:export`：检查导出页面、站内链接与锚点、资源、旧测试入口、静态搜索，以及首页、视频、正文内嵌图与离线图表。

需要人工确认导航和排版时运行：

```bash
yarn docs:preview
```

`.github/workflows/pages.yml` 在文档或自身工作流变动的 PR 上构建并验证静态导出；合入 `main` 后部署。新增文档应填写中文标题和说明，并在对应目录的 `meta.json` 中安排导航顺序。
