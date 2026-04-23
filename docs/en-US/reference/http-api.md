---
title: HTTP API Reference
---

# HTTP API Reference

Koharu exposes a local HTTP API under:

```text
http://127.0.0.1:<PORT>/api/v1
```

This is the same API used by the desktop UI and the headless Web UI.

## Runtime model

Important current behavior:

- the API is served by the same process as the GUI or headless runtime
- the server binds to `127.0.0.1` by default; use `--host` to bind elsewhere
- the API and MCP server share the same loaded project, models, and pipeline state
- when no `--port` is provided, Koharu chooses a random local port
- everything except `/api/v1/downloads`, `/api/v1/operations`, and `/api/v1/events` returns `503 Service Unavailable` until the app finishes bootstrapping

## Resource model

The API is project-centric. A single project is open at a time and contains:

- a list of `Pages` indexed by `PageId`
- per-page `Nodes` (image layers, masks, text blocks) referenced by `NodeId`
- a content-addressed `Blob` store that holds raw image bytes by Blake3 hash
- a `Scene` snapshot built from those pieces, advanced by an `epoch` counter
- a history of `Op` mutations that can be undone or redone

Mutations always go through the history layer (`POST /history/apply`) so the scene, autosave, and event subscribers stay in sync.

## Common response shapes

Frequently used response types include:

- `MetaInfo` — app version and ML device label
- `EngineCatalog` — installable engine ids per pipeline stage
- `ProjectSummary` — id, name, path, page count, last opened
- `SceneSnapshot` — `{ epoch, scene }`
- `LlmState` — current LLM load state (status, target, error)
- `LlmCatalog` — local + provider models grouped by family
- `JobSummary` — `{ id, kind, status, error }`
- `DownloadProgress` — package id, byte counts, status

## Endpoints

### Meta

| Method | Path        | Purpose                                    |
| ------ | ----------- | ------------------------------------------ |
| `GET`  | `/meta`     | get app version and active ML backend      |
| `GET`  | `/engines`  | list registered pipeline engines per stage |

### Fonts

| Method | Path                                  | Purpose                                              |
| ------ | ------------------------------------- | ---------------------------------------------------- |
| `GET`  | `/fonts`                              | combined system + Google Fonts catalog for rendering |
| `GET`  | `/google-fonts`                       | Google Fonts catalog as a standalone list            |
| `POST` | `/google-fonts/{family}/fetch`        | download and cache one Google Fonts family           |
| `GET`  | `/google-fonts/{family}/{file}`       | serve the cached TTF/WOFF file                       |

### Projects

Every project lives under the managed `{data.path}/projects/` directory; clients never supply filesystem paths.

| Method   | Path                              | Purpose                                                   |
| -------- | --------------------------------- | --------------------------------------------------------- |
| `GET`    | `/projects`                       | list managed projects                                     |
| `POST`   | `/projects`                       | create a new project (body `{ name }`)                    |
| `POST`   | `/projects/import`                | extract a `.khr` archive into a fresh dir and open it     |
| `PUT`    | `/projects/current`               | open a managed project by `id`                            |
| `DELETE` | `/projects/current`               | close the current session                                 |
| `POST`   | `/projects/current/export`        | export the current project; returns binary bytes          |

`POST /projects/current/export` accepts `{ format, pages? }` where `format` is one of `khr`, `psd`, `rendered`, `inpainted`. When the format produces multiple files, the response is `application/zip`.

### Pages

| Method | Path                                    | Purpose                                              |
| ------ | --------------------------------------- | ---------------------------------------------------- |
| `POST` | `/pages`                                | create pages from N uploaded image files (multipart) |
| `POST` | `/pages/from-paths`                     | Tauri-only fast path that imports by absolute path   |
| `POST` | `/pages/{id}/image-layers`              | add a Custom image node from an uploaded file        |
| `PUT`  | `/pages/{id}/masks/{role}`              | upsert a mask node from raw PNG bytes                |
| `GET`  | `/pages/{id}/thumbnail`                 | get the page thumbnail (cached as WebP)              |

`role` is `segment` or `brushInpaint`. `POST /pages` accepts an optional `replace=true` field; the import is filename-sorted using natural order.

### Scene and blobs

| Method | Path                | Purpose                                                       |
| ------ | ------------------- | ------------------------------------------------------------- |
| `GET`  | `/scene.json`       | full scene snapshot for web/UI clients                        |
| `GET`  | `/scene.bin`        | postcard-encoded `Snapshot { epoch, scene }` for Tauri client |
| `GET`  | `/blobs/{hash}`     | raw blob bytes by Blake3 hash                                 |

`/scene.bin` includes the current epoch in the `x-koharu-epoch` response header.

### History (mutations)

All scene mutations go through here. Each response returns `{ epoch }`.

| Method | Path                | Purpose                                  |
| ------ | ------------------- | ---------------------------------------- |
| `POST` | `/history/apply`    | apply an `Op` (including `Op::Batch`)    |
| `POST` | `/history/undo`     | revert the last applied op               |
| `POST` | `/history/redo`     | re-apply the last undone op              |

`Op` is the discriminated union that covers add/remove/update node, add/remove page, batch, and other scene transitions. The body is the JSON-tagged variant.

### Pipelines

| Method | Path          | Purpose                                |
| ------ | ------------- | -------------------------------------- |
| `POST` | `/pipelines`  | start a pipeline run as an operation   |

