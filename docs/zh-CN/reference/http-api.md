---
title: HTTP API 参考
---

# HTTP API 参考

Koharu 在本地暴露 HTTP API：

```text
http://127.0.0.1:<PORT>/api/v1
```

桌面 UI 与 headless Web UI 使用的就是这套 API。

## 运行时模型

当前实现中的重要行为：

- API 与 GUI 或 headless 运行时由同一个进程提供
- 服务器默认绑定到 `127.0.0.1`；使用 `--host` 可绑定到其他地址
- API 与 MCP 服务器共享同一个已加载的项目、模型和管线状态
- 没有提供 `--port` 时，Koharu 会选择一个随机本地端口
- 在应用完成启动之前，除 `/api/v1/downloads`、`/api/v1/operations` 与 `/api/v1/events` 之外的所有路由都会返回 `503 Service Unavailable`

## 资源模型

API 是以项目为中心的。同一时间只会打开一个项目，它包含：

- 一组 `Pages`，按 `PageId` 索引
- 每个页面下的 `Nodes`（图像图层、掩码、文本块），通过 `NodeId` 引用
- 一个内容寻址的 `Blob` 存储，按 Blake3 哈希保存原始图像字节
- 由这些组件构建出的 `Scene` 快照，并通过 `epoch` 计数器递进
- 一段 `Op` 修改历史，可以撤销与重做

所有修改都必须经过历史层（`POST /history/apply`），以保证场景、自动保存和事件订阅者都保持同步。

## 常见响应类型

高频返回结构包括：

- `MetaInfo`：应用版本与 ML 设备
- `EngineCatalog`：每个管线阶段可安装的引擎 id
- `ProjectSummary`：id、名称、路径、页数、最近一次打开时间
- `SceneSnapshot`：`{ epoch, scene }`
- `LlmState`：当前 LLM 加载状态（status、target、error）
- `LlmCatalog`：按系列分组的本地 + 提供商模型
- `JobSummary`：`{ id, kind, status, error }`
- `DownloadProgress`：包 id、字节数、状态

## 端点

### 元信息

| 方法  | 路径       | 用途                            |
| ----- | ---------- | ------------------------------- |
| `GET` | `/meta`    | 获取应用版本与当前 ML 后端      |
| `GET` | `/engines` | 列出每个管线阶段已注册的引擎    |

### 字体

| 方法   | 路径                              | 用途                                          |
| ------ | --------------------------------- | --------------------------------------------- |
| `GET`  | `/fonts`                          | 系统字体 + Google Fonts 的合并目录，用于渲染   |
| `GET`  | `/google-fonts`                   | 单独列出 Google Fonts 目录                     |
| `POST` | `/google-fonts/{family}/fetch`    | 下载并缓存某个 Google Fonts 字体族             |
| `GET`  | `/google-fonts/{family}/{file}`   | 提供已缓存的 TTF/WOFF 文件                     |

### 项目

每个项目都位于受管理的 `{data.path}/projects/` 目录下；客户端永远不需要传入文件系统路径。

| 方法     | 路径                          | 用途                                              |
| -------- | ----------------------------- | ------------------------------------------------- |
| `GET`    | `/projects`                   | 列出受管理的项目                                  |
| `POST`   | `/projects`                   | 创建新项目（请求体 `{ name }`）                    |
| `POST`   | `/projects/import`            | 把一个 `.khr` 归档解压到新目录并打开                |
| `PUT`    | `/projects/current`           | 通过 `id` 打开一个受管理项目                       |
| `DELETE` | `/projects/current`           | 关闭当前会话                                      |
| `POST`   | `/projects/current/export`    | 导出当前项目；返回二进制字节                       |

`POST /projects/current/export` 接受 `{ format, pages? }`，其中 `format` 是 `khr`、`psd`、`rendered`、`inpainted` 之一。当某种格式会产生多个文件时，响应类型是 `application/zip`。

### 页面

