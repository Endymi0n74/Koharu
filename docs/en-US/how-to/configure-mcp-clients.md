---
title: Configure MCP Clients
---

# Configure MCP Clients

Koharu exposes a built-in MCP server over local Streamable HTTP. This page shows how to connect MCP clients to it, with concrete setup for Antigravity, Claude Desktop, and Claude Code.

## What Koharu exposes over MCP

Koharu's MCP server is the same local runtime used by the desktop app and headless Web UI. The current tool surface is intentionally small and centred on the project lifecycle, the history layer, and pipeline jobs:

- `koharu.open_project` / `koharu.close_project`
- `koharu.apply` / `koharu.undo` / `koharu.redo`
- `koharu.start_pipeline`

For richer inspection and editing — scene snapshots, page thumbnails, blob fetches, font lists, LLM control, exports, configuration — agents call Koharu's HTTP API at `http://127.0.0.1:<PORT>/api/v1` directly. The HTTP API and MCP server share the same process and state, so an agent can mix the two freely in a single workflow.

For the full tool list and parameter schemas, see [MCP Tools Reference](../reference/mcp-tools.md).

## 1. Start Koharu on a stable port

Use a fixed port so your MCP client always has the same URL.

```bash
# macOS / Linux
koharu --port 9999 --headless

# Windows
koharu.exe --port 9999 --headless
```

You can also keep the desktop window and still expose MCP:

```bash
# macOS / Linux
koharu --port 9999

# Windows
koharu.exe --port 9999
```

Koharu's MCP endpoint will then be:

```text
http://127.0.0.1:9999/mcp
```

Important details:

- keep Koharu running while the MCP client is connected
- Koharu binds to `127.0.0.1` by default, so these examples assume the MCP client is on the same machine
- no authentication headers are required for the default local setup

## 2. Quick endpoint check

Before editing any client config, make sure Koharu is actually running on the expected port.

Open:

```text
http://127.0.0.1:9999/
```

If the Web UI loads, the local server is up and the MCP endpoint should also exist at `/mcp`.

## Antigravity

Antigravity can point directly at Koharu's local MCP URL through its raw MCP config.

### Steps

1. Start Koharu with `--port 9999`.
2. Open Antigravity.
3. Open the `...` menu at the top of the editor's agent panel.
4. Click **Manage MCP Servers**.
5. Click **View raw config**.
6. Add a `koharu` entry under `mcpServers`.
7. Save the config.
8. Restart Antigravity if it does not reload the MCP server automatically.

### Example config

```json
{
  "mcpServers": {
    "koharu": {
      "serverUrl": "http://127.0.0.1:9999/mcp"
    }
  }
}
```

If you already have other MCP servers configured, add `koharu` alongside them instead of replacing the whole `mcpServers` object.

### After setup

Ask Antigravity something simple first:

- `What Koharu MCP tools do you have available?`
- `Open the Koharu project at C:\\projects\\my-manga.khrproj.`

If that works, move on to actual work such as:

- `Open the project at C:\\projects\\my-manga.khrproj and start a pipeline with steps detect, ocr, llm-translate, aot-inpainting, koharu-renderer.`
- `Undo the last edit in Koharu.`
- `Apply this Op to add a new text block to page <id>: { ... }`

## Claude Desktop

Claude Desktop's current local MCP config is command-based. Because Koharu exposes a local HTTP MCP endpoint rather than a packaged desktop extension, the practical approach is to use a small bridge process that connects Claude Desktop to `http://127.0.0.1:9999/mcp`.

This guide uses `mcp-remote` for that bridge.

### Before you start

Make sure one of these is true:

- `npx` is already available on your machine
- Node.js is installed so `npx` can run

### Steps

1. Start Koharu with `--port 9999`.
2. Open Claude Desktop.
3. Open **Settings**.
4. Open the **Developer** section.
5. Open the MCP config file from Claude Desktop's built-in editor entry.
6. Add a `koharu` server entry.
7. Save the file.
8. Fully restart Claude Desktop.

