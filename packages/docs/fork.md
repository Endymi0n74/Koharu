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

- **Batch CLI (`koharu-batch`)** — translate a whole chapter (folder of images or CBZ) — or a
  whole volume, one chapter per subfolder and per archive — in one unattended command, with
  natural page ordering, resume support, per-page fault isolation, retries, a machine-readable
  `--json` summary, a VRAM budget guard, and a `--llm auto` model picker that scales up with the
  GPU (see *Batch mode* below).
- **French typography profile** — `fr-FR` output normalization (guillemets, curly apostrophes,
  narrow no-break spaces) applied before rendering, with stylized lettering preserved
  (see *French typography profile* below).
- **Chapter-level translation quality** — the translator receives the source language OCR
  recognized, the already translated lines of the earlier pages, and one focused follow-up
  for any segment a response left untranslated, so a chapter does not come out half in
  Japanese (see *Translation quality and chapter continuity* below).
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

Hugging Face files resolve by exact repository, revision, and filename. Downloads are streamed,
then verified against the repository metadata (size + SHA-256 for LFS files) before being
published. A truncated or corrupt file is rejected instead of deployed, and a file with the
wrong size is re-downloaded on the next launch.

Runtime packages are checked against the digest their source publishes: the GitHub release
assets (llama.cpp, diffusion.cpp, libtorch) carry a SHA-256 digest, and CUDA wheels are checked
against the PyPI metadata. Verification degrades to the byte-count checks of the transfer
itself — instead of failing the install — when a metadata API is unreachable, and for the ROCm
wheels whose index publishes no digest.

The store can grow to tens of gigabytes. Deleting it forces every model and runtime to be
re-downloaded (several hours on a typical connection), so keep it intact when reinstalling.

## Choosing a translation model for 8 GB of VRAM

Measurements on a Ryzen 7600 / 32 GB RAM / RTX 3070 (8 GB, CUDA, BF16), with a realistic
8-segment Japanese→French prompt under the constrained JSON schema:

| Model | Quantization | Size | Generation | VRAM (peak, no vision) |
|---|---|---|---|---|
| gemma-4-E2B-it | Q4_K_XL | 2.4 GB | ~139 tok/s | ~3.1 GB |
| **gemma-4-E4B-uncensored** | **Q4_K_P** | **5.0 GB** | **~80 tok/s** | **~5.4 GB** (6.3 GB with vision) |
| Ministral-3-8B | Q4_K_M | 4.8 GB | ~66 tok/s | ~6.6 GB |
| gemma-4-12B-it | Q4_K_XL | 6.3 GB | ~20 tok/s | ~7.7 GB |

Recommendation for an 8 GB card:

- **Default: `gemma4-e4b-uncensored` (Q4_K_P)** — uncensored, vision-capable (the projector
  adds ~0.9 GiB, for a 6.3 GiB peak against the ~7.2 GiB an 8 GB card can spare), and roughly
  4× faster than the 12B model.
- **Fast text-only:** `ministral-3-8b-instruct` — fastest dense option, but no vision.
- **Very fast with vision:** `gemma4-e2b-it` — low VRAM, lower translation quality.
- **Avoid `gemma4-12b-it` on 8 GB** — it peaks at ~7.7 GB *without* the vision projector and
  becomes the slowest option.

### Larger GPUs: what `--llm auto` picks

`auto` walks the preference list from the largest model down and takes the first one that fits,
so a card with more VRAM gets a stronger translation model instead of the 8 GB recommendation.
The *download* column is what the configuration needs in the store (weights plus vision
projector); the *VRAM* column is the peak it has to fit. Both are reported in GiB, as in the
CLI output. Only the 8 GiB row is a measured peak — the larger ones come from the
parameter/quantization formula until a real run calibrates them, so treat them as planning
figures and let `--dry-run` confirm the plan.

| Card | Model | Quantization | Download | VRAM peak |
|---|---|---|---|---|
| 8 GiB | gemma4-e4b-uncensored | Q4_K_P | ≈5.3 GiB | 6.3 GiB (measured) |
| 12 GiB | gemma4-12b-uncensored | Q4_K_M | ≈7.3 GiB | 8.3 GiB (estimated) |
| 20 GiB | gemma4-26b-a4b-uncensored | Q4_K_M | ≈14.7 GiB | 15.7 GiB (estimated) |
| 24 GiB and up | gemma4-31b-uncensored | Q4_K_M | ≈17.4 GiB | 18.4 GiB (estimated) |

