---
title: "本地运行测试"
description: "准备依赖、选择测试命令，并排查常见环境问题。"
---

除代码块另有说明外，以下命令均在仓库根目录执行。

## 准备环境

### Node 与 Yarn

安装项目所需的 Node.js 和 Yarn，然后安装锁定的依赖：

```bash
yarn install --frozen-lockfile
```

文档站依赖独立安装，相关命令见[持续集成与文档检查](ci.md)。

### Rust 与原生依赖

通过 rustup 安装 Rust。仓库的 `rust-toolchain.toml` 会选择固定工具链，并包含 rustfmt 和 Clippy，不需要修改全局默认版本：

```bash
rustc --version
cargo --version
git submodule update --init --recursive
yarn setup:kache
```

仓库 Cargo 配置默认使用 kache；缺少它会在编译前报错。完整构建还需要 Tauri、FFmpeg、Skia 和相关语音库依赖：

- [FFmpeg 源码构建配置](../../getting-started/config/ffmpeg.md#从源码构建)：各系统的 SDK、动态库及可选 CLI 配置。
- [原生渲染构建](../native-overlay.md)：Skia、Vulkan 和平台工具链要求。
- [平台与实机验收](platform-validation.md)：Windows 直接运行 Cargo 的准备命令，以及 macOS/Windows 自动启动流程的验收。

FFmpeg 开发头文件和共享库必须匹配目标架构。不要仅安装 `ffmpeg` 可执行文件，也不要把机器专属的 SDK 路径写入共享 Cargo 配置。直接运行 Cargo 不会经过 `yarn tauri` 的自动环境准备。

## 前端与脚本测试

```bash
# 运行 scripts/tests/ 下全部 *.test.mjs
yarn test

# 按改动范围运行已有的测试组
yarn test:agent
yarn test:state-refresh
yarn test:live-preview
yarn test:danmu-content
yarn test:updater
yarn test:subtitle

# Svelte 与 TypeScript 静态检查，不是运行时测试
yarn check
```

这些测试通过 Node 测试运行器执行；部分平台专属用例会在其他系统上跳过。

## Rust 单元与集成测试

以下命令先切换到 `src-tauri`，后续命令都在该目录执行：

```bash
cd src-tauri

# 工作区库测试；排除 whisper-cpp-rs 自身的测试目标
cargo test --workspace --lib --exclude whisper-cpp-rs

# recorder 包的测试
cargo test --package recorder

# 只运行 recorder 的全部集成测试目标
cargo test --package recorder --test '*'

# 指定集成测试文件
cargo test --package recorder --test integration_test

# 显示测试中的输出
cargo test --package recorder -- --nocapture

# 按名称筛选单个测试
cargo test --package recorder test_platform_type_string_conversion
```

`--exclude whisper-cpp-rs` 只排除工作区测试目标，不会阻止其他包把它作为依赖编译，因此不能替代子模块初始化和 CMake 安装。

## 原生媒体与应用测试

在仓库根目录执行：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --package ffmpeg_utils
cargo test --manifest-path src-tauri/Cargo.toml --package recorder --lib core::hls_recorder::tests
cargo test --manifest-path src-tauri/Cargo.toml --package bili-shadowreplay ffmpeg::audio
cargo test --manifest-path src-tauri/Cargo.toml --package bili-shadowreplay material::tests
```

应用二进制目标中的测试不包含在 `--workspace --lib` 中。按需运行默认配置或无界面配置：

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features headless
```

媒体验证的比较规则与覆盖范围见[音频回归测试](audio.md)和[视频与录制回归测试](video-recording.md)。真实直播和硬件测试按需选择具体名称，不要无差别运行所有忽略用例。

## 常见问题

### 找不到 OpenSSL 或 pkg-config

Ubuntu/Debian 可安装：

```bash
sudo apt-get install -y libssl-dev pkg-config
```

如果已安装，检查 `PKG_CONFIG_PATH` 是否包含对应架构的 `.pc` 文件目录。`/usr/lib/x86_64-linux-gnu/pkgconfig` 只适用于相应的 Linux x86_64 环境，不应照搬到 macOS、Windows 或其他架构。

### 找不到 whisper.cpp 的 CMakeLists.txt

在仓库根目录初始化子模块，并确认已安装 CMake：

```bash
git submodule update --init --recursive
```

排除 `whisper-cpp-rs` 测试目标不保证跳过其依赖构建，见前面的说明。

### macOS 链接器提示 unknown architecture arm64e.x1-macos

检查当前 SDK 与链接器是否兼容。若本机确实安装了兼容的旧 SDK，可临时指定，例如：

```bash
export SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.sdk
```

路径仅为示例，应以本机已安装的 SDK 为准，不要将此设置提交到仓库。

### 测试超时

- 异步测试应使用 `#[tokio::test]` 或显式管理运行时，避免在缺少运行时的情况下等待异步任务。
- 本地 HTTP 测试需要允许监听回环地址，不能把监听失败当作外部平台故障。
- 检查是否误选了真实网络或设备测试，以及测试是否为阻塞 I/O 配置了超时和取消机制。
