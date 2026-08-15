---
title: 分支指南
description: 在本地构建并安装此分支、了解运行时存储、并为 8GB GPU 选择翻译模型。
---

# 分支指南

本页面介绍 `Endymi0n74/Koharu` 分支：如何在没有代码签名的情况下安装本地构建、运行时存储的位置，以及如何为配备 8GB 显存的消费级 GPU（例如 RTX 3070）选择翻译模型。

## 此分支的改动

在原生 Koharu 之上，此分支目前增加了以下内容：

- **下载完整性验证** — 模型和运行时包的下载在发布到存储之前，会对照预期大小和 SHA-256 进行验证。
- **llama.cpp 日志透出** — 当 GGUF 模型加载失败时，具体的 llama.cpp/ggml 日志行会附加到错误信息中。
- **更易读的活动错误** — 失败任务的消息以中央省略号截断，GGUF 文件名高亮显示，多行日志可通过「Show more」开关展开。
- **更新身份** — `tauri.conf.json` 指向此分支的发布端点，并使用分支自己的更新器密钥签名。

## 本地（未签名）安装

上游项目的发布二进制文件带有 Authenticode 签名。本地（或未配置 Azure 签名密钥的此分支 CI）构建的版本**未签名**，因此 Windows 可能在首次启动时显示「发布者未知」的 SmartScreen 警告。这是正常现象，程序本身完全相同。

Windows 上典型的本地安装布局：

```text
D:\koharu\
├── koharu.exe              # Tauri 应用程序（CEF 运行时位于其旁）
├── koharu-torch.dll        # 原生 torch 运行时
├── uninstall.exe
└── store\                  # 运行时存储（见下文）
```

要更新现有安装，请将新构建的文件集复制到安装目录，**排除 `store\` 和 `uninstall.exe`**——必须保留存储，以免模型被重新下载。覆盖前请先备份之前的可执行文件。

启用分支的更新器密钥后，从此仓库构建的应用只接受由分支密钥签名的更新清单，并检查此分支的 GitHub 发布。

## 运行时存储

存储配置在 `resource_dir()/store`——对于上面的布局，即 `D:\koharu\store`：

```text
store\
├── hugging-face\
│   ├── models\    <owner>--<repo>\snapshots\<commit>\<file>
│   └── datasets\  ...
├── cuda\          # CUDA 运行时包
├── llama\         # llama.cpp 运行时
├── torch\         # libtorch
└── diffusion\     # stable-diffusion.cpp 运行时
```

Hugging Face 文件通过精确的仓库、修订版本和文件名解析。下载会先暂存，再对照仓库元数据（LFS 文件的大小 + SHA-256）验证，之后才发布。截断或损坏的文件会被拒绝而不是部署，大小不正确的文件会在下次启动时重新下载。运行时包（llama.cpp、CUDA wheels、torch）也以同样方式验证，并在元数据 API 不可达时优雅地回退为仅验证大小。

存储可能增长到数十 GB。删除它会强制重新下载所有模型和运行时（在普通网络下需要数小时），因此重新安装时请保留它。

## 为 8GB 显存选择翻译模型

在 Ryzen 7600 / 32GB 内存 / RTX 3070（8GB、CUDA、BF16）上的实测数据，使用约束 JSON 模式下的 8 段日译法现实提示词：

| 模型 | 量化 | 大小 | 生成速度 | 显存（峰值，无视觉） |
|---|---|---|---|---|
| gemma-4-E2B-it | Q4_K_XL | 2.4 GB | 约 139 tok/s | 约 3.1 GB |
| **gemma-4-E4B-uncensored** | **Q4_K_P** | **5.0 GB** | **约 80 tok/s** | **约 4.8 GB** |
| Ministral-3-8B | Q4_K_M | 4.8 GB | 约 66 tok/s | 约 6.6 GB |
| gemma-4-12B-it | Q4_K_XL | 6.3 GB | 约 20 tok/s | 约 7.7 GB |

8GB 显卡的推荐：

- **默认：`gemma4-e4b-uncensored`（Q4_K_P）** — 无审查、支持视觉（投影仪约增加 0.9GB）、轻松容纳在 8GB 内，速度约为 12B 模型的 4 倍。
- **快速纯文本：** `ministral-3-8b-instruct` — 最快的高密度模型，但没有视觉。
- **带视觉且极快：** `gemma4-e2b-it` — 显存占用低，翻译质量较低。
- **8GB 上避免 `gemma4-12b-it`** — 在*不*含视觉投影仪的情况下就达到约 7.7GB，并且是最慢的选择。

所选模型和量化位于 `~/.koharu/config.toml`：

```toml
[pipeline.translation.model]
model = "gemma4-e4b-uncensored"
provider = "local"
quantization = "Q4_K_P"
vision = true
```
