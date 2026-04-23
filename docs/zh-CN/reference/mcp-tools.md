---
title: MCP 工具参考
---

# MCP 工具参考

Koharu 在以下地址暴露 MCP 工具：

```text
http://127.0.0.1:<PORT>/mcp
```

MCP 服务器使用 `rmcp 1.5` 的 streamable HTTP 传输，并与 GUI 和 HTTP API 共享同一个项目、场景与管线状态。

## MCP 服务器目前暴露的内容

当前实现刻意只暴露一个小而底层的接口面，聚焦在项目生命周期、历史层和管线任务上。细粒度编辑通过 `koharu.apply` 携带 `Op` 负载完成，而不是为每个字段单独提供工具。

如果你需要更丰富的查询能力（页面缩略图、图像图层、字体列表、场景快照），请直接使用 [HTTP API](http-api.md)。两者运行在同一个端口上，并共享同一个进程内的状态。

## 工具列表

| 工具                    | 作用                                              | 参数                                                                              |
| ----------------------- | ------------------------------------------------- | --------------------------------------------------------------------------------- |
| `koharu.apply`          | 对当前场景应用一个 `Op`                            | `op`：带 JSON tag 的 `Op` 值                                                       |
| `koharu.undo`           | 撤销最近一次 op                                   | 无                                                                                |
| `koharu.redo`           | 重新应用最近一次撤销的 op                          | 无                                                                                |
| `koharu.open_project`   | 打开或创建一个 Koharu 项目目录                     | `path`，可选 `createName`                                                          |
| `koharu.close_project`  | 关闭当前项目                                      | 无                                                                                |
| `koharu.start_pipeline` | 启动一次管线运行；返回 `jobId`                     | `steps[]`，可选 `pages[]`、`targetLanguage`、`systemPrompt`、`defaultFont`         |

### `koharu.apply`

通过历史层向场景应用一次修改。`op` 值就是 HTTP API 在 `POST /history/apply` 处接受的同一个带 JSON tag 的 `Op` 枚举——常见变体包括 `AddPage`、`RemovePage`、`AddNode`、`UpdateNode`、`RemoveNode` 与 `Batch`。

返回 `{ epoch }`——op 应用后场景的新 epoch。

### `koharu.undo` / `koharu.redo`

在历史栈上向任一方向移动一格。两者都返回 `{ epoch }`，当到达栈边界（已无可撤销或可重做的内容）时，`epoch` 为 `null`。

### `koharu.open_project`

打开一个已存在的项目目录，或在指定路径创建一个新项目。传入 `createName` 会在该路径下新建项目；省略它则直接打开已存在的内容。

返回当前会话的 `{ name, path }`。

### `koharu.close_project`

关闭当前会话。在新项目被打开之前，任何需要项目的后续调用都会返回 `invalid request` 错误。

### `koharu.start_pipeline`

在后台启动一次管线运行。`steps` 是通过管线 `Registry` 注册的引擎 id 的有序列表（会与 `GET /api/v1/engines` 校验）。省略 `pages` 表示在项目里所有页面上运行；传入 `PageId` 列表则把范围限定到子集。

调用立即返回 `{ jobId }`。进度与完成事件通过 HTTP `/events` 流推送，事件类型为 `JobStarted`、`JobProgress`、`JobWarning` 与 `JobFinished`。MCP 传输本身不流式输出任务进度——你需要通过 SSE 来观察。

## 建议的 Agent 流程

大多数 Agent 会话的结构如下：

1. `koharu.open_project`：指向某个受管理的项目目录
2. 通过 HTTP 读取 `GET /api/v1/scene.json` 检查场景
3. 二选一：
    - 通过 `koharu.apply` 携带显式 `Op` 负载进行局部编辑，或者
    - 通过 `koharu.start_pipeline` 运行端到端管线，并监听 `GET /api/v1/events`
4. 通过 HTTP `POST /api/v1/projects/current/export` 导出
5. `koharu.close_project`

当某次 op 出错、想撤回而不是手算逆操作时，`koharu.undo` 与 `koharu.redo` 很有用。

## 相关页面

- [配置 MCP 客户端](../how-to/configure-mcp-clients.md)
- [以 GUI、Headless 与 MCP 模式运行](../how-to/run-gui-headless-and-mcp.md)
- [HTTP API 参考](http-api.md)