Body fields:

- `steps` — engine ids to run in order (validated against the registry)
- `pages` — optional subset of `PageId`s; omit to process the whole project
- `region` — optional bounding box for the inpainter (repair-brush flow)
- `targetLanguage`, `systemPrompt`, `defaultFont` — optional per-run overrides

The response carries an `operationId`. Progress and completion arrive on `/events` as `JobStarted`, `JobProgress`, `JobWarning`, and `JobFinished`.

### Operations

`/operations` is the unified registry for in-flight and recently-completed jobs (pipelines + downloads).

| Method   | Path                  | Purpose                                                    |
| -------- | --------------------- | ---------------------------------------------------------- |
| `GET`    | `/operations`         | snapshot of every in-flight or recent operation            |
| `DELETE` | `/operations/{id}`    | cancel a pipeline run; best-effort eviction for downloads  |

### Downloads

| Method | Path                | Purpose                                    |
| ------ | ------------------- | ------------------------------------------ |
| `GET`  | `/downloads`        | snapshot of every active or recent download |
| `POST` | `/downloads`        | start a model-package download (`{ modelId }`) |

`modelId` is a package id declared via `declare_hf_model_package!` (e.g. `"model:comic-text-detector:yolo-v5"`). The response is `{ operationId }` reusing the package id.

### LLM control

The loaded model is a singleton resource at `/llm/current`.

| Method   | Path             | Purpose                                       |
| -------- | ---------------- | --------------------------------------------- |
| `GET`    | `/llm/current`   | current state (status, target, error)         |
| `PUT`    | `/llm/current`   | load the given target (local or provider)     |
| `DELETE` | `/llm/current`   | unload / release the model                    |
| `GET`    | `/llm/catalog`   | list available local + provider-backed models |

`PUT /llm/current` accepts an `LlmLoadRequest`:

- provider targets — `{ kind: "provider", providerId, modelId }`
- local targets — `{ kind: "local", modelId }`
- optional `options { temperature, maxTokens, customSystemPrompt }`

`PUT /llm/current` returns `204` once the load task is queued. The actual ready state is published as `LlmLoaded` on `/events`.

### Config

| Method   | Path                                    | Purpose                                         |
| -------- | --------------------------------------- | ----------------------------------------------- |
| `GET`    | `/config`                               | read the current `AppConfig`                    |
| `PATCH`  | `/config`                               | apply a `ConfigPatch`; persists and broadcasts  |
| `PUT`    | `/config/providers/{id}/secret`         | save (or overwrite) a provider's API key        |
| `DELETE` | `/config/providers/{id}/secret`         | clear a provider's stored API key               |

`AppConfig` exposes top-level `data`, `http`, `pipeline`, and `providers`:

- `data.path` — local data directory used for runtime, model cache, and projects
- `http { connectTimeout, readTimeout, maxRetries }` — shared HTTP client used by downloads and provider-backed requests
- `pipeline { detector, fontDetector, segmenter, bubbleSegmenter, ocr, translator, inpainter, renderer }` — engine id selected for each stage
- `providers[] { id, baseUrl?, apiKey? }` — saved API keys round-trip as the redacted placeholder `"[REDACTED]"`; never the raw secret

Built-in provider ids:

- `openai`
- `gemini`
- `claude`
- `deepseek`
- `deepl`
- `google-translate`
- `caiyun`
- `openai-compatible`

API keys are stored in the platform credential store, not in `config.toml`. PATCHing `apiKey: ""` clears the saved key; PATCHing `"[REDACTED]"` leaves it unchanged. The dedicated `/config/providers/{id}/secret` routes are the explicit, non-PATCH way to manage one provider's secret.

## Events stream

Koharu exposes a Server-Sent Events stream at:

```text
GET /events
```

Behavior:

- a fresh connection (no `Last-Event-ID` header) starts with a `Snapshot` event holding the current jobs and downloads registries
- on reconnect, the server replays buffered events with `seq > Last-Event-ID` in order; if the requested id has scrolled out of the ring, the server re-sends a `Snapshot`
- each live event is emitted with its `seq` as the SSE `id:` field
- a 15-second keep-alive is maintained

Event variants currently include:

- `Snapshot` — full state seed for fresh and lag-recovery clients
- `JobStarted`, `JobProgress`, `JobWarning`, `JobFinished` — pipeline job lifecycle
- `DownloadProgress` — package download progress ticks
- `ConfigChanged` — config was applied via `PATCH /config` or a secret route
- `LlmLoaded`, `LlmUnloaded` — LLM lifecycle transitions
- `SceneAdvanced` — emitted when a scene mutation advances the epoch

## Typical workflow

The normal API order for one new project is:

1. `POST /projects` — create the project
2. `POST /pages` (or `/pages/from-paths` from Tauri) — import images
3. `PUT /llm/current` — load a translation model (local or provider)
4. `POST /pipelines` — kick off `detect → ocr → translate → inpaint → render`
5. tail `GET /events` until `JobFinished`
6. `POST /projects/current/export` with `format = "rendered"` or `"psd"`

For finer control, post `POST /history/apply` with explicit `Op` payloads instead of running a full pipeline.

If you want agent-oriented access instead of HTTP endpoint orchestration, see [MCP Tools Reference](mcp-tools.md).
