---
title: HTTP API リファレンス
---

# HTTP API リファレンス

Koharu は次のローカル HTTP API を公開しています。

```text
http://127.0.0.1:<PORT>/api/v1
```

これはデスクトップ UI と headless Web UI が使っているのと同じ API です。

## ランタイムモデル

現在の実装で重要な挙動は次の通りです。

- API は GUI または headless ランタイムと同じプロセスで提供される
- サーバーは既定で `127.0.0.1` にバインドされる。別ホストに公開したい場合は `--host` を使う
- API と MCP サーバーは同じ読み込み済みプロジェクト、モデル、パイプライン状態を共有する
- `--port` を指定しない場合、Koharu はランダムなローカルポートを選ぶ
- `/api/v1/downloads`、`/api/v1/operations`、`/api/v1/events` を除く全エンドポイントは、アプリのブートストラップが完了するまで `503 Service Unavailable` を返す

## リソースモデル

API はプロジェクト中心です。一度に開けるプロジェクトは 1 つで、次のものを含みます。

- `PageId` でインデックスされた `Pages` のリスト
- `NodeId` で参照されるページごとの `Nodes` (画像レイヤー、マスク、テキストブロック)
- 画像のバイト列を Blake3 ハッシュで保存する content-addressed な `Blob` ストア
- それらから組み立てられる `Scene` スナップショット (`epoch` カウンタで進む)
- undo/redo 可能な `Op` 変更の履歴

すべてのシーン変更は履歴レイヤー (`POST /history/apply`) を経由するため、シーン、自動保存、イベント購読者は常に同期されます。

## よく使うレスポンス型

頻出するレスポンス型には次があります。

- `MetaInfo` — アプリバージョンと ML デバイスラベル
- `EngineCatalog` — パイプライン段階ごとにインストール可能な engine id
- `ProjectSummary` — id、name、path、ページ数、最終オープン日時
- `SceneSnapshot` — `{ epoch, scene }`
- `LlmState` — 現在の LLM 読み込み状態 (status、target、error)
- `LlmCatalog` — ローカルおよびプロバイダのモデルを family ごとにまとめたもの
- `JobSummary` — `{ id, kind, status, error }`
- `DownloadProgress` — パッケージ id、バイト数、ステータス

## エンドポイント

### Meta

| Method | Path        | 目的                                                 |
| ------ | ----------- | ---------------------------------------------------- |
| `GET`  | `/meta`     | アプリバージョンと有効な ML バックエンドを取得する   |
| `GET`  | `/engines`  | 段階ごとに登録済みのパイプライン engine を一覧する   |

### フォント

| Method | Path                                  | 目的                                                          |
| ------ | ------------------------------------- | ------------------------------------------------------------- |
| `GET`  | `/fonts`                              | レンダリング用に system + Google Fonts を統合したカタログ     |
| `GET`  | `/google-fonts`                       | Google Fonts カタログを単体で取得する                         |
| `POST` | `/google-fonts/{family}/fetch`        | Google Fonts の family を 1 つダウンロードしてキャッシュする  |
| `GET`  | `/google-fonts/{family}/{file}`       | キャッシュ済みの TTF/WOFF ファイルを返す                      |

### Projects

すべてのプロジェクトは管理対象の `{data.path}/projects/` 配下に置かれ、クライアントがファイルシステムパスを渡すことはありません。

| Method   | Path                              | 目的                                                              |
| -------- | --------------------------------- | ----------------------------------------------------------------- |
| `GET`    | `/projects`                       | 管理されているプロジェクトを一覧する                              |
| `POST`   | `/projects`                       | 新しいプロジェクトを作成する (body `{ name }`)                    |
| `POST`   | `/projects/import`                | `.khr` アーカイブを新しいディレクトリへ展開して開く               |
| `PUT`    | `/projects/current`               | `id` で管理対象プロジェクトを開く                                 |
| `DELETE` | `/projects/current`               | 現在のセッションを閉じる                                          |
| `POST`   | `/projects/current/export`        | 現在のプロジェクトを書き出す。バイナリを返す                      |

`POST /projects/current/export` は `{ format, pages? }` を受け付け、`format` は `khr`、`psd`、`rendered`、`inpainted` のいずれかです。複数ファイルを生成する形式の場合、レスポンスは `application/zip` になります。

### Pages

