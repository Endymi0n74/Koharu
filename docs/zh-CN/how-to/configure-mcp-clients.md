---
title: 配置 MCP 客户端
---

# 配置 MCP 客户端

Koharu 通过本地 Streamable HTTP 暴露内置 MCP 服务器。本页说明如何把 MCP 客户端接到它上面，并给出 Antigravity、Claude Desktop 与 Claude Code 的详细配置步骤。

## Koharu 通过 MCP 暴露了什么

Koharu 的 MCP 服务器与桌面应用和 headless Web UI 使用的是同一个本地运行时。当前工具表面刻意保持精简，聚焦在项目生命周期、历史层和管线任务上：

- `koharu.open_project` / `koharu.close_project`
- `koharu.apply` / `koharu.undo` / `koharu.redo`
- `koharu.start_pipeline`

要进行更丰富的查询与编辑——例如场景快照、页面缩略图、blob 拉取、字体列表、LLM 控制、导出、配置——Agent 可以直接调用 Koharu 的 HTTP API（`http://127.0.0.1:<PORT>/api/v1`）。HTTP API 与 MCP 服务器共用同一个进程和状态，因此 Agent 可以在同一次工作流中自由混用两者。

完整的工具列表与参数 schema 请参见 [MCP 工具参考](../reference/mcp-tools.md)。

## 1. 用固定端口启动 Koharu

使用固定端口，这样你的 MCP 客户端总能连到同一个 URL。

```bash
# macOS / Linux
koharu --port 9999 --headless

# Windows
koharu.exe --port 9999 --headless
```

你也可以保留桌面窗口，同时暴露 MCP：

```bash
# macOS / Linux
koharu --port 9999

# Windows
koharu.exe --port 9999
```

此时 Koharu 的 MCP 端点就是：

```text
http://127.0.0.1:9999/mcp
```

重要细节：

- 当 MCP 客户端连接时，Koharu 进程必须保持运行
- Koharu 默认绑定到 `127.0.0.1`，因此以下示例都假定 MCP 客户端和 Koharu 在同一台机器上
- 默认本地配置下不需要额外鉴权头

## 2. 快速检查端点是否正常

在编辑任何客户端配置前，先确认 Koharu 确实已经运行在预期端口。

打开：

```text
http://127.0.0.1:9999/
```

如果 Web UI 能打开，就说明本地服务已经起来，对应的 MCP 端点也应该在 `/mcp`。

## Antigravity

Antigravity 可以通过原始 MCP 配置直接指向 Koharu 的本地 URL。

### 步骤

1. 用 `--port 9999` 启动 Koharu。
2. 打开 Antigravity。
3. 打开编辑器 Agent 面板顶部的 `...` 菜单。
4. 点击 **Manage MCP Servers**。
5. 点击 **View raw config**。
6. 在 `mcpServers` 下新增一个 `koharu` 条目。
7. 保存配置。
8. 如果 Antigravity 没有自动重载 MCP 服务器，请重启它。

### 示例配置

```json
{
  "mcpServers": {
    "koharu": {
      "serverUrl": "http://127.0.0.1:9999/mcp"
    }
  }
}
```

如果你已经配置了其他 MCP 服务器，请把 `koharu` 加进去，不要直接覆盖整个 `mcpServers` 对象。

### 配好之后先试什么

先问几个简单问题：

- `你能使用哪些 Koharu MCP 工具？`
- `打开位于 C:\\projects\\my-manga.khrproj 的 Koharu 项目。`

如果这一步通了，再尝试真实工作：

- `打开 C:\\projects\\my-manga.khrproj 项目，并以 detect、ocr、llm-translate、aot-inpainting、koharu-renderer 这几个 step 启动管线。`
- `撤销 Koharu 中的最近一次编辑。`
- `把这个 Op 应用到第 <id> 页，新增一个文本块：{ ... }`

## Claude Desktop

Claude Desktop 当前本地 MCP 配置是基于命令的。由于 Koharu 暴露的是本地 HTTP MCP 端点，而不是打包成桌面扩展的插件，所以实际可行的配置方式是使用一个小型桥接进程，把 Claude Desktop 接到 `http://127.0.0.1:9999/mcp`。

本页使用 `mcp-remote` 作为桥接工具。

### 开始前

请确保满足以下条件之一：

- 机器上已经有 `npx`
- 已安装 Node.js，因此可以运行 `npx`

### 步骤

1. 用 `--port 9999` 启动 Koharu。
2. 打开 Claude Desktop。
3. 打开 **Settings**。
4. 进入 **Developer** 区域。
5. 从 Claude Desktop 自带入口打开 MCP 配置文件。
6. 添加一个 `koharu` 服务器条目。
7. 保存文件。
8. 完全重启 Claude Desktop。

