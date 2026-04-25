---
title: 使用 Codex 图像生成
---

# 使用 Codex 图像生成

Koharu 可以使用 Codex 进行端到端的 image-to-image 生成。这个流程会把源页面图像和提示词发送给 Codex，然后把生成出的图像保存为渲染后的页面结果。

## 要求

- 拥有 Codex 访问权限的 ChatGPT 账号
- 已为该账号启用双重身份验证
- 能够访问 OpenAI 和 ChatGPT 服务的网络连接

设备码登录要成功完成，必须先在账号上启用双重身份验证。

## 这个功能会做什么

Codex image-to-image 生成是一个整页重绘流程。它可以根据源图像和提示词完成：

- 翻译可见文字
- 移除原始字稿
- 重绘被编辑的区域
- 保留分镜、气泡、网点和构图
- 一次生成完整页面图像

这不同于 Koharu 的本地分阶段流水线。本地流水线会把检测、OCR、修复、翻译和渲染拆成独立步骤执行；Codex 流程会把页面图像发送到远程服务，并接收生成后的图像结果。

## 提示词

请用提示词描述你希望得到的整页结果。例如：

```text
Translate all visible text to natural English, remove the original lettering,
and redraw the page as a clean manga image while preserving the artwork,
panel layout, speech bubbles, tone, and composition.
```

如果只想做更窄范围的编辑，请说明目标修改以及必须保留的内容。模型会收到源页面图像，所以提示词应重点描述转换目标，而不是重新列出每个视觉细节。

## 隐私与可靠性

这个功能会把源页面图像和提示词发送到 ChatGPT Codex 后端。如果你需要离线处理，或不希望把页面图像发送给远程提供商，请使用本地流水线。

Codex 图像生成依赖 OpenAI 的上游服务。生成失败时，如果上游返回了响应文本和请求 ID，Koharu 会将其显示出来。临时故障有时可以通过重试解决。持续失败可能与账号访问权限、服务可用性，或后端对图像生成工具调用的支持状态有关。

## 何时使用

当你希望用远程模型快速完成整页重绘，并接受模型改写最终图像时，可以使用 Codex 图像生成。

当你需要更细致地控制中间 OCR、清理遮罩、翻译文本、字体和可编辑输出时，请使用本地分阶段流水线。
