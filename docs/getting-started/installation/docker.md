---
title: "Docker 部署"
---


BiliBili ShadowReplay 可以部署在没有图形界面的服务器上，通过浏览器中的 Web 控制界面使用。

官方镜像支持 `linux/amd64` 和 `linux/arm64`。Docker 会根据宿主机架构自动
选择对应镜像；在 Android Termux 中运行请参考 [Termux 部署](./termux)。

镜像的构建和运行环境统一使用 Debian trixie，以提供原生弹幕渲染所需的系统 FreeType、HarfBuzz 和 ICU。容器自带这些用户态依赖及匹配的 FFmpeg 共享库，正常运行不需要 `ffmpeg` 或 `ffprobe` 可执行文件，也不要求宿主机安装 Ubuntu 24.04 桌面运行库。自行修改镜像时，请保持 builder 与 runtime 的发行版基线一致，详见 [原生弹幕渲染构建](../../development/native-overlay.md)。

## 镜像获取

```bash
# 拉取最新版本
docker pull ghcr.io/powerlive-io/bili-shadowreplay:latest
# 拉取指定版本
docker pull ghcr.io/powerlive-io/bili-shadowreplay:2.5.0
# 速度太慢？从镜像源拉取
docker pull ghcr.nju.edu.cn/powerlive-io/bili-shadowreplay:latest
```

## 镜像使用

使用方法：

```bash
sudo docker run -it -d\
    -p 3000:3000 \
    -v $DATA_DIR:/app/data \
    -v $CACHE_DIR:/app/cache \
    -v $OUTPUT_DIR:/app/output \
    -v $WHISPER_MODEL:/app/whisper_model.bin \
    --name bili-shadowreplay \
    ghcr.io/powerlive-io/bili-shadowreplay:latest
```

其中：

- `$DATA_DIR`：应用数据目录，对应桌面版的以下位置：

  Windows 下位于 `C:\Users\{用户名}\AppData\Roaming\cn.vjoi.bilishadowreplay`;

  MacOS 下位于 `/Users/{user}/Library/Application Support/cn.vjoi.bilishadowreplay`

- `$CACHE_DIR`：为默认本地录制存储目录。保留 `/app/cache` 挂载位置以兼容旧录制；backend 配置和每场录播的绑定保存在 `$DATA_DIR` 中的数据库，通过设置页管理，不再以 TOML 的 `storage.backends.default.root` 为运行时来源。请同时保留数据库卷和录制文件卷；上传暂存和预览缓存改用容器的系统临时目录，不使用这个持久卷；
- `$OUTPUT_DIR`：素材输出目录，对应桌面版的输出目录；
- `$WHISPER_MODEL`：本地 Whisper 模型文件的路径。

## 网页设置

首次打开 `http://127.0.0.1:3000` 时会显示连接页。在「API 服务器地址」填写浏览器可访问的 BSR 地址，或点击「使用当前站点地址」，然后点击「连接并进入」。如果映射了其他端口或使用反向代理，请填写对应的外部地址，不要附加 `/api`。

连接成功后，当前浏览器会记住地址。进入「设置」即可管理录制存储、MCP、AI 模型、字幕、弹幕样式和自动素材生成；也可以更换服务器，或清除地址回到连接页。清除地址不会删除服务端数据。

网页中的文件路径是**服务端路径**，不是浏览器所在机器的路径。例如，本地 Whisper 应填写模型挂载到容器后的路径 `/app/whisper_model.bin`；素材保存位置则通过挂载 `/app/output` 指定。Docker 不发送桌面系统通知，可配置 Webhook 接收事件；查看日志使用 `docker logs <容器名>`。开机启动由 Docker 重启策略管理，PowerLive 账号登录仍需桌面版。