Below 8 GiB the list steps down to the instruct 4B, the dense 8B, and the small 2B, and a card
too small for any vision model makes `auto` refuse instead of guessing (pass `--llm` explicitly
with `--force`, or `--cpu`).

Because `auto` can now select a model that is not in the store yet, the run prints what it
picked together with the download it implies, and warns explicitly when the choice needs more
than 8 GiB — the first page then waits for that one-time download. `--list-models` shows the
same figure for every model and quantization, and `--llm <id> --quantization <id>` always
overrides the automatic pick.

The selected model and quantization live in `~/.koharu/config.toml`:

```toml
[pipeline.translation.model]
model = "gemma4-e4b-uncensored"
provider = "local"
quantization = "Q4_K_P"
vision = true
```

### Automatic calibration from real runs

The table above is only the starting point. Every `koharu-batch` run that uses NVML
telemetry records the VRAM peak it actually observed for the model configuration it ran
into `~/.koharu/vram-calibration.toml`, keeping the highest observation per configuration.
Later runs, `--llm auto`, and the budget guard then prefer that measurement over the
reference table: a model that measured over the budget on *your* machine is refused (or
stepped down by `auto`) even when the static estimate says it fits — and vice versa.

`koharu-batch --list-models` shows the VRAM estimate, the download size, and what was measured
locally.
`koharu-batch --reset-calibration` deletes the calibration file so later runs fall back
to the built-in reference estimates, and `--no-calibration` runs a translation without
reading or writing it (reference estimates, nothing recorded).

## Batch mode (`koharu-batch`)

`koharu-batch` is a CLI binary that translates an entire chapter — a folder of images or a
`.cbz` archive — in one command. It reuses the same local pipeline (detection, OCR,
inpainting, translation, rendering) as the desktop app, but runs unattended so it can be
scripted or launched for a long reading session.

Key behaviors:

- **Natural page ordering** — `page2.png` sorts before `page10.png`, padding is ignored
  (`p01_10.jpg` compares like `p1_10.jpg`), and only images (png/jpg/webp) count as pages.
- **Resume support** — pages whose output already exists are skipped, so an interrupted run
  restarts where it stopped; pass `--overwrite` to redo them. A resumed `.cbz` output carries
  the pages it already holds over into the rewritten archive instead of translating them again,
  relabeling each entry's extension from the bytes it actually contains.
- **Per-page fault isolation** — a page that fails (bad OCR, corrupted image) is reported and
  skipped; it does not abort the chapter. Each stage grants `--retries` extra attempts (default
  `1`) before a page is dropped, so a transient failure does not cost a page the run paid for,
  and the report is rewritten after every phase and page — an interruption costs the current
  page at most.
- **Volume mode** — an input folder with no images at its top level but with subfolders and/or
  `.cbz` archives is translated as a volume: one chapter per subfolder and per archive — a
  folder that only groups other folders (no pages of its own) descends into them, so
  `Vol/Ch1/p.png` is the chapter `Vol/Ch1` — outputs
  mirrored under `--output`, one report per chapter, and a global summary. The phases run
  across the whole volume so each model loads once for all of it (see *Volume mode* below).
- **Progress with ETA** — every phase prints `[i/n] stage · Xs · ETA …` per chapter; `--quiet`
  keeps failures and the final summary only.
- **Model-resident stage ordering** — detection runs across every page (of every chapter of a
  volume), then OCR, then inpainting, then translation: each model loads once per run instead
  of once per page (decisive on 8 GB cards, where the vision and language models cannot
  coexist). Fault isolation and the per-page report are unchanged, and each output page is
  written as soon as its own translation finishes — encoding, writing, and thumbnails run on a
  worker thread so they overlap the next page's translation.
- **VRAM budget guard** — the binary queries the GPU (NVML on Windows), caps the budget at
  the VRAM actually free at startup, and refuses to start if the selected model/quantization
  cannot fit; `--llm auto` picks the strongest model that
  fits (and warns before a large first download), and `--force` overrides the check.