| 方法   | 路径                                    | 用途                                                |
| ------ | --------------------------------------- | --------------------------------------------------- |
| `POST` | `/pages`                                | 通过 multipart 上传 N 张图片创建页面                  |
| `POST` | `/pages/from-paths`                     | Tauri 专用的快速通道，按绝对路径导入                   |
| `POST` | `/pages/{id}/image-layers`              | 从上传文件添加一个 Custom 图像节点                     |
| `PUT`  | `/pages/{id}/masks/{role}`              | 用原始 PNG 字节 upsert 一个掩码节点                   |
| `GET`  | `/pages/{id}/thumbnail`                 | 获取页面缩略图（以 WebP 缓存）                        |

`role` 是 `segment` 或 `brushInpaint`。`POST /pages` 接受可选字段 `replace=true`；导入时会按文件名自然顺序排序。

### Scene 与 blobs

| 方法  | 路径               | 用途                                                              |
| ----- | ------------------ | ----------------------------------------------------------------- |
| `GET` | `/scene.json`      | 完整的场景快照，供 web/UI 客户端使用                                |
| `GET` | `/scene.bin`       | postcard 编码的 `Snapshot { epoch, scene }`，供 Tauri 客户端使用     |
| `GET` | `/blobs/{hash}`    | 按 Blake3 哈希返回 blob 的原始字节                                  |

`/scene.bin` 会在 `x-koharu-epoch` 响应头里附带当前 epoch。

### 历史（修改）

所有场景修改都从这里走。每个响应都返回 `{ epoch }`。

| 方法   | 路径                | 用途                                  |
| ------ | ------------------- | ------------------------------------- |
| `POST` | `/history/apply`    | 应用一个 `Op`（包括 `Op::Batch`）       |
| `POST` | `/history/undo`     | 撤销上一次应用的 op                     |
| `POST` | `/history/redo`     | 重新应用上一次撤销的 op                 |

`Op` 是一个带判别字段的联合类型，覆盖添加/删除/更新节点、添加/删除页面、批量操作以及其他场景过渡。请求体是带 JSON tag 的变体。

### 管线

| 方法   | 路径          | 用途                              |
| ------ | ------------- | --------------------------------- |
| `POST` | `/pipelines`  | 以 operation 形式启动一次管线运行  |

请求体字段：

- `steps`：按顺序执行的引擎 id（会与注册表校验）
- `pages`：可选的 `PageId` 子集；省略时处理整个项目
- `region`：可选的 inpainter 边界框（修复笔刷流程使用）
- `targetLanguage`、`systemPrompt`、`defaultFont`：每次运行可覆盖的可选参数

响应携带 `operationId`。进度与完成事件会通过 `/events` 以 `JobStarted`、`JobProgress`、`JobWarning`、`JobFinished` 推送。

### Operations

`/operations` 是正在运行和最近完成的任务（管线 + 下载）的统一登记表。

| 方法     | 路径                  | 用途                                                 |
| -------- | --------------------- | ---------------------------------------------------- |
| `GET`    | `/operations`         | 所有正在运行或最近的 operation 的快照                  |
| `DELETE` | `/operations/{id}`    | 取消一次管线运行；对下载尽力而为地清理                  |

### Downloads

| 方法   | 路径          | 用途                                            |
| ------ | ------------- | ----------------------------------------------- |
| `GET`  | `/downloads`  | 所有正在进行或最近的下载快照                      |
| `POST` | `/downloads`  | 启动一次模型包下载（`{ modelId }`）                |

`modelId` 是通过 `declare_hf_model_package!` 声明的包 id（例如 `"model:comic-text-detector:yolo-v5"`）。响应是 `{ operationId }`，其值复用包 id。

### LLM 控制

已加载模型是位于 `/llm/current` 的单例资源。

| 方法     | 路径             | 用途                                          |
| -------- | ---------------- | --------------------------------------------- |
| `GET`    | `/llm/current`   | 当前状态（status、target、error）              |
| `PUT`    | `/llm/current`   | 加载指定的 target（local 或 provider）         |
| `DELETE` | `/llm/current`   | 卸载 / 释放当前模型                            |
| `GET`    | `/llm/catalog`   | 列出可用的本地模型 + 提供商支持的模型           |

