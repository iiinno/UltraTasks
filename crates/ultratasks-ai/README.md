# 共通 AI アダプター

GPUI・CLI・Android から再利用できる提案と適用の Rust API。プロバイダーは OpenAI 互換 Chat Completions または Ollama Chat。`Provider::propose` は選択データと明示した `PlanningClock` を送り、検証済み `Plan` を返す。モデルの応答から自動で変更しない。

`ProviderConfig.structured_output` は省略時にtrue。`schemars`でcoreの `Command` 型から生成したJSON Schemaを、OpenAI互換のstrict JSON Schema／Ollamaのschema formatとして渡す。任意フィールドはnull可能な必須キーにし、追加フィールドを禁止する。`proposal_schema()` で同じスキーマを取得できる。古い接続先では明示的な `structured_output: false` によってJSON objectモードを選択できるが、HTTPエラー後に自動で制約を解除しない。どちらでもcoreの検証・範囲制限・承認・Undoは同じ。

生成スキーマは空の `commands` を許し、要求を安全に表現できないモデルが変更を提案せず辞退できるようにする。空の提案をホストが適用することはなく、検証エラーとして返し、データ・履歴を変更しない。

CLI は `Provider::new` で環境変数名から認証情報を読む。ネイティブホストは OS の資格情報ストアから取得した値を `Provider::with_api_key` へメモリ上で渡せる。後者では `ProviderConfig.api_key_env` は `None` にし、認証情報を設定や提案へ含めない。`Provider` は `Debug` を実装しない。

ユーザーが内容を確認した後に `apply_plan` を呼ぶ。選択 ID と更新版、連鎖して変更する対象範囲をトランザクション内で照合し、永続的なリクエスト ID で再送を処理する。API 自体は UI の承認状態を持たないため、ホストはユーザーの適用操作からだけ呼び出す。

詳細と実行例は [automation の説明](../../apps/automation/README.md)。結合テストは `cargo test -p ultratasks-automation` にあり、実モデルへの送信は行わずローカル HTTP fixture で検証する。