- **French typography by default** — output text goes through the `fr-FR` profile:
  guillemets « », curly apostrophes ’, and narrow no-break spaces before `; : ! ?` and inside
  guillemets (see the typography notes below).

Typical usage:

```bash
# Translate a folder of scans to French, pages written as PNG next to it
koharu-batch --input ./chapter-12 --output ./chapter-12-fr

# Same, packaged as a CBZ, letting the tool pick the model for the GPU
koharu-batch --input ./chapter-13.cbz --output ./chapter-13-fr.cbz

# A whole volume: every subfolder and .cbz of ./series becomes one chapter
koharu-batch --input ./series --output ./series-fr

# Inspect what would run (pages, chosen model, VRAM estimate, volume rule)
# without executing
koharu-batch --input ./chapter-12 --output ./chapter-12-fr --dry-run

# Same, plus a machine-readable plan for scripts
koharu-batch --input ./series --output ./series-fr --dry-run --json plan.json

# List local models with their VRAM estimates and exit
koharu-batch --list-models
```

### Installing the binary outside the repository

Build it once from the repository (LLVM and CMake must be on the PATH, see
`docs/development/setup.md`):

```bash
cargo build --release -p koharu-pipeline --bin koharu-batch
```

For the fastest binary on the machine that runs it, prefix that build with
`RUSTFLAGS="-C target-cpu=native"` (PowerShell:
`$env:RUSTFLAGS="-C target-cpu=native"`). The result targets that CPU only, so keep such
builds local and rebuild after a CPU change.

Flash attention needs no flag: the bundled llama.cpp defaults `flash_attn_type` to `AUTO`,
which already enables it on CUDA builds. Measured on an RTX 3070 (E4B, one fixture page),
disabling it raised translation from ~11 s to ~16 s, while forcing it `ENABLED` over `AUTO`
changed nothing beyond run-to-run noise — so the default is left as is.

Then copy the single self-contained executable (48 MB on Windows, no extra DLLs) next to
a Koharu app installation — or anywhere else:

```bash
copy target\release\koharu-batch.exe D:\koharu\koharu-batch.exe
```

When the binary sits next to a Koharu installation it finds the app's `store` directory
automatically (all the models the app downloaded are reused); anywhere else, pass
`--store D:\koharu\store` once or let it fall back to `%LOCALAPPDATA%\koharu\packages`.

Full options (from `koharu-batch --help`): `--input`, `--output`, `--lang` (default
`fr-FR`), `--llm` (model id or `auto`), `--quantization`, `--no-vision` (text-only
translation, skips the vision projector), `--force`, `--vram-budget-mib`,
`--detection`, `--ocr`, `--inpainting`, `--translation-instructions`, `--format`
(png/jpg/webp for folder outputs), `--store`, `--report <BASE|none>`, `--json <PATH>`,
`--retries <N>` (extra attempts per stage, default `1`), `--pages <RANGE>` (translate only
these pages, 1-based, `1-10,15`), `--recursive` (flatten subfolders into one chapter
instead of the volume rule), `--quiet`, `--list-models`, `--reset-calibration`,
`--no-calibration`, `--overwrite`, `--dry-run`, `--cpu`.

In a volume, `--pages` applies to **each** chapter (it must fit the smallest one), and
`--recursive` opts out of the volume rule: the whole tree becomes one chapter with
relative page names. `--report` follows the same split: one file pair per chapter at
`<BASE>-<label>.md` / `.html`, a nested label's `/` flattened to `-` so both files stay beside
`BASE` (`<BASE>-Vol-Ch03.md`).

`--lang` picks the target language (default `fr-FR`). English (`--lang en-US`) is a
solid fallback: every catalog model handles it, and for Japanese or Russian sources it
is usually the strongest pair the small local models produce — use it when the target
language's output quality matters more than the language itself.

The runtime store resolves like the desktop app: when the binary sits next to a Koharu
installation (a `store` directory alongside the executable), that store is used and every
model already downloaded by the app is reused. Otherwise it falls back to the
operating-system cache (`%LOCALAPPDATA%\koharu\packages` on Windows); `--store <dir>`
points at any other store explicitly. `--dry-run` plans the run (pages, model, VRAM) without
downloading runtimes or processing pages.

