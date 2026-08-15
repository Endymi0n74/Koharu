---
title: Fork Guide
description: Build and install this fork locally, understand the runtime store, and pick a translation model for an 8 GB GPU.
---

# Fork Guide

This page documents the `Endymi0n74/Koharu` fork: how a local build is installed without
code-signing, where the runtime store lives, and how to choose a translation model for a
consumer GPU with 8 GB of VRAM (for example an RTX 3070).

## What this fork changes

On top of upstream Koharu, this fork currently adds:

- **Download integrity validation** — model and runtime-package downloads are verified
  against expected size and SHA-256 before they are published to the store.
- **llama.cpp log surfacing** — when a GGUF model fails to load, the concrete llama.cpp/ggml
  log lines are appended to the error.
- **Readable activity errors** — failed-job messages are truncated with a central ellipsis,
  the GGUF filename is highlighted, and multi-line logs expand behind a *Show more* toggle.
- **Update identity** — `tauri.conf.json` points at this fork's release endpoint and is signed
  with the fork's own updater key.

## Local (unsigned) installation

Release binaries from the upstream project are Authenticode-signed. Builds produced locally
(or by this fork's CI without Azure signing secrets) are **not signed**, so Windows may show a
"Unknown publisher" SmartScreen warning on first launch. This is expected; the program is
identical otherwise.

A typical local installation layout on Windows:

```text
D:\koharu\
├── koharu.exe              # Tauri application (CEF runtime alongside it)
├── koharu-torch.dll        # native torch runtime
├── uninstall.exe
└── store\                  # runtime store (see below)
```

To update an existing installation, copy the freshly built file set into the install
directory, **excluding `store\` and `uninstall.exe`** — the store must survive so models are
not re-downloaded. Keep a backup of the previous executable before overwriting.

After the fork's updater key is enabled, an app built from this repository only accepts update
manifests signed with the fork's key, and checks this fork's GitHub releases.

## Runtime store

The store is configured at `resource_dir()/store` — for the layout above, `D:\koharu\store`:

```text
store\
├── hugging-face\
│   ├── models\    <owner>--<repo>\snapshots\<commit>\<file>
│   └── datasets\  ...
├── cuda\          # CUDA runtime packages
├── llama\         # llama.cpp runtime
├── torch\         # libtorch
└── diffusion\     # stable-diffusion.cpp runtime
```

Hugging Face files resolve by exact repository, revision, and filename. Downloads are staged,
then verified against the repository metadata (size + SHA-256 for LFS files) before being
published. A truncated or corrupt file is rejected instead of deployed, and a file with the
wrong size is re-downloaded on the next launch. Runtime packages (llama.cpp, CUDA wheels,
torch) are validated the same way, with a graceful fallback to size-only verification when a
metadata API is unreachable.

The store can grow to tens of gigabytes. Deleting it forces every model and runtime to be
re-downloaded (several hours on a typical connection), so keep it intact when reinstalling.

## Choosing a translation model for 8 GB of VRAM

Measurements on a Ryzen 7600 / 32 GB RAM / RTX 3070 (8 GB, CUDA, BF16), with a realistic
8-segment Japanese→French prompt under the constrained JSON schema:

| Model | Quantization | Size | Generation | VRAM (peak, no vision) |
|---|---|---|---|---|
| gemma-4-E2B-it | Q4_K_XL | 2.4 GB | ~139 tok/s | ~3.1 GB |
| **gemma-4-E4B-uncensored** | **Q4_K_P** | **5.0 GB** | **~80 tok/s** | **~4.8 GB** |
| Ministral-3-8B | Q4_K_M | 4.8 GB | ~66 tok/s | ~6.6 GB |
| gemma-4-12B-it | Q4_K_XL | 6.3 GB | ~20 tok/s | ~7.7 GB |

Recommendation for an 8 GB card:

- **Default: `gemma4-e4b-uncensored` (Q4_K_P)** — uncensored, vision-capable (adds ~0.9 GB
  for the projector), comfortably inside 8 GB, and roughly 4× faster than the 12B model.
- **Fast text-only:** `ministral-3-8b-instruct` — fastest dense option, but no vision.
- **Very fast with vision:** `gemma4-e2b-it` — low VRAM, lower translation quality.
- **Avoid `gemma4-12b-it` on 8 GB** — it peaks at ~7.7 GB *without* the vision projector and
  becomes the slowest option.

The selected model and quantization live in `~/.koharu/config.toml`:

```toml
[pipeline.translation.model]
model = "gemma4-e4b-uncensored"
provider = "local"
quantization = "Q4_K_P"
vision = true
```