| Method | Path                                    | 目的                                                            |
| ------ | --------------------------------------- | --------------------------------------------------------------- |
| `POST` | `/pages`                                | アップロードされた N 枚の画像からページを作る (multipart)       |
| `POST` | `/pages/from-paths`                     | 絶対パスで取り込む Tauri 専用の高速パス                         |
| `POST` | `/pages/{id}/image-layers`              | アップロード画像から Custom 画像ノードを追加する                |
| `PUT`  | `/pages/{id}/masks/{role}`              | 生 PNG バイト列からマスクノードを upsert する                   |
| `GET`  | `/pages/{id}/thumbnail`                 | ページサムネイルを取得する (WebP としてキャッシュ)              |

`role` は `segment` または `brushInpaint` です。`POST /pages` は任意の `replace=true` フィールドを受け付け、取り込みはファイル名を natural order で整列して行われます。

### Scene と blob

| Method | Path                | 目的                                                                   |
| ------ | ------------------- | ---------------------------------------------------------------------- |
| `GET`  | `/scene.json`       | Web/UI クライアント用のフルシーンスナップショット                      |
| `GET`  | `/scene.bin`        | Tauri クライアント用に postcard エンコードされた `Snapshot { epoch, scene }` |
| `GET`  | `/blobs/{hash}`     | Blake3 ハッシュで指定した blob の生バイト                              |

`/scene.bin` は現在の epoch をレスポンスヘッダ `x-koharu-epoch` に含めて返します。

### History (mutations)

シーンの変更はすべてここを通します。各レスポンスは `{ epoch }` を返します。

| Method | Path                | 目的                                              |
| ------ | ------------------- | ------------------------------------------------- |
| `POST` | `/history/apply`    | `Op` を適用する (`Op::Batch` を含む)              |
| `POST` | `/history/undo`     | 最後に適用された op を取り消す                    |
| `POST` | `/history/redo`     | 最後に取り消された op を再適用する                |

`Op` はノードの追加/削除/更新、ページの追加/削除、batch などのシーン遷移を表す discriminated union です。body は JSON タグ付きの variant を渡します。

### Pipelines

| Method | Path          | 目的                                              |
| ------ | ------------- | ------------------------------------------------- |
| `POST` | `/pipelines`  | パイプライン実行を operation として開始する       |

body のフィールド:

- `steps` — 順に実行する engine id (レジストリで検証される)
- `pages` — 任意の `PageId` 部分集合。省略するとプロジェクト全体が対象
- `region` — inpainter 用の任意のバウンディングボックス (repair-brush フロー)
- `targetLanguage`、`systemPrompt`、`defaultFont` — この実行のみの任意上書き

レスポンスは `operationId` を返します。進捗と完了は `/events` から `JobStarted`、`JobProgress`、`JobWarning`、`JobFinished` として届きます。

### Operations

`/operations` は実行中および直近完了したジョブ (パイプラインとダウンロード) を統一的に扱うレジストリです。

| Method   | Path                  | 目的                                                                |
| -------- | --------------------- | ------------------------------------------------------------------- |
| `GET`    | `/operations`         | 実行中または直近の operation 一覧スナップショット                   |
| `DELETE` | `/operations/{id}`    | パイプライン実行をキャンセル。ダウンロードは best-effort で除去     |

### Downloads

| Method | Path                | 目的                                              |
| ------ | ------------------- | ------------------------------------------------- |
| `GET`  | `/downloads`        | 実行中または直近のダウンロードのスナップショット  |
| `POST` | `/downloads`        | モデルパッケージのダウンロードを開始する (`{ modelId }`) |

`modelId` は `declare_hf_model_package!` で宣言されたパッケージ id (例: `"model:comic-text-detector:yolo-v5"`) です。レスポンスはパッケージ id を再利用した `{ operationId }` を返します。

### LLM 制御

ロード済みモデルは `/llm/current` に対するシングルトンリソースとして扱われます。

| Method   | Path             | 目的                                                       |
| -------- | ---------------- | ---------------------------------------------------------- |
| `GET`    | `/llm/current`   | 現在の状態 (status、target、error)                         |
| `PUT`    | `/llm/current`   | 指定 target をロードする (local または provider)           |
| `DELETE` | `/llm/current`   | モデルをアンロード/解放する                                |
| `GET`    | `/llm/catalog`   | ローカルおよびプロバイダのモデルを一覧する                 |