### Windows 配置

```json
{
  "mcpServers": {
    "koharu": {
      "command": "C:\\Progra~1\\nodejs\\npx.cmd",
      "args": [
        "-y",
        "mcp-remote@latest",
        "http://127.0.0.1:9999/mcp"
      ],
      "env": {}
    }
  }
}
```

### macOS / Linux 配置

```json
{
  "mcpServers": {
    "koharu": {
      "command": "npx",
      "args": [
        "-y",
        "mcp-remote@latest",
        "http://127.0.0.1:9999/mcp"
      ],
      "env": {}
    }
  }
}
```

注意事项：

- 如果你已经有其他 `mcpServers` 条目，请在保留原有配置的前提下增加 `koharu`
- `mcp-remote@latest` 第一次运行时会拉取包，所以首次启动可能需要联网
- 如果 Windows 上的 Node 没安装在 `C:\\Program Files\\nodejs`，请相应修改 `command` 路径

### 配好之后先试什么

新开一个 Claude Desktop 会话，先问：

- `你能使用哪些 Koharu MCP 工具？`
- `打开位于 D:\\projects\\my-manga.khrproj 的 Koharu 项目。`

然后再做真实工作：

- `在我刚才打开的项目上运行 Koharu 管线，step 包括 detect、ocr、llm-translate、aot-inpainting、koharu-renderer。`
- `用 Koharu 的 HTTP API（http://127.0.0.1:9999/api/v1/operations）查看管线状态。`
- `用 Koharu 的 HTTP API 把项目导出为 PSD。`

## Claude Code

如果你说的 “Claude” 指的是 Claude Code，那么连接 Koharu 的本地 `http://127.0.0.1` MCP 端点，最稳妥的方式同样是 stdio 桥接。

### 添加到用户配置

macOS / Linux：

```bash
claude mcp add-json koharu "{\"type\":\"stdio\",\"command\":\"npx\",\"args\":[\"-y\",\"mcp-remote@latest\",\"http://127.0.0.1:9999/mcp\"],\"env\":{}}" --scope user
```

Windows：

```bash
claude mcp add-json koharu "{\"type\":\"stdio\",\"command\":\"cmd\",\"args\":[\"/c\",\"npx\",\"-y\",\"mcp-remote@latest\",\"http://127.0.0.1:9999/mcp\"],\"env\":{}}" --scope user
```

在原生 Windows 上，Claude Code 官方文档明确建议对使用 `npx` 的本地 stdio MCP 服务器采用 `cmd /c npx` 这一包装方式。

### 验证配置

```bash
claude mcp get koharu
claude mcp list
```

如果你已经在 Claude Desktop 里配置好了 Koharu，在支持的平台上也可以尝试导入：

```bash
claude mcp add-from-claude-desktop --scope user
```

## 初次连接后建议先做的事

连接完成后，推荐先做以下几步：

- 让 Agent 列出可用的 Koharu MCP 工具
- 打开一个已存在的 Koharu 项目目录
- 用一个简短的 step 列表启动管线（例如 `detect`、`ocr`）
- 让 Agent 通过 HTTP 读取 `GET /api/v1/scene.json` 检查结果，再运行完整管线

把精简的 MCP 工具表面与直接的 HTTP 调用混用是刻意的设计——既能保持协议层最小，又能让 Agent 访问完整的编辑器状态。

## 常见错误

- 启动 Koharu 时没有加 `--port`，却试图把客户端连到错误的端口
- 使用了 `http://127.0.0.1:9999/` 而不是 `http://127.0.0.1:9999/mcp`
- 添加完客户端配置后，把 Koharu 进程关掉了
- 直接覆盖整个客户端配置，而不是追加一个 `koharu` 条目
- 以为 Claude Desktop 可以不用桥接进程，直接用一个无 `command` 的配置连 HTTP URL
- 忘了 Koharu 默认只对本机开放

## 相关页面

- [以 GUI、Headless 与 MCP 模式运行](run-gui-headless-and-mcp.md)
- [MCP 工具参考](../reference/mcp-tools.md)
- [CLI 参考](../reference/cli.md)
- [故障排查](troubleshooting.md)

## 外部参考

- [Claude Code MCP 文档](https://code.claude.com/docs/en/mcp)
- [Claude 帮助：通过远程 MCP 服务器构建自定义连接器](https://support.claude.com/en/articles/11503834-building-custom-connectors-via-remote-mcp-servers)
- [Wolfram 支持文章：包含 Antigravity 与 Claude Desktop 的 MCP 配置示例](https://support.wolfram.com/73463/)
