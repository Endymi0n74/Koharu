---
title: MCP ツールリファレンス
---

# MCP ツールリファレンス

Koharu は次の場所で MCP ツールを公開しています。

```text
http://127.0.0.1:<PORT>/mcp
```

MCP サーバーは `rmcp 1.5` の streamable HTTP transport を使い、GUI および HTTP API と同じプロジェクト、シーン、パイプライン状態に対して動作します。

## 現在の MCP サーバーが公開しているもの

現在の実装は意図的に、プロジェクトのライフサイクル、履歴レイヤー、パイプラインジョブを中心とした小さく低レベルなサーフェスだけを公開しています。フィールド単位の細かな編集は、専用ツールではなく `koharu.apply` に `Op` ペイロードを渡して行います。

ページサムネイル、画像レイヤー、フォント一覧、シーンスナップショットなど、より詳細な検査が必要な場合は [HTTP API](http-api.md) を直接使ってください。両者は同じポートで並走し、同一プロセス内の状態を共有します。

## ツール

| ツール                  | 役割                                                       | パラメータ                                                                       |
| ----------------------- | ---------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `koharu.apply`          | 有効なシーンに `Op` を適用する                             | `op` — JSON タグ付きの `Op` 値                                                   |
| `koharu.undo`           | 直近の op を取り消す                                       | なし                                                                             |
| `koharu.redo`           | 直近に取り消された op を再適用する                         | なし                                                                             |
| `koharu.open_project`   | Koharu プロジェクトディレクトリを開く、または作成する      | `path`、任意で `createName`                                                      |
| `koharu.close_project`  | 現在のプロジェクトを閉じる                                 | なし                                                                             |
| `koharu.start_pipeline` | パイプライン実行を開始し `jobId` を返す                    | `steps[]`、任意で `pages[]`、`targetLanguage`、`systemPrompt`、`defaultFont`     |

### `koharu.apply`

履歴レイヤー経由でシーンに 1 つの変更を適用します。`op` には HTTP API の `POST /history/apply` が受け付けるのと同じ JSON タグ付き `Op` enum を渡します。代表的な variant には `AddPage`、`RemovePage`、`AddNode`、`UpdateNode`、`RemoveNode`、`Batch` があります。

レスポンスは `{ epoch }` で、op 適用後の新しいシーン epoch を返します。

### `koharu.undo` / `koharu.redo`

履歴スタックを 1 ステップずつ進めたり戻したりします。両方とも `{ epoch }` を返し、スタック端 (取り消すまたは再適用するものがない場合) では `epoch` が `null` になります。

### `koharu.open_project`

既存のプロジェクトディレクトリを開くか、指定パスに新しく作成します。`createName` を渡すとそのパス配下に新規プロジェクトを作成し、省略するとそこにある既存のものを開きます。

レスポンスはアクティブになったセッションの `{ name, path }` を返します。

### `koharu.close_project`

現在のセッションを閉じます。これ以降、プロジェクトを必要とする呼び出しは、別のプロジェクトを開くまで `invalid request` エラーを返します。

### `koharu.start_pipeline`

バックグラウンドでパイプライン実行をスポーンします。`steps` はパイプラインの `Registry` に登録された engine id の順序付きリストです (`GET /api/v1/engines` で検証されます)。`pages` を省略するとプロジェクト内全ページが対象になり、`PageId` のリストを渡すと対象を絞り込めます。

レスポンスは即座に `{ jobId }` を返します。進捗と完了は HTTP の `/events` ストリームに `JobStarted`、`JobProgress`、`JobWarning`、`JobFinished` として配信されます。MCP transport 自体はジョブ進捗をストリームしないので、SSE を購読してください。

## 推奨されるエージェントフロー

ほとんどのエージェントセッションは次のような流れになります。

1. `koharu.open_project` — 管理対象のプロジェクトディレクトリを指定する
2. HTTP 経由で `GET /api/v1/scene.json` を読み、シーンを確認する
3. 次のいずれか:
    - 明示的な `Op` ペイロードで `koharu.apply` を呼び、絞り込んだ編集を行う
    - `koharu.start_pipeline` でエンドツーエンドのパイプラインを実行し、`GET /api/v1/events` を監視する
4. HTTP 経由で `POST /api/v1/projects/current/export` から書き出す
5. `koharu.close_project`

`koharu.undo` と `koharu.redo` は、間違った op を適用してしまい、逆 op を手で計算するより取り消した方が早いときに便利です。

## 関連ページ

- [MCP クライアントを設定する](../how-to/configure-mcp-clients.md)
- [GUI / Headless / MCP モードを使う](../how-to/run-gui-headless-and-mcp.md)
- [HTTP API リファレンス](http-api.md)
