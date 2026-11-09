# 開発と検証

## 構成

- `crates/ultratasks-core`: UI・ネットワークから独立した操作APIとSQLite。Task、TaskList、Event、TimeBlock、検索、更新版の照合、Undo/Redoを担当する。
- `apps/desktop/src/backend.rs`: SQLiteを所有する専用スレッド。GPUIからの操作を順番に処理する。
- `apps/desktop/src/views/`: 仮画面と入力イベント。レイアウト、タスク一覧、編集欄、予定一覧を別ファイルに分け、追加・編集・完了・削除、時間割り当て、日付切替、Undo/Redoを接続する。
- `apps/desktop/src/presentation.rs`: 表示時刻の解釈。夏時間で存在しない／二通りになる時刻を曖昧なまま保存しない。
- `apps/desktop/src/theme.rs` と `design/tokens.json`: 色・余白・モーションを調整する入口。現在の仮画面はダークテーマを使用。
- `crates/ultratasks-sync` と `apps/server`: 認証・HTTP同期、端末用CLIとセルフホストサーバー。Androidではサーバー・CLI機能を外した依存構成を使う。
- `crates/ultratasks-notifications`: 共通の通知計画、端末別の配信台帳、WindowsのOS通知と定期処理。
- `crates/ultratasks-ai` と `apps/automation`: AI提案・適用、公式SDKのMCP、画面なしの操作・入出力CLI。
- `apps/android`: Rust JNI、OS通知・権限・TLS・暗号化した資格情報保存・バックグラウンド同期。製品のGPUI画面とは分離したシステムホスト。

コアのデータ構造に画面レイアウトを含めない。UIを編集する場合は、操作APIの意味と保存テストを維持したまま`apps/desktop`側を変更する。

## ビルド

Rustは`rust-toolchain.toml`、依存は`Cargo.lock`へ固定する。WindowsではRustのMSVC toolchainを利用できるC++ビルドツールとWindows SDKが必要になる。GitHub ActionsではWindowsのビルドと、Windows / Linuxのコアテストを実行する。

```sh
cargo test --locked --workspace --exclude ultratasks-desktop --all-targets
cargo run --locked -p ultratasks-desktop
```

通常起動ではOSのユーザー用アプリデータディレクトリへ保存する。検証用のDBを分けたい場合は明示する。

```sh
cargo run --locked -p ultratasks-desktop -- --database local/test.db --timezone Asia/Tokyo
cargo run --locked -p ultratasks-desktop -- --database local/test.db --reduce-motion
```

`--timezone`は表示・入力用のIANAタイムゾーン。省略時はOSの設定を取得し、取得できない場合はUTCを使用する。時刻付き予定はUTCの瞬間とIANAタイムゾーン、終日は日付として保持する。`--reduce-motion`は仮アニメーションを無効化する。画面上の切替でも同じ比較ができる。

LinuxでのGUI検証には、GPUIのX11 / Wayland / fontconfig / xkbcommon / Vulkan等の開発・実行ライブラリが必要。環境固有のパスや資格情報はリポジトリへ含めない。XvfbとソフトウェアVulkanによる起動は機能検証であり、WindowsやAndroid実機の性能測定ではない。

## 品質チェック

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --exclude ultratasks-desktop --all-targets -- -D warnings
cargo test --locked --workspace --exclude ultratasks-desktop --all-targets
cargo clippy --locked -p ultratasks-desktop --all-targets -- -D warnings
cargo test --locked -p ultratasks-desktop
python scripts/check_native_dependencies.py
bash apps/android/test-jni.sh
```

コアの検証には、再起動後の復元、異なる編集元の競合、更新版の巻き戻り防止、途中失敗の原子的なロールバック、日時境界、関連の整合性、ランダムな操作列と参照モデルの比較を含む。HTTPテストは実際のループバックTCP、MCPはstdin/stdoutの別プロセス、JNIは実JVMを利用する。AIのHTTP結合テストはローカルfixtureであり、モデルの生成品質評価とは区別する。

WebView検査は選択された依存グラフのパッケージ名を調べる。実装や最終バイナリの全面的な監査を代替するものではない。`tauri-winrt-notification`はOS通知のライブラリなので、TauriのWebViewランタイムとは区別する。

## コアの計測

```sh
cargo run --locked --release -p ultratasks-core --example measure_core -- 1000
cargo run --locked --release -p ultratasks-core --example measure_core -- 10000
cargo run --locked --release -p ultratasks-core --example measure_recurrence
```

一時ファイルのSQLiteへ固定の内容を作り、更新と問い合わせの分布をJSONで出力する。データ件数、ビルド種別、OS、保存先、端末条件と一緒に評価する。毎回新しいDBを作るため、ユーザーのデータには触れない。クラウド環境での計測値をユーザーの実機性能として扱わない。

## データと履歴

- SQLiteを正とし、更新と操作履歴を同じトランザクションに保存する。
- 更新には取得時のrevisionを渡す。古い値なら競合として返す。
- Undo/Redoでもrevisionは増加する。後から行われた他の編集を無条件に巻き戻さない。
- タスク削除時は関連する作業枠も同じ操作で削除状態にし、Undoで一緒に復元する。完了は作業枠の削除を意味しない。
- 編集元ごとに直近1000操作の履歴を保持する。履歴の編集元IDは認証・権限の代わりにはならない。
- 取得件数とカレンダーの期間を制限する。現在の仮画面はタスクをページ単位で読み、見える行を描画する。

## 現在の範囲

システム機能はRust APIとCLI・JNIへ実装している。仮画面はタスクと作業枠が中心で、全APIの編集画面はまだ揃えていない。

[CLI・MCP・AI](../apps/automation/README.md)、[同期サーバーと復旧](../apps/server/README.md)、[入出力](data-portability.md)、[通知](notifications.md)、[Androidホスト](android-system.md)を参照する。Windowsの実通知、Android実機でのTLS・Doze・権限・バックグラウンド動作は未検証。AndroidのGPUI描画は依存の整合と入力・ライフサイクルの検証が残る。

初期プロトコルは10万レコード（削除記録を含む）・ドメインJSON約32MiBまで。超過する変更は保存・履歴・同期送信待ちを一緒にロールバックする。起動と復元時には容量管理用の索引も実レコードと照合する。JNIホストは通常操作の接続をキャッシュし、毎回の操作で全DBの起動検査を繰り返さない。
