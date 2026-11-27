---
title: "Whisper 配置"
---


要用 Whisper 生成字幕，请先在设置页选择本地模型或在线 Whisper 服务。在线服务通常需要付费获取 API Key。

> [!NOTE]
> 有些服务的中文识别效果更好，但通常需要先把文件上传到对象存储，再异步获取结果。BSR 采用本地或在线 Whisper，可以直接从请求响应中取得字幕，无需额外实现上传和轮询流程。

## 本地运行 Whisper 模型

![WhisperLocal](/images/whisper_local.png)

本地识别通过 whisper.cpp 运行 Whisper 模型。首次使用前，请通过设置页提供的下载入口取得兼容的模型文件，再设置「Whisper 模型路径」。

### 可用模型

| 模型名 | 大小 | 语言 | 说明 |
|--------|------|------|------|
| `tiny` | ~75MB | 多语言 | 速度最快，精度较低 |
| `tiny.en` | ~75MB | 仅英文 | 英文优化版本 |
| `base` | ~145MB | 多语言 | 速度与精度平衡 |
| `small` | ~465MB | 多语言 | 较高质量 |
| `large-v3` | ~2.9GB | 多语言 | 最高精度，速度最慢 |

模型文件不必放在固定的 `models/` 目录。桌面端可点击「变更」选择文件；网页控制端需要填写服务端能读取的路径。使用 Docker 时，这里填写的是容器内的路径。

### 硬件加速

- **macOS**：构建时默认启用 Metal，实际能否使用 GPU 加速取决于设备。
- **Windows / Linux**: 默认使用 CPU，启用 CUDA 后可支持 NVIDIA GPU 加速

## 使用在线 Whisper 服务

![WhisperOnline](/images/whisper_online.png)

在设置中切换到在线 Whisper，并填写服务提供商的 API Key。除 OpenAI 外，也可以选择提供 Whisper 服务的其他云平台。

## 字幕识别质量的调优

目前在设置中支持设置 Whisper 语言和 Whisper 提示词，这些设置对于本地和在线的 Whisper 服务都有效。

通常情况下，`auto` 语言选项能够自动识别语音语言，并生成相应语言的字幕。如果需要生成其他语言的字幕，或是生成的字幕语言不匹配，可以手动配置指定的语言。
根据 OpenAI 官方文档中对于 `language` 参数的描述，目前支持的语言包括

Afrikaans, Arabic, Armenian, Azerbaijani, Belarusian, Bosnian, Bulgarian,
Catalan, Chinese, Croatian, Czech, Danish, Dutch, English, Estonian, Finnish,
French, Galician, German, Greek, Hebrew, Hindi, Hungarian, Icelandic,
Indonesian, Italian, Japanese, Kannada, Kazakh, Korean, Latvian, Lithuanian,
Macedonian, Malay, Marathi, Maori, Nepali, Norwegian, Persian, Polish,
Portuguese, Romanian, Russian, Serbian, Slovak, Slovenian, Spanish, Swahili,
Swedish, Tagalog, Tamil, Thai, Turkish, Ukrainian, Urdu, Vietnamese, and Welsh.

Whisper 不适合执行复杂指令。可以用简短的提示词描述直播主题，引导模型选用相关领域的词汇；也可以用示例展示希望保留的标点风格。提示词主要影响字幕风格，也可能影响识别质量。
