# ヘッドレス操作・MCP・AI

`ultratasks-automation` は画面を使わず同じ Rust core を操作するホスト。MCP は公式 Rust SDK `rmcp 3.9.0` の stdio transport を使用する。HTTP の MCP 公開サーバーは設けていない。

```bash
cargo build --locked -p ultratasks-automation
mkdir -p local
cargo run --locked -p ultratasks-automation -- --database local/tasks.db init
cargo run --locked -p ultratasks-automation -- --database local/tasks.db tasks --search "買い物"
```

`local/` は Git 管理外。設定、入力プロンプト、提案、出力バックアップにも予定の内容が含まれるため、個人用ファイルはそこかリポジトリ外に保存する。

## 共通コマンドとデータの持ち出し

`command --file PATH --request-id ID` は core の `Command` を JSON 配列で受け取る。ひとつのトランザクション・Undo 単位として適用し、途中の入力不正・版競合では全体を取り消す。同じ actor・request ID・内容の再送は保存済み結果を返し、同じ ID の別内容は拒否する。作成した ID は実行結果で確定するので、同じ入力配列の後続コマンドへ仮の ID を書かない。

```json
[
  {"CreateTask":{"title":"買い物","due":{"Date":"2026-11-09"},"list_id":null}},
  {"CreateCalendar":{"title":"個人"}}
]
```

```bash
ultratasks-automation --database local/tasks.db preview-command --file local/commands.json
ultratasks-automation --database local/tasks.db command --file local/commands.json --request-id purchase-001
ultratasks-automation --database local/tasks.db undo
ultratasks-automation --database local/tasks.db redo
ultratasks-automation --database local/tasks.db calendar --start 2026-11-09T00:00:00+09:00 --end 2026-11-10T00:00:00+09:00 --timezone Asia/Tokyo
ultratasks-automation --database local/tasks.db export-json --output local/export.json
ultratasks-automation --database local/new.db init
ultratasks-automation --database local/new.db import-json --input local/export.json
ultratasks-automation --database local/tasks.db backup --output local/backup.db
ultratasks-automation --database local/restored.db restore --input local/backup.db
```

更新・削除には読み取った `expected_revision` が必須。全コマンド定義は [model.rs](../../crates/ultratasks-core/src/model.rs)。`preview-command` の作成 ID は仮の値であり、本適用時の ID と一致する保証はない。

JSON/ICS import は標準では空のデータベースへ取り込む。`--merge` は完全一致する既存 ID を維持して新しい ID を追加し、衝突した内容を上書きしない。`export-ics` は変換で含められない種類を stderr と結果の `notices` へ明示する。完全な持ち出しには JSON、操作履歴を含む復旧には SQLite online backup を使う。出力ファイル・復元先は新規作成のみ。

## MCP

MCP クライアントの stdio server 設定例。実際にビルドした実行ファイルと既存データベースの絶対パスを指定する。

```json
{
  "mcpServers": {
    "ultratasks": {
      "command": "/absolute/path/ultratasks-automation",
      "args": ["--database", "/absolute/path/tasks.db", "--actor", "mcp-personal", "mcp"]
    }
  }
}
```

標準は読み取り専用。タスク検索、完全なレコード取得、リスト・カレンダー・通知定義の一覧、繰り返しを展開した期間取得を公開する。書き込みを許可する場合は末尾へ `--allow-writes` を加える。変更ごとに安定した `request_id` が必要で、再試行では同じ ID を使う。タスク・予定・作業枠・所属先・繰り返し規則・例外・通知定義の各操作を個別ツールとして公開する。

読み取り専用 MCP は SQLite 自体も read-only 接続とし、起動時のマイグレーションを行わない。古いデータベースを使う場合は先に `init` または通常のアプリで更新する。

ツールから任意の SQL、ファイルパス、接続 URL、シェルを渡せない。stdout は MCP JSON-RPC のみ。受信フレーム 256 KiB、ツール引数 128 KiB、通常クエリは最大 1000 件。繰り返しの上限超過はエラーにし、クライアントへ期間を狭めるよう伝える。通常の作業枠取得は limit 件のページなので、密集した期間では窓を狭めて取得する。

