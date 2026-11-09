# AndroidのシステムAPI

`apps/android/native`は共有Rust coreを呼ぶJNIホスト。GPUI画面を持たないシステム層で、WebViewは使わない。Android側の通知処理も同じDBと操作APIを使う。

Javaの`NativeBridge.dispatch(databasePath, requestJson)`は編集可能な接続、`dispatchReadOnly(databasePath, requestJson)`は読み取り専用の接続。接続権限はJava側の呼び出しで選び、JSONから変更できない。DBパスはアプリのホストが決める。Intent等の外部入力からそのまま渡さない。

呼び出しはメインスレッドを避ける。プロセス内で一つのEngineをキャッシュし、同じDB・権限のローカル操作はMutexで直列化する。毎回DB全体の検証やマイグレーションを繰り返さない。DBまたは権限を切り替えると接続を作り直す。ネットワーク処理は別の接続を使い、通信待ちの間はローカル操作用Mutexを保持しない。

読み取り専用接続はSQLiteの`READ_ONLY`と`query_only`を使う。存在しないDBの作成、旧スキーマの移行、編集・Undo・インポート、バックアップファイル作成、ネットワーク操作を許可しない。

## ローカル要求

すべて`type`で種類を指定するJSONオブジェクト。未知のフィールドを拒否し、成功時は`ok: true`、失敗時は`ok: false`と`error`を返す。

| type | 追加フィールド | 結果 |
| --- | --- | --- |
| `execute` | `request_id`, `commands` | 原子的な共有コマンドと永続的な重複排除。`outcomes` |
| `snapshot` | なし | 削除記録を含む`records` |
| `history` | なし | 接続の履歴ストリームの`history` |
| `undo` / `redo` | なし | 変更したIDの`changed` |
| `export` | なし | 版付きJSONオブジェクトの`data` |
| `import` | `data`、任意の`require_empty` | 取り込み件数の`report` |
| `export_ics` | なし | ICS文字列の`data`と対象外情報の`notices` |
| `import_ics` | ICS文字列の`data`、任意の`require_empty` | `report` |
| `backup` | `filename` | 新しく作ったバックアップの`filename` |
| `restore` | 元の`filename`、新しい`destination` | 復元した新しいDBの`filename` |
| `plan` | `timezone`, `enabled`、任意の`missed_grace_minutes` | OS通知へ渡す`notifications` |

`require_empty: true`は空DBにだけ取り込む。省略時は既存の同一レコードを無視し、新しいIDだけを追加する。既存IDの内容が異なる場合は全体を失敗させる。JSON・ICSの対応範囲は[入出力](data-portability.md)を参照。

バックアップ名はDBと同じディレクトリ内の単一ファイル名に限定する。1〜128バイト、先頭は英数字、以降は英数字・`.`・`_`・`-`、末尾は`.sqlite3`。ディレクトリ移動、絶対パス、シンボリックリンクを拒否する。復元は新しいファイルへ行い、現在開いているDBへの上書きや自動切り替えはしない。

入力はUTF-8換算で32 MiB＋要求オブジェクト用64 KiBまで、出力は64 MiBまで。ホストのファイル読み込みやUIにも上限を設ける。これらは大量データを一つのJNI文字列として渡すときの上限であり、通常の画面操作で全レコードを毎回取得する設計にはしない。

## 同期とAI

`{"type":"network","request":{...}}`で呼び出す。内側の`operation`は`sync`、`sync_status`、`sync_conflict`、`resolve_conflict`、`recover_server`、`ai_propose`、`ai_apply`。正確な型は[network.rs](../apps/android/native/src/network.rs)に定義している。

同期先はループバック以外でHTTPSを必須とする。認証情報は呼び出し元が管理し、DBやAI提案へ保存しない。競合解決・サーバー復旧後の選択・AI提案の適用は`approved: true`を明示して呼ぶ。AI提案は自動適用せず、共有コマンドの更新版・対象範囲の検証を通して適用する。

Javaの`SystemController`はHTTPSの同期先と認証情報の名前を、一つの設定スナップショットと世代番号で管理する。変更・無効化後に、待ち行列に残った古い設定の要求を開始しない。設定保存とJobScheduler登録の結果を区別し、登録に失敗した場合は`background_schedule_failed`を残す。設定は手動同期に使用できる。

Java側の通信処理は2ワーカーと最大8件の待ち行列に制限する。ネットワーク要求はコピー時点で1 MiB・深さ32・20,000要素を上限とし、呼び出し元が後からJSONを書き換えても送信内容を変えられない。同期・競合解決・サーバー復旧は同時に一つだけ実行する。

`CredentialStore`は秘密値をAES-GCMで暗号化し、アプリ専用のSharedPreferencesへ保存する。AndroidKeyStoreに保存するのはエクスポートできないAES鍵であり、APIキーそのものではない。認証情報の名前を追加認証データに含め、名前を入れ替えた暗号文を拒否する。秘密値はUTF-8で8,192バイトまで。通信時だけ復号した値をRustへ渡す。

JobServiceはOSからの停止・別の実行への置き換え・サービス終了を識別する。停止した実行の遅いコールバックは、新しい実行を完了させたり状態を書き換えたりしない。Javaで待機中の要求は取り消せるが、JavaのFuture取消で実行中のJNIが中断するとは扱わない。すでに開始した要求はネイティブ側の時間制限内で完了しうる。途中でプロセスが終了した場合は永続的な操作IDで再送する。

## 検証範囲

[test-jni.sh](../apps/android/test-jni.sh)はLinuxホストの実際のJVMからJNIの共有ライブラリを呼ぶ。[BridgeSmoke.java](../apps/android/tests/BridgeSmoke.java)で保存・重複排除・Undo/Redo・JSON/ICS往復・バックアップ/復元・パス制限・読み取り専用・入力上限・並列要求・HTTPS要求・AIの適用承認を確認する。

[JobRunStateTest.java](../apps/android/tests/JobRunStateTest.java)では、実際にJobServiceが使う実行状態の処理をAndroidフレームワークから独立してテストする。停止後の完了、同じJob IDの置き換え、停止後のFuture登録、重複した完了、サービス終了を固定順序で再現する。

これはAndroidの実機テストではない。Androidの通知権限、Doze、再起動、GPUI描画、日本語IME、ライフサイクル、TLS初期化を含む実機動作は別に確認する。
