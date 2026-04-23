---
title: MCP クライアントを設定する
---

# MCP クライアントを設定する

Koharu はローカルの Streamable HTTP 上で組み込み MCP サーバーを公開します。このページでは、それに MCP クライアントを接続する方法を説明します。対象は Antigravity、Claude Desktop、Claude Code です。

## Koharu が MCP 経由で公開しているもの

Koharu の MCP サーバーは、デスクトップアプリや headless Web UI と同じローカルランタイムを使います。現在のツールサーフェスは意図的に小さく、プロジェクトのライフサイクル、履歴レイヤー、パイプラインジョブを中心に絞り込まれています。

- `koharu.open_project` / `koharu.close_project`
- `koharu.apply` / `koharu.undo` / `koharu.redo`
- `koharu.start_pipeline`

シーンスナップショット、ページサムネイル、blob 取得、フォント一覧、LLM 制御、export、設定など、より詳細な検査や編集が必要な場合、エージェントは `http://127.0.0.1:<PORT>/api/v1` にある Koharu の HTTP API を直接呼び出します。HTTP API と MCP サーバーは同一プロセス、同一状態を共有するので、エージェントは 1 つのワークフローの中で両者を自由に組み合わせられます。

ツールの全リストとパラメータスキーマは [MCP ツールリファレンス](../reference/mcp-tools.md) を参照してください。

## 1. 安定したポートで Koharu を起動する

MCP クライアントから毎回同じ URL でアクセスできるよう、固定ポートを使います。

```bash
# macOS / Linux
koharu --port 9999 --headless

# Windows
koharu.exe --port 9999 --headless
```

デスクトップウィンドウを残したまま MCP を公開することもできます。

```bash
# macOS / Linux
koharu --port 9999

# Windows
koharu.exe --port 9999
```

このとき、Koharu の MCP エンドポイントは次になります。

```text
http://127.0.0.1:9999/mcp
```

重要な点:

- MCP クライアント接続中は Koharu を起動したままにする
- Koharu は既定で `127.0.0.1` にバインドされるため、ここでの例は同じマシン上のクライアントを前提としている
- 既定のローカル構成では認証ヘッダは不要

## 2. エンドポイントを手早く確認する

クライアント設定を書く前に、まず Koharu が想定ポートで本当に動いているか確認してください。

次を開きます。

```text
http://127.0.0.1:9999/
```

Web UI が表示されるならローカルサーバーは起動しており、MCP エンドポイントも `/mcp` に存在するはずです。

## Antigravity

Antigravity では raw MCP config を使って、Koharu のローカル MCP URL を直接指定できます。

### 手順

1. `--port 9999` 付きで Koharu を起動します。
2. Antigravity を開きます。
3. エディタ上部のエージェントパネルにある `...` メニューを開きます。
4. **Manage MCP Servers** をクリックします。
5. **View raw config** をクリックします。
6. `mcpServers` の下に `koharu` エントリを追加します。
7. 設定を保存します。
8. 自動再読み込みされない場合は Antigravity を再起動します。

### 設定例

```json
{
  "mcpServers": {
    "koharu": {
      "serverUrl": "http://127.0.0.1:9999/mcp"
    }
  }
}
```

すでに他の MCP サーバーを設定している場合は、`mcpServers` 全体を置き換えるのではなく、その横に `koharu` を追加してください。

### 設定後に試すこと

まずは簡単な質問から始めるのが安全です。

- `What Koharu MCP tools do you have available?`
- `Open the Koharu project at C:\\projects\\my-manga.khrproj.`

動作したら、次のような実作業に進めます。

- `Open the project at C:\\projects\\my-manga.khrproj and start a pipeline with steps detect, ocr, llm-translate, aot-inpainting, koharu-renderer.`
- `Undo the last edit in Koharu.`
- `Apply this Op to add a new text block to page <id>: { ... }`

## Claude Desktop

Claude Desktop の現在のローカル MCP 設定は command ベースです。Koharu はローカル HTTP MCP エンドポイントを公開しており、デスクトップ拡張の形ではないため、実用上は `http://127.0.0.1:9999/mcp` に接続する小さなブリッジプロセスを挟む方法になります。

このガイドでは、そのブリッジとして `mcp-remote` を使います。

### 始める前に

次のどちらかを満たしてください。

- すでに `npx` が使える
- Node.js が入っていて `npx` を実行できる

### 手順

1. `--port 9999` 付きで Koharu を起動します。
2. Claude Desktop を開きます。
3. **Settings** を開きます。
4. **Developer** セクションを開きます。
5. Claude Desktop 内のエディタ導線から MCP 設定ファイルを開きます。
6. `koharu` サーバーエントリを追加します。
7. 保存します。
8. Claude Desktop を完全に再起動します。