デスクトップとの同時起動は、各プロセスの専用 Engine 接続を SQLite の `IMMEDIATE` トランザクションで直列化する。更新版照合と永続 request ID を通し、外部ホストから SQL を直接書き換えない方式にした。actor は認証情報ではなく履歴・再送の識別子で、デスクトップ・MCP・自動化ホストごとに分ける。MCP の操作を CLI で Undo するときは同じ `--actor` を指定する。接続を許可したローカルプロセスと OS アカウントは信頼境界に含む。

## 内蔵 AI の接続と提案

現時点の AI はヘッドレス API/CLI に接続済みで、GPUI の操作ボタンは未接続。クラウド／ローカルを切り替えられる OpenAI 互換 Chat Completions と Ollama Chat の HTTP アダプターを持つ。設定は特定ベンダーへ固定しない。以下はローカル Ollama の例で、モデルは利用者の環境で用意する。

```json
{
  "provider": "ollama",
  "endpoint": "http://127.0.0.1:11434/api/chat",
  "model": "YOUR_INSTALLED_MODEL",
  "api_key_env": null,
  "structured_output": true
}
```

クラウドの OpenAI 互換サーバーは `provider: "openai_compatible"` と完全な HTTPS Chat Completions URL を指定する。認証情報は `api_key_env` で環境変数名だけを指定する。値を設定ファイル・引数・提案へ保存しない。URL 内の認証情報・クエリ・フラグメントを拒否し、リダイレクトにも追従しない。HTTP はループバックのみ許可する。

`structured_output` は省略時も `true`。Rustの `Command` と同じ型から生成するJSON Schemaを送り、OpenAI互換では `response_format.type: "json_schema"` と `strict: true`、Ollamaでは `format` にスキーマ本体を指定する。全オブジェクトの追加キーを禁止し、任意フィールドもnullを許す必須キーとして指定する。構造化出力に非対応の接続先だけ、設定で明示的に `false` として従来のJSON objectモードを選べる。400等のエラーから自動で制約を外して再送しない。

モデルが安全な操作を提案できない場合のため、生成スキーマは空の `commands` による辞退を許す。空の提案はホストで検証エラーになり、適用できず、データや履歴も変わらない。

```bash
ultratasks-automation --database local/tasks.db --actor ai-personal ai-propose --config local/ai.json --prompt-file local/request.txt --timezone Asia/Tokyo --task TASK_UUID --output local/proposal.json
# proposal.json の summary、scope、commands を確認してから適用する
ultratasks-automation --database local/tasks.db --actor ai-personal ai-apply --plan local/proposal.json --approve
```

送るデータはプロンプトと、`--task / --event / --list / --time-block / --calendar / --reminder` で明示したレコードだけ。対象を一つも指定しなければ既存の予定は送らない。現在時刻と指定 IANA タイムゾーンも送る。選択は最大 100 件、コンテキスト 128 KiB、応答 1 MiB、操作は最大 100 件。スキーマに従った応答でも信頼せず、Rustで構文・日付・ID・操作範囲・更新版・関連を再検証し、ロールバックされるプレビューを通す。モデルが対象外の ID を使う提案は拒否する。

提案生成は保存済みデータを変更しない。`ai-apply --approve` が適用点で、選択した全レコードの版をトランザクション内で照合する。文脈として読んだだけの予定が変わっても古い提案の適用を拒否する。削除で連鎖的に変更する作業枠・通知も選択に含める必要があり、提案の後で新しい従属レコードが追加された場合も一括して拒否する。成功後の再送は同じ結果を返す。適用は永続 Undo の対象で、元データ内の文章をモデルへの指示として扱わないようプロンプトを分離する。

AI はタスク・リスト・カレンダー・予定・作業枠・通知の作成／変更／削除、タスク完了、繰り返し規則・単発例外を提案できる。新規 ID の生成後に別の操作でその ID を参照する場合は、適用結果を読み取って次の提案を作る。外部の実モデルでは未検証。テストはローカル HTTP fixture を使い、送信範囲・応答検証・自動適用しないこと・手動適用・Undo・競合・再送を確認する。

時間割り当ての提案が考慮する予定も、選択したデータに限る。選択外の予定や、その後に新規追加された予定との重なりを自動で全面禁止する機能ではない。core は重なった予定の保存も許容するため、提案は適用前に確認する。

```bash
cargo test --locked -p ultratasks-automation
```