### Windows config

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

### macOS / Linux config

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

Notes:

- if you already have other entries in `mcpServers`, add `koharu` without deleting them
- `mcp-remote@latest` is fetched on first use, so the first startup may need internet access
- if your Windows Node install is not under `C:\\Program Files\\nodejs`, update the `command` path accordingly
- Anthropic's current remote-MCP connector flow for Claude Desktop is managed through **Settings > Connectors** for actual remote servers; this page intentionally covers the config-file bridge pattern for Koharu's local `127.0.0.1` endpoint

### After setup

Open a new Claude Desktop chat and ask:

- `What Koharu MCP tools do you have available?`
- `Open the Koharu project at D:\\projects\\my-manga.khrproj.`

Then move to actual work:

- `Run a Koharu pipeline with steps detect, ocr, llm-translate, aot-inpainting, koharu-renderer on the project I just opened.`
- `Use Koharu's HTTP API at http://127.0.0.1:9999/api/v1/operations to check pipeline status.`
- `Use Koharu's HTTP API to export the project as PSD.`

## Claude Code

If by "Claude" you mean Claude Code, the safest setup for Koharu's local `http://127.0.0.1` MCP endpoint is to use the same stdio bridge pattern.

### Add it to your user config

macOS / Linux:

```bash
claude mcp add-json koharu "{\"type\":\"stdio\",\"command\":\"npx\",\"args\":[\"-y\",\"mcp-remote@latest\",\"http://127.0.0.1:9999/mcp\"],\"env\":{}}" --scope user
```

This writes the server into Claude Code's MCP configuration for your user account.

Windows:

```bash
claude mcp add-json koharu "{\"type\":\"stdio\",\"command\":\"cmd\",\"args\":[\"/c\",\"npx\",\"-y\",\"mcp-remote@latest\",\"http://127.0.0.1:9999/mcp\"],\"env\":{}}" --scope user
```

On native Windows, Claude Code's docs explicitly recommend the `cmd /c npx` wrapper for local stdio MCP servers that use `npx`.

### Verify it

```bash
claude mcp get koharu
claude mcp list
```

If you already configured Koharu in Claude Desktop, Claude Code can also import compatible entries from Claude Desktop on supported platforms:

```bash
claude mcp add-from-claude-desktop --scope user
```

## First tasks to try

Once the client is connected, these are good first tasks:

- ask the agent which Koharu MCP tools are available
- open an existing Koharu project directory
- start a pipeline with a small step list (e.g. `detect`, `ocr`)
- have the agent read `GET /api/v1/scene.json` over HTTP to inspect the result before running the full pipeline

Mixing the small MCP tool surface with direct HTTP calls is intentional — it keeps the protocol surface tiny while still giving agents access to the full editor state.

## Common mistakes

- starting Koharu without `--port`, then trying to connect a client to the wrong port
- using `http://127.0.0.1:9999/` instead of `http://127.0.0.1:9999/mcp`
- closing Koharu after adding the client config
- replacing your entire client config instead of merging a new `koharu` entry
- expecting Claude Desktop to connect directly to Koharu's HTTP URL through a plain command-less config entry
- forgetting that Koharu's default local server is only reachable from the same machine

## Related pages

- [Run GUI, Headless, and MCP Modes](run-gui-headless-and-mcp.md)
- [MCP Tools Reference](../reference/mcp-tools.md)
- [CLI Reference](../reference/cli.md)
- [Troubleshooting](troubleshooting.md)

## External references

- [Claude Code MCP docs](https://code.claude.com/docs/en/mcp)
- [Claude Help: Building custom connectors via remote MCP servers](https://support.claude.com/en/articles/11503834-building-custom-connectors-via-remote-mcp-servers)
- [Wolfram support article with current Antigravity and Claude Desktop MCP config examples](https://support.wolfram.com/73463/)