`PUT /llm/current` 接受 `LlmLoadRequest`：

- provider target：`{ kind: "provider", providerId, modelId }`
- local target：`{ kind: "local", modelId }`
- 可选的 `options { temperature, maxTokens, customSystemPrompt }`

`PUT /llm/current` 在加载任务入队后立即返回 `204`。实际就绪状态会作为 `LlmLoaded` 事件发布到 `/events`。

### 配置

| 方法     | 路径                              | 用途                                            |
| -------- | --------------------------------- | ----------------------------------------------- |
| `GET`    | `/config`                         | 读取当前 `AppConfig`                             |
| `PATCH`  | `/config`                         | 应用 `ConfigPatch`；持久化并广播变更               |
| `PUT`    | `/config/providers/{id}/secret`   | 保存（或覆盖）某个 provider 的 API key             |
| `DELETE` | `/config/providers/{id}/secret`   | 清除某个 provider 已保存的 API key                |

`AppConfig` 暴露顶层的 `data`、`http`、`pipeline` 与 `providers`：

- `data.path`：本地数据目录，用于运行时、模型缓存与项目
- `http { connectTimeout, readTimeout, maxRetries }`：下载与 provider 请求共用的 HTTP 客户端
- `pipeline { detector, fontDetector, segmenter, bubbleSegmenter, ocr, translator, inpainter, renderer }`：每个阶段选用的引擎 id
- `providers[] { id, baseUrl?, apiKey? }`：保存的 API key 在往返中始终以遮蔽占位符 `"[REDACTED]"` 出现，绝不返回原始密钥

内置 provider id：

- `openai`
- `gemini`
- `claude`
- `deepseek`
- `deepl`
- `google-translate`
- `caiyun`
- `openai-compatible`

API key 保存在平台凭据存储中，而不是 `config.toml` 里。PATCH `apiKey: ""` 会清除已保存的 key；PATCH `"[REDACTED]"` 表示保持原值不变。专门的 `/config/providers/{id}/secret` 路由是显式管理某个 provider 密钥的非 PATCH 方式。

## 事件流

Koharu 通过以下地址暴露 Server-Sent Events：

```text
GET /events
```

行为：

- 全新连接（不带 `Last-Event-ID` 请求头）会先收到一个 `Snapshot` 事件，包含当前的任务和下载登记表
- 重连时，服务器会按顺序回放缓冲区中 `seq > Last-Event-ID` 的事件；如果请求的 id 已经从环形缓冲区中滚出，则会重新发送一个 `Snapshot`
- 每个实时事件都会用其 `seq` 作为 SSE `id:` 字段
- 维持 15 秒的 keep-alive

当前事件变体包括：

- `Snapshot`：用于全新客户端与延迟恢复客户端的完整状态种子
- `JobStarted`、`JobProgress`、`JobWarning`、`JobFinished`：管线任务生命周期
- `DownloadProgress`：包下载进度
- `ConfigChanged`：通过 `PATCH /config` 或 secret 路由应用了配置变更
- `LlmLoaded`、`LlmUnloaded`：LLM 生命周期切换
- `SceneAdvanced`：场景修改使 epoch 推进时触发

## 典型工作流

新建一个项目时常见的 API 调用顺序：

1. `POST /projects`：创建项目
2. `POST /pages`（或 Tauri 下的 `/pages/from-paths`）：导入图片
3. `PUT /llm/current`：加载翻译模型（本地或 provider）
4. `POST /pipelines`：启动 `detect → ocr → translate → inpaint → render`
5. 监听 `GET /events` 直到 `JobFinished`
6. `POST /projects/current/export`，`format = "rendered"` 或 `"psd"`

如果你需要更精细的控制，可以直接 `POST /history/apply` 携带显式 `Op` 负载，而不是运行整条管线。

如果你更想用面向 Agent 的接口，而不是手动编排 HTTP 端点，请参见 [MCP 工具参考](mcp-tools.md)。