`PUT /llm/current` は `LlmLoadRequest` を受け付けます。

- provider target — `{ kind: "provider", providerId, modelId }`
- local target — `{ kind: "local", modelId }`
- 任意 `options { temperature, maxTokens, customSystemPrompt }`

`PUT /llm/current` はロードタスクをキューに入れた時点で `204` を返します。実際のロード完了状態は `/events` の `LlmLoaded` で通知されます。

### Config

| Method   | Path                                    | 目的                                                       |
| -------- | --------------------------------------- | ---------------------------------------------------------- |
| `GET`    | `/config`                               | 現在の `AppConfig` を取得する                              |
| `PATCH`  | `/config`                               | `ConfigPatch` を適用する。永続化と broadcast を行う        |
| `PUT`    | `/config/providers/{id}/secret`         | プロバイダの API キーを保存 (上書き) する                  |
| `DELETE` | `/config/providers/{id}/secret`         | プロバイダに保存された API キーを削除する                  |

`AppConfig` はトップレベルに `data`、`http`、`pipeline`、`providers` を持ちます。

- `data.path` — ランタイム、モデルキャッシュ、プロジェクトに使うローカルデータディレクトリ
- `http { connectTimeout, readTimeout, maxRetries }` — ダウンロードと provider リクエストで共有される HTTP クライアント
- `pipeline { detector, fontDetector, segmenter, bubbleSegmenter, ocr, translator, inpainter, renderer }` — 各段階で選ばれた engine id
- `providers[] { id, baseUrl?, apiKey? }` — 保存された API キーは生値ではなく、マスク済みプレースホルダ `"[REDACTED]"` で往復する

組み込みの provider id:

- `openai`
- `gemini`
- `claude`
- `deepseek`
- `deepl`
- `google-translate`
- `caiyun`
- `openai-compatible`

API キーはプラットフォームの credential store に保存され、`config.toml` には書き出されません。`apiKey: ""` を PATCH すると保存済みキーが削除され、`"[REDACTED]"` を PATCH するとそのまま維持されます。`/config/providers/{id}/secret` は、PATCH を介さずに 1 プロバイダのシークレットを明示的に管理するための専用ルートです。

## Events stream

Koharu は次の URL で Server-Sent Events を公開しています。

```text
GET /events
```

挙動:

- 新規接続 (`Last-Event-ID` ヘッダなし) では、最初に現在のジョブとダウンロードのレジストリを含む `Snapshot` イベントを送る
- 再接続時は `seq > Last-Event-ID` のバッファ済みイベントを順に再送する。要求された id が ring からスクロールアウトしている場合はサーバーが再度 `Snapshot` を送る
- 各ライブイベントは SSE の `id:` フィールドに `seq` を付けて発行される
- 15 秒ごとに keep-alive を送信する

現在のイベント variant:

- `Snapshot` — 新規接続および遅延回復クライアント用の完全な状態シード
- `JobStarted`、`JobProgress`、`JobWarning`、`JobFinished` — パイプライン job のライフサイクル
- `DownloadProgress` — パッケージダウンロード進捗
- `ConfigChanged` — `PATCH /config` または secret ルートで config が更新された
- `LlmLoaded`、`LlmUnloaded` — LLM ライフサイクル遷移
- `SceneAdvanced` — シーン変更により epoch が進んだときに発行される

## 典型的なワークフロー

新規プロジェクト 1 件分の通常の API 呼び出し順は次の通りです。

1. `POST /projects` — プロジェクトを作成する
2. `POST /pages` (または Tauri からは `/pages/from-paths`) — 画像を取り込む
3. `PUT /llm/current` — 翻訳用モデルをロードする (local または provider)
4. `POST /pipelines` — `detect → ocr → translate → inpaint → render` を開始する
5. `JobFinished` まで `GET /events` を tail する
6. `format = "rendered"` または `"psd"` を指定して `POST /projects/current/export`

より細かく制御したい場合は、フルパイプラインを実行する代わりに、明示的な `Op` ペイロードで `POST /history/apply` を呼びます。

HTTP エンドポイントを順に叩く代わりに、エージェント向けのアクセスが欲しい場合は [MCP ツールリファレンス](mcp-tools.md) を参照してください。
