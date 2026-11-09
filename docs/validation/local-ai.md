# 実ローカルモデルによるAI接続検証

2026-11-09。実際のCPU推論を、製品の `ultratasks-ai` → OpenAI互換HTTP → llama.cpp → GGUFモデルの経路で確認した。モデル応答の差し替え・検証の緩和・クラウドAI API・APIキー・課金APIは使っていない。入力とDBは架空データのみ。[当初のJSONモードの記録](local-ai-results.json)と[改良後の厳格なJSON Schemaモードの記録](local-ai-strict-results.json)に、失敗した試行も保存した。

結論: **Qwen2.5-1.5Bで自然文からのタスク提案→内容確認→明示適用→再送→Undoが通った。JSON Schemaを加えると時間割り当ても構造として正しい応答になったが、モデルは作業枠の代わりにリマインダーを提案した。レビューで不適合と判断し、適用しなかった。** 構造の正しさと依頼の意図への一致は別に確認する必要がある。小型モデルの提案品質が安定した、という結果ではない。

## 配布元と実行条件

- llama.cpp: 公式 [ggml-org/llama.cpp](https://github.com/ggml-org/llama.cpp/tree/cd959bae99ff4bed22bef2604f45241c09bbb8c8)、v0.9.0のコミット `cd959bae99ff4bed22bef2604f45241c09bbb8c8`。MITライセンスを確認。ソースアーカイブSHA256は `fbe74146a3cdc00f706773eb233bc4aedb7d8c3c7e2122440e492a99c1ec5a76`。
- 0.5B: 公式 [Qwen/Qwen2.5-0.5B-Instruct-GGUF](https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/tree/9217f5db79a29953eb74d5343926648285ec7e67)、固定版 `9217f5db79a29953eb74d5343926648285ec7e67`、`qwen2.5-0.5b-instruct-q4_k_m.gguf`、491,400,032 bytes。Apache 2.0、SHA256 `74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db` を照合。
- 1.5B: 公式 [Qwen/Qwen2.5-1.5B-Instruct-GGUF](https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/tree/91cad51170dc346986eccefdc2dd33a9da36ead9)、固定版 `91cad51170dc346986eccefdc2dd33a9da36ead9`、`qwen2.5-1.5b-instruct-q4_k_m.gguf`、1,117,320,736 bytes。Apache 2.0、SHA256 `6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e` を照合。
- Linux x86_64、AMD EPYC 9V74、利用可能CPU 4。推論2スレッド、バッチ2スレッド、GPUなし。llama.cppはCMake 3.31.6 / GCC 14.2でRelease、`GGML_NATIVE=ON`、CUDA/Vulkan/組み込みUIなしでビルド。UltraTasks CLIはRust 1.99.1のdebug build。
- コンテキスト16,384、並列スロット1、temperature 0、seed 42、Jinjaテンプレート有効、事前warmup無効。HTTPは `127.0.0.1:18765` だけで待受。アダプターの90秒タイムアウト、JSON形式・範囲・版・保存前プレビューの検証は通常実装のまま。

起動に使った設定:

```sh
llama-server --model MODEL.gguf --host 127.0.0.1 --port 18765 \
  --ctx-size 16384 --parallel 1 --threads 2 --threads-batch 2 \
  --n-gpu-layers 0 --temp 0 --seed 42 --alias MODEL_ALIAS --jinja --no-warmup
```

```json
{
  "provider": "openai_compatible",
  "endpoint": "http://127.0.0.1:18765/v1/chat/completions",
  "model": "Qwen2.5-1.5B-Instruct-Q4_K_M",
  "api_key_env": null,
  "structured_output": true
}
```

モデル・ビルド・検証DBはリポジトリに含めない。切り替え時に0.5Bの重みと自分の一時ビルドツールを除去し、最終的な作業用配置は約1.3GiBに収めた。検証後は両モデルのサーバープロセスを停止した。

現在の既定値は `structured_output: true`。coreのCommand型から導出したJSON Schemaを送り、外部タグ・必須項目・余分なフィールドを生成段階から制約する。初期試行は改良前の `response_format: json_object` を使用していた。現在の実装でその旧動作を明示的に選ぶ場合は `structured_output: false` を指定する。

## 入力と結果

自然文の作成依頼:

> 「牛乳を買う」というタスクを1件だけ作成する提案をしてください。期限や所属リストは設定しないでください。

自然文の時間割り当て依頼。選択したのは、別途作ったダミータスク「資料を確認する」1件のみ:

> 選択したタスクに、2026年11月10日午前9時から30分間、Asia/Tokyo の作業時間を割り当てる提案をしてください。タイトルや期限は変更しないでください。

次の表はJSON Schema導入前の試行である。

| モデル | 入力 | CLI結果 | 提案時の保存変更 |
| --- | --- | --- | --- |
| 0.5B Q4_K_M | 自然文の作成依頼 | コマンド形式不一致で拒否 | なし |
| 0.5B Q4_K_M | 外部タグJSONの形式も明示した作成依頼 | コマンド形式不一致で拒否 | なし |
| 1.5B Q4_K_M | 同じ自然文の作成依頼 | 正しい `CreateTask` 1件 | なし |
| 1.5B Q4_K_M | 自然文の時間割り当て依頼 | コマンド形式不一致で拒否 | なし |
| 1.5B Q4_K_M | 次の明示的な形式を添えた時間割り当て依頼 | 選択IDを使う正しい `ScheduleTask` 1件 | なし |

最後の試行では次の入力を用いた。UTC時刻はこの入力に明示しているため、モデル自身が自然文から時差を正しく計算できた証拠にはしない。

> 選択したタスクに2026年11月10日午前9時から30分間、Asia/Tokyoの作業枠を割り当ててください。commandsにScheduleTaskを1件だけ含める提案です。返答は `{"summary":"説明","commands":[{"ScheduleTask":{"task_id":"選択したタスクのid","expected_task_revision":1,"start":"2026-11-10T00:00:00Z","end":"2026-11-10T00:30:00Z","timezone":"Asia/Tokyo"}}]}` のJSON形式とし、task_idにはselected_recordsの実際のidをそのままコピーしてください。タイトルと期限は変更しません。

0.5Bの失敗を診断した別のローカル要求では、`{"CreateTask":{...}}` の代わりに `{"id":"CreateTask",...}` が返った。アダプターでの形式変換や不足項目の補完は行っていない。

## JSON Schema導入後の再検証

モデル、量子化、CPU設定は同じ1.5Bのまま、別の空DBで、上記の自然文2件をそのまま再実行した。JSON形式やUTC時刻をユーザー入力へ追記していない。更新したシステムプロンプトには、summaryは保存済みと述べず提案を説明する、という指示も加えた。

| 入力 | 構造検証 | 内容のレビュー | 適用 |
| --- | --- | --- | --- |
| 自然文の作成依頼 | 成功。`CreateTask` 1件 | `牛乳を買う`、期限・リストなしで依頼と一致 | 明示適用・再送・Undoまで確認 |
| 自然文の時間割り当て依頼 | 成功。選択ID・revisionも一致 | `ScheduleTask` の代わりに `CreateReminder`、30分前の通知を提案しており不一致 | レビューで拒否。適用していない |

後者の実際のコマンドは `CreateReminder { target: Task(選択ID), trigger: Before { minutes: 30 } }`。summaryも中国語で通知を説明していた。アダプターの構造・版・範囲の検証は通る内容だが、依頼した「09:00〜09:30の作業枠」にはならない。提案ファイルの保存までに留め、`--approve` を呼ばなかった。task・time_block・history・outbox・request receiptは要求前と同じ件数である。

生成Schemaは当初のコマンド形式の不一致に対する改善になった。一方で、この試行はSchemaだけでは操作の意味や説明言語を保証できないことも示した。承認前の内容レビューを省ける根拠にはしない。このSchemaモードの不適合については、その後に入力を調整して再試行せず、最後の結果として保持した。

この実モデル試行後、生成Schemaに空の`commands`による辞退を許可した。ホストは空提案を保存せず拒否する。この追加変更は実HTTP fixtureで確認し、実モデルの再試行はしていない。

## 適用・再送・Undoの照合

作成提案はJSONモード・Schemaモードの両方で、タイトル `牛乳を買う`、期限 `null`、リスト `null` の `CreateTask` 1件であることを確認した。適用前のtask・history・outbox・request receiptはすべて0件。`ai-apply` を承認フラグなしで呼ぶと拒否され、`--approve` を付けて初めて1タスクが保存された。同じ提案の再送は同じIDを返し、行・履歴・送信待ちを増やさなかった。同じactorでUndoすると有効なタスクは0件へ戻った。

JSONモードで形式とUTC時刻を明示して得た時間割り当て提案は、選択したIDとrevision 1だけを対象とすることを確認した。`--approve` 後、SQLite上で `2026-11-10T00:00:00.000000000Z`〜`00:30:00.000000000Z`、`Asia/Tokyo` を照合した。再送で作業枠は増えず、Undo後は有効な作業枠0件、元タスクのタイトル・期限・revisionは変更なしだった。

当初の作成提案のsummaryには、保存前なのに「作成しました」という過去形が含まれた。Schema導入後の追加指示を含む試行では「提案：1件のタスク『牛乳を買う』を作成」になった。いずれも保存前の正しい状態はホストの `applied: false` とDBの0件である。UIはモデルの説明文を保存完了の判定に使わず、ホストの適用状態を示す必要がある。

この記録は接続・提案検証・明示適用・再送・Undoの実動作と、誤った提案をレビューで適用しなかった事実を確認するもの。各要求の秒数は少数の単発実行の記録であり、推論のベンチマークや製品品質の保証ではない。上記以外のモデル、クラウド提供元、自由な自然文での正答率、日本語全般の品質、Windows/Android実機の推論性能は検証していない。
