# UltraTasks

予定とタスクを、一つの流れで扱うネイティブアプリ。

一番こだわるのはUIUX。Linearの情報密度と整った見た目、Google系アプリの迷わず触れる操作感を参考に、入力・移動・完了が気持ちよくつながる道具を作る。

最初に作るのはカレンダーとToDo。WindowsとAndroid、RustとGPUI、WebViewなし。ローカルだけで使えて、必要ならセルフホストで同期できる構成を目指す。ドキュメント、カンバン、AI、MCPは、この核の出来栄えを見ながら広げる。

## 現在地

**ローカルで動くRustコアとGPUIの仮アプリを実装した段階。** タスク・リスト・予定・作業枠の操作API、SQLite保存、更新競合の検査、永続Undo/Redoを備える。仮画面ではタスクの追加・編集・完了・削除、時間割り当て、Undo/Redoを操作できる。UIの仕上げはユーザーが後から監修する。

Linuxのネイティブ画面で日本語の貼り付け・保存と一連の操作を検証した。Windows実機でのIME・読み上げ・性能、Androidのビルド・実機、同期・AI接続は未検証または未実装。繰り返し・入出力・通知などは後続段階とする。

```sh
cargo test --locked -p ultratasks-core --all-targets
cargo run --locked -p ultratasks-desktop
```

- [ビルド・起動・テスト・性能計測](docs/development.md)
- [初期実装の検証記録・実画面・計測結果](docs/validation/README.md)
- [コアの操作APIと制約](crates/ultratasks-core/README.md)
- [Androidの具体的な依存検証](docs/android-spike.md)
- [システム実装計画・決定事項・完了条件](docs/implementation-plan.md)
- [UIの方向性と制作手順](docs/ui-direction.md)
- [画面コンセプト01・02](design/concepts/README.md)
- [OSSの比較、コードの参照先、部品の品質基準](docs/design-system-research.md)
- [構成案とWindows / Androidの成立性検証](docs/architecture.md)
- [配色プレビュー](design/palette-preview.svg)
- [共通トークン](design/tokens.json)、[取り込んだ色](design/palettes.json)、[アイコン](design/icons)
- [素材の出典・固定コミット](design/upstream-lock.json)、[ライセンス通知](THIRD_PARTY_NOTICES.md)

## 作り方

既存の良い部品を使い、UltraTasks固有の体験に開発時間を使う。GPUI Kitを第一候補として入力・メニュー等を評価し、Radixの色とLucideのアイコンを共通化する。React Ariaは日付操作の状態遷移を読む。採用候補と、実際に取り込んだ素材は分けて記録する。

画面なしで動くRustコア → ローカル保存と操作API → 日付・繰り返し・入出力 → OS連携と性能検証 → 自分の複数端末同期 → MCP・AIの順で進める。簡単なGPUI画面は初期からコアへ接続し、Androidの入力・描画も早期に検証する。UIの完成をシステム実装の前提にしない。

初期対象は個人用。外部カレンダーの常時連携は後回しとし、自分の同期サーバーはデータを読める前提で認証と通信保護を実装する。

素材の再取得はPython 3とGitHub CLIで実行する。既存のlockにあるコミットを使う。

```sh
python3 scripts/import_design_assets.py
python3 scripts/build_design_preview.py
```

Rust 2024 / Rust 1.99.1、GPUI Kit 0.9.0、SQLite（rusqlite）、chrono / chrono-tz、UUID v7、serde、proptestを採用。依存はCargo.lockへ固定する。コアの検証には故障注入・更新競合・操作列のプロパティテストを含む。

UltraTasks本体の公開・配布ライセンスは未決定。第三者素材・ライブラリにはそれぞれのライセンスが適用される。