Exit codes: `0` when every page of every chapter succeeds, non-zero when the
run cannot start (no pages found, VRAM guard tripped) or when at least one page failed.

### Volume mode

A **volume** is an input folder that holds **no page images at its top level** but does hold
subfolders and/or `.cbz`/`.zip` archives — the typical layout of a series or a tankōbon:

```text
series/                    ← no .png/.jpg directly inside
├── Ch01/                  ← chapter 1 (pages at any depth)
│   ├── p1.png
├── Ch02.cbz               ← chapter 2 (an archive is its own chapter)
└── Vol/                   ← a grouping folder: no pages of its own → it descends
    ├── Ch03/              ← chapter 3, label `Vol/Ch03`
    │   └── p1.png
    └── Ch04/              ← chapter 4, label `Vol/Ch04`
        └── p1.png
```

The rule, printed by every run and by `--dry-run`, is:

- page images at the top level → **one chapter** (the whole folder, subfolders ignored);
- `--recursive` → **one chapter** flattened from the whole tree;
- otherwise, with subfolders/archives → **a volume**, chapters in natural order
  (`Ch01` before `Ch10`), one report per chapter;
- a folder without pages of its own but with subfolders → **descends into it**: every chapter
  below it is one level deeper, at any depth, labelled by its path relative to the input
  (`Vol/Ch1`, `Vol/Book1/Ch2`); a folder that holds pages never descends (it stays one chapter
  listing its own subfolders);
- neither → the run refuses with a message naming both possibilities.

Outputs are mirrored under `--output`: a folder output gets one subfolder per chapter
(`--output ./out` → `./out/Ch01/p1.png`, …; a nested chapter keeps its folders:
`./out/Vol/Ch03/p1.png`), an archive output becomes a folder of archives
(`--output out.cbz` → `out/Ch01.cbz`, `out/Ch02.cbz`). Each chapter gets its own report
(`<CHAPTER-OUTPUT>.md`/`.html`), and the run ends with a global summary listing every
chapter's translated/skipped/failed counts. The exit code is `0` only when **every page of
every chapter** succeeded.

Execution is phase-major **across the whole volume**: detection runs over every chapter, then
OCR, then inpainting, then translation chapter by chapter. Each model therefore loads once
for the entire volume — the same win as the per-chapter ordering, at volume scale — while
chapters keep separate sessions so the translator's chapter context never bleeds from one
chapter into the next. A chapter that is already fully translated (resume) is skipped and
reported as such without touching its outputs.

### Machine-readable summary (`--json`)

`--json <PATH>` writes a UTF-8 JSON document when the run ends — including dry runs and
runs where some pages failed — so scripts can consume the outcome without parsing stderr:

```json
{
  "ok": true,
  "volume": false,
  "dry_run": false,
  "started_at": "2026-09-25 08:12:00 UTC",
  "elapsed_seconds": 73.3,
  "input": "./chapter-12",
  "output": "./chapter-12-fr",
  "language": "fr-FR",
  "model": { "id": "gemma4-e4b-it", "quantization": "Q4_K_XL",
             "vram_estimate": "4.3 GiB (measured)", "device": "NVIDIA GeForce RTX 3070" },
  "counts": { "translated": 6, "skipped": 0, "failed": 0, "planned": 0 },
  "chapters": [
    { "label": "", "input": "./chapter-12", "output": "./chapter-12-fr",
      "counts": { "translated": 6, "skipped": 0, "failed": 0, "planned": 0 },
      "pages": [
        { "index": 1, "name": "p1.png", "status": "translated",
          "elapsed_seconds": 11.9, "stages": [{ "stage": "translation", "seconds": 10.9 }],
          "vram_peak": "5.9 GiB", "error": null }
      ] }
  ],
  "failures": []
}
```

`ok` is `false` as soon as one page failed (`failures` then carries every chapter/page/error
triple). `status` is one of `translated`, `skipped`, `failed`, or `planned` (pages a dry run
would translate); `index` is the 1-based reading-order position; `counts.planned` is non-zero
only in a dry run. In a volume, `chapters` holds one entry per chapter with its own paths
(`label` is empty for a single-chapter run, otherwise the chapter's path relative to the input,
such as `Vol/Ch1`).

