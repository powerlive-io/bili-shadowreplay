---
title: "LLM 配置"
---


![LLM](/images/model_config.png)

使用 AI 助手前，请在「设置 → AI 模型」中选择提供商、填写服务地址和模型名。可以连接兼容 OpenAI 协议的服务，也可以使用本地 Ollama 模型。

BSR 不提供大模型服务。使用按量计费的服务时，请留意 Token 余额：助手调用工具时可能需要多轮对话，比普通对话消耗更多 Token。

要让助手调用工具，所选模型必须支持 Function Calling（函数调用）。