### Windows 用設定

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

### macOS / Linux 用設定

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

補足:

- すでに `mcpServers` に他の設定がある場合は、それを消さずに `koharu` を追加してください
- `mcp-remote@latest` は初回利用時に取得されるため、最初の起動時にはインターネット接続が必要になる場合があります
- Windows で Node.js が `C:\\Program Files\\nodejs` 以外に入っている場合は、`command` のパスを実環境に合わせて変えてください
- Anthropic の現行の remote-MCP connector 導線は **Settings > Connectors** にありますが、このページではあくまで Koharu のローカル `127.0.0.1` エンドポイントに接続するための設定ファイル経由のブリッジ構成を扱っています

### 設定後に試すこと

Claude Desktop で新しいチャットを開き、次のように聞いてください。

- `What Koharu MCP tools do you have available?`
- `Open the Koharu project at D:\\projects\\my-manga.khrproj.`

その後、実際の作業に進みます。

- `Run a Koharu pipeline with steps detect, ocr, llm-translate, aot-inpainting, koharu-renderer on the project I just opened.`
- `Use Koharu's HTTP API at http://127.0.0.1:9999/api/v1/operations to check pipeline status.`
- `Use Koharu's HTTP API to export the project as PSD.`

## Claude Code

ここでいう「Claude」が Claude Code を指しているなら、Koharu のローカル `http://127.0.0.1` MCP エンドポイントには、同じ stdio ブリッジ方式を使うのが最も安全です。

### ユーザー設定に追加する

macOS / Linux:

```bash
claude mcp add-json koharu "{\"type\":\"stdio\",\"command\":\"npx\",\"args\":[\"-y\",\"mcp-remote@latest\",\"http://127.0.0.1:9999/mcp\"],\"env\":{}}" --scope user
```

これで、Claude Code のユーザーアカウント用 MCP 設定にサーバーが追加されます。

Windows:

```bash
claude mcp add-json koharu "{\"type\":\"stdio\",\"command\":\"cmd\",\"args\":[\"/c\",\"npx\",\"-y\",\"mcp-remote@latest\",\"http://127.0.0.1:9999/mcp\"],\"env\":{}}" --scope user
```

ネイティブ Windows では、`npx` を使うローカル stdio MCP サーバーには `cmd /c npx` ラッパーを使うことが Claude Code 側でも推奨されています。

### 確認する

```bash
claude mcp get koharu
claude mcp list
```

すでに Claude Desktop 側に Koharu を設定している場合、対応プラットフォームでは Claude Code が Claude Desktop の互換エントリを読み込むこともできます。

```bash
claude mcp add-from-claude-desktop --scope user
```

## 最初に試すとよいタスク

クライアントが接続できたら、次のあたりが取り掛かりに向いています。

- どの Koharu MCP ツールが使えるかをエージェントに尋ねる
- 既存の Koharu プロジェクトディレクトリを開く
- 小さな step リスト (例: `detect`、`ocr`) でパイプラインを開始する
- フルパイプラインを走らせる前に、エージェントに HTTP 経由で `GET /api/v1/scene.json` を読ませて結果を確認させる

小さな MCP ツールサーフェスと直接の HTTP 呼び出しを併用するのは意図した設計です。プロトコル面のサーフェスは最小限のまま、エージェントはエディタの完全な状態にアクセスできます。

## よくある間違い

- `--port` なしで Koharu を起動し、誤ったポートにクライアントを向ける
- `http://127.0.0.1:9999/mcp` ではなく `http://127.0.0.1:9999/` を使う
- クライアント設定後に Koharu を閉じてしまう
- 設定全体を置き換えてしまい、新しい `koharu` エントリをマージしていない
- Claude Desktop が plain な command-less 設定だけで Koharu の HTTP URL に直接つながると思う
- Koharu の既定ローカルサーバーは同じマシンからしか届かない点を忘れる

## 関連ページ

- [GUI / Headless / MCP モードを使う](run-gui-headless-and-mcp.md)
- [MCP ツールリファレンス](../reference/mcp-tools.md)
- [CLI リファレンス](../reference/cli.md)
- [トラブルシューティング](troubleshooting.md)

## 外部参照

- [Claude Code MCP docs](https://code.claude.com/docs/en/mcp)
- [Claude Help: Building custom connectors via remote MCP servers](https://support.claude.com/en/articles/11503834-building-custom-connectors-via-remote-mcp-servers)
- [Wolfram support article with current Antigravity and Claude Desktop MCP config examples](https://support.wolfram.com/73463/)