### End-of-run report

After every run (success, resume, or partial failure), `koharu-batch` writes a Markdown and
an HTML report next to the output: the output path with `.cbz`/`.zip` replaced (when present)
and `.md` / `.html` appended — `--output ./out` reports to `./out.md`, `--output out.cbz` to
`out.md`, a volume chapter `./out/Ch01` to `./out/Ch01.md`. The report
lists every page in reading order with its status (translated / skipped / failed), total
page duration, per-stage durations (detection, OCR, inpainting, translation), and the full
error message for failed pages. The HTML report also embeds before/after thumbnails for
each translated page (small base64 JPEGs, click to zoom), so the chapter can be reviewed
without opening the output. It is rewritten after every phase and page, so an interrupted
run still leaves the partial report on disk.

The HTML report is keyboard-navigable: `←`/`→` (or `↑`/`↓`) move the selected page
(highlighted row), `Enter` opens the selected page full screen (press `←`/`→` again to
swap between the original and the translated image, `Escape` or a click closes), and
`+`/`−` resize the thumbnails (`0` restores the default size). The page itself is
self-contained: the only script is this small inline viewer, no external resource.

A header block records the input/output paths, target
language, model and quantization with their VRAM estimate, the GPU used, the start time,
and the total duration — useful to compare chapter runs or spot a regression in timings.

When the GPU exposes NVML telemetry (NVIDIA on Windows/Linux), the report also shows the
**real VRAM usage** next to the estimate: the run's own peak footprint above the GPU usage
observed at startup (desktop and other processes excluded), the whole-GPU peak for
context, and a per-page peak column. That measurement is also persisted to
`~/.koharu/vram-calibration.toml` and feeds back into `--llm auto` and the budget guard
(see *Automatic calibration from real runs*). On the reference 8 GB RTX 3070 the E4B
Q4_K_P run peaks around 6.0–6.4 GiB above the desktop baseline — slightly more than the
static estimate — so real measurements are worth checking before tightening a budget.

Use `--report <BASE>` to choose a different base path (two files `<BASE>.md` and
`<BASE>.html` are written; a volume writes one pair per chapter at `<BASE>-<label>`), or
`--report none` to skip the report entirely.

## French typography profile

The `fr-FR` typography profile normalizes translator output before rendering (both in the
desktop app and in `koharu-batch`):

- Straight quotes `'` and `"` become curly `’` / `« »` where appropriate.
- Space before `;`, `:`, `!`, `?` becomes a narrow no-break space (U+202F); a plain space
  *inside* guillemets becomes a narrow no-break space too.
- Lettering that is stylized on purpose (all-caps words with digits or punctuation like
  `DOOM!!`, `?!`) is left untouched, so sound effects and impact text keep their look.

The profile lives in `koharu-translator/src/typography.rs` and is applied in the translation
stage before the renderer sees the text.

## Translation quality and chapter continuity

The translation stage translates a page as part of its chapter, not as an isolated sheet:

- **Source language** — OCR records the language it recognized (`ja-JP` for manga); the
  translator is told the source language instead of guessing it from the segments.
- **Chapter context** — the last 12 translated lines of the *earlier* pages are passed as
  `context`, so names, places, tone, and recurring phrases stay consistent across page
  boundaries. Untranslated lines and lines longer than 160 characters are left out, and the
  entries themselves are never translated back.
- **No half-translated pages** — a response that omits a segment, returns it empty, or echoes
  it back in the source script is detected, and those segments get one focused follow-up
  request (the rest of the page is never regenerated). The output budget of the constrained
  JSON also scales with the number of segments, so a dense page is no longer truncated
  mid-JSON. Only if the follow-up fails too does a segment keep its source text, and the run
  logs which ones did.

Quality also depends on the model. The 8 GB default (`gemma4-e4b-uncensored`) is chosen for
speed and footprint, not for French prose; on a card with more VRAM, select a larger model
explicitly — `--llm gemma4-12b-it --quantization Q4_K_XL`, or one of the 26B/31B entries in
`--list-models` — and pass `--translation-instructions` with any style or terminology rule the
chapter needs.
