---
title: CLI Reference
---

# CLI Reference

This page documents the command-line options exposed by Koharu's desktop binary.

Koharu uses the same binary for:

- desktop startup
- headless local Web UI
- the local HTTP API
- the built-in MCP server

## Common usage

```bash
# macOS / Linux
koharu [OPTIONS]

# Windows
koharu.exe [OPTIONS]
```

## Options

| Option | Meaning |
| --- | --- |
| `-d`, `--download` | Prefetch runtime libraries and the default vision and OCR stack, then exit |
| `--cpu` | Force CPU mode even when a GPU is available |
| `-p`, `--port <PORT>` | Bind the local HTTP server to a specific port instead of a random one |
| `--host <HOST>` | Bind the HTTP service to a specific host instead of `127.0.0.1` |
| `--headless` | Run without starting the desktop GUI |
| `--debug` | Enable debug-oriented console output |

## Behavior notes

Some flags affect more than startup appearance:

- without `--port`, Koharu chooses a random local port
- without `--host`, Koharu binds only to `127.0.0.1` so the API is reachable from the same machine only
- with `--headless`, Koharu skips the Tauri window but still serves the Web UI and API
- with `--download`, Koharu exits after dependency prefetch and does not stay running
- with `--cpu`, both the vision stack and local LLM path avoid GPU acceleration

When a fixed port is set, the main local endpoints are:

- `http://localhost:<PORT>/`
- `http://localhost:<PORT>/api/v1`
- `http://localhost:<PORT>/mcp`

## Common patterns

Start headless Web UI on a stable port:

```bash
koharu --port 4000 --headless
```

Start with CPU-only inference:

```bash
koharu --cpu
```

Download runtime packages ahead of time:

```bash
koharu --download
```

Run a local MCP endpoint on a stable port:

```bash
koharu --port 9999
```

Then connect your MCP client to:

```text
http://localhost:9999/mcp
```

Start with explicit debug logging:

```bash
koharu --debug
```

Bind to all interfaces so other machines on the local network can reach the Web UI and API:

```bash
koharu --host 0.0.0.0 --port 4000 --headless
```

This is the practical pattern for running Koharu in a container or VM where the desktop client lives on a different host. Anything other than `127.0.0.1` reachable from the network deliberately, so only set `--host` when you actually want non-loopback access.
