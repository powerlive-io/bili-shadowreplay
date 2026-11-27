---
title: "平台与实机验收"
description: "Windows、macOS、GPU、安装包与真实直播间的手动验证清单。"
---

本页的硬件、真实网络和交互式验收不能由普通单元测试替代。除另有说明外，命令在仓库根目录执行。实机测试应使用独立应用数据，避免触碰现有录像和账号。

## Windows 原生媒体验证

直接运行 Cargo 时，使用 x64 MSVC 开发者 PowerShell 和项目固定的 Gyan FFmpeg 9.0.2 shared SDK。准备步骤与 Cargo 必须在同一个终端执行：

```powershell
& .\ffmpeg_setup.ps1
rustup component add llvm-tools
$skia = node scripts/prepare-windows-skia.mjs
$env:SKIA_BINARIES_URL = ($skia | Where-Object { $_ -like 'SKIA_BINARIES_URL=*' }) -replace '^SKIA_BINARIES_URL=', ''
cargo test --manifest-path src-tauri/Cargo.toml -p ffmpeg_utils --locked --lib
cargo test --manifest-path src-tauri/Cargo.toml -p bili-shadowreplay --locked ffmpeg:: -- --include-ignored --test-threads=1 --skip native_gpu_
cargo test --manifest-path src-tauri/Cargo.toml -p recorder --locked
```

`ffmpeg_setup.ps1` 为当前进程设置 `FFMPEG_DIR` 和 SDK DLL／可选 CLI 搜索路径，并在找到 LLVM 时定位 `LIBCLANG_PATH`；未找到时应显式设置为 LLVM 的 `bin` 目录。

Skia 准备脚本输出的 URL 必须保留 `{key}` 占位符。Windows 应用（包括无界面构建）默认包含 Vulkan 后端；准备脚本会选择 Vulkan 包。如果此前准备的是 CPU 包，应重新运行。

### GPU 与 GOP

在支持 Vulkan Video 的设备上运行实际生产 GPU 导出测试，无需额外启用 GPU 特性：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked --no-default-features --features headless native_gpu_ -- --ignored --nocapture --test-threads=1
```

定向验证关键帧间隔：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml -p bili-shadowreplay --locked encoded_keyframe_gaps_respect_the_120_frame_gop -- --include-ignored
```

GOP 测试覆盖软件编码器，以及可用的 NVENC／VideoToolbox 编码器的两种优先级。CLI 和原生计划都必须保留 301 帧，第一关键帧位于零时刻，相邻关键帧间隔与末尾区间最多 120 帧，并至少出现一个恰好 120 帧的间隔。320×240 测试数据与原生探测支持的尺寸一致。

### 安装包运行库

1. 将核心测试可执行文件放在打包的 SDK DLL 旁。
2. 在临时测试终端中移除 SDK 环境变量，并从 `PATH` 移除 SDK／CLI 目录。
3. 确认原生媒体测试仍通过；缺少可执行文件时，可选 CLI 对照应跳过。
4. 确认打包的 DLL 与构建时使用的 SDK 匹配，并检查安装包中没有 FFmpeg CLI 可执行文件。
5. 补做交互式 GUI 冒烟测试，不能只凭包内测试通过就完成验收。

Linux 包的私有运行库、干净环境安装和动态依赖检查见 [FFmpeg 配置](../../getting-started/config/ffmpeg.md#linux-docker)。

## macOS 开发启动运行库

```bash
node --test scripts/tests/macos-runtime.test.mjs
```

自动化测试覆盖 SDK 显式配置、下载中断与进程退出后的续传、并发下载隔离、校验失败清理、运行库签名、FFmpeg 裸文件名加载、离线缓存复用及损坏缓存拒绝。

HTTP 测试通过本机临时服务验证 curl 的 Range 重试，以及不支持续传和 HTTP 416 时的恢复；不访问外网，但需要允许监听回环地址。动态库加载测试仅在 macOS 执行。

完整启动验收应在临时终端中清除手工 `FFMPEG_DIR`、`PKG_CONFIG_PATH`、`BSR_VULKAN_LIBRARY`、`BSR_MOLTENVK_LIBRARY` 和 `DYLD_*` 配置，通过 `yarn tauri dev` 确认自动准备，并执行 GPU 导出探测。不要用此前测试目录里的运行库代替首次准备流程。

## Windows 开发启动依赖

```powershell
node --test scripts/tests/windows-runtime.test.mjs scripts/tests/prepare-windows-skia.test.mjs
```

自动化测试覆盖环境变量大小写、开发者环境解析、自定义 FFmpeg SDK、DLL 部署与重复启动、SDK 缺失文件拒绝、Skia 缓存版本及校验。PowerShell 集成测试仅在 Windows 执行，不联网。

实机验收要验证启动器能否自行准备环境，不同于前面的直接 Cargo 验证。请使用**普通终端**，清除手动设置的 `FFMPEG_DIR`、`SKIA_BINARIES_URL`、`LIBCLANG_PATH` 和 VS 开发环境，再依次验证首次准备、缓存复用、`yarn tauri dev` 构建和原生 GPU 导出探测。

## 真实直播间冒烟测试

真实平台测试受直播间在线状态、CDN、网络和反爬策略影响，默认标记为 `#[ignore]`，CI 使用本地数据或解析测试替代。已知用例包括：

- 虎牙：`platforms::huya::api::tests::test_get_room_info_live`、`test_get_user_info`。
- 抖音：`platforms::douyin::api::tests::test_get_room_owner_sec_uid_live`。

按完整名称选择所需用例，再添加 `-- --ignored --nocapture`，不要直接执行全部忽略测试。

### 斗鱼五分钟连续录制

需要匹配的原生媒体库、外网连接，以及**当前正在直播**的数字房间号。在 POSIX 终端运行：

```bash
cd src-tauri
DOUYU_TEST_ROOM_ID=1667826 cargo test -p recorder live_room_records_continuously_for_five_minutes --lib -- --ignored --nocapture
```

该测试使用生产代码中的 `DouyuRecorder` 循环录制五分钟。预期只产生一个归档，每分钟新增的 HLS 分片总时长约为一分钟；测试结束后删除临时录制。如果示例房间已下播，请换成正在直播的房间号。
