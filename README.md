# UltraTasks

予定とタスクを、一つの流れで扱うネイティブアプリ。

一番こだわるのはUIUX。Linearの情報密度と整った見た目、Google系アプリの迷わず触れる操作感を参考に、入力・移動・完了が気持ちよくつながる道具を作る。

最初に作るのはカレンダーとToDo。WindowsとAndroid、RustとGPUI、WebViewなし。ローカルだけで使えて、任意のセルフホストサーバーと同期する。ドキュメント・カンバンは後から検討する。

## 現在地

**UIと独立したカレンダー・ToDoのシステム、CLI、GPUI仮画面、Androidのネイティブシステムホストを実装した段階。** UIの仕上げはユーザーが監修する。追加したシステム機能は共通Rust APIとCLI・JNIから利用でき、すべての設定・編集画面が揃った完成品ではない。

| システム | 実装 |
| --- | --- |
| 保存・編集 | SQLite、型付きID、関連整合性、原子的バッチ、永続Undo/Redo、重複要求の排除、読み取り専用接続 |
| カレンダー・ToDo | 所属先、期限検索、日次・週次・月次、今回だけ変更・取消・完了、タイムゾーンと夏時間 |
| データ持ち出し | 完全JSON、対応範囲を明示したICS、オンラインバックアップ、新規保存先への検証付き復元 |
| 同期 | オフライン送信待ち、端末認証と失効、HTTPS、競合の保存・解決、サーバー復旧検知、CLI常駐処理 |
| 通知 | 共通計算、Windows予約通知・任意の定期ワーカー、Android AlarmManager・再起動対応 |
| MCP・AI | 公式MCP SDKのstdio接続、選択データ限定のAI提案、承認・版照合・変更範囲検査を経た適用 |

実際のHTTP同期・MCP別プロセス・JVM/JNI・保存と復旧を検証する。Windows通知、Android上のTLS・Doze・権限操作は実機未検証。AndroidのGPUI描画・入力の接続は[互換性調査](docs/android-spike.md)にある課題が残る。外部クラウドAIの資格情報は設定していない。[検証記録](docs/validation/README.md)で実行済みと未検証を分ける。

```sh
cargo test --locked --workspace --exclude ultratasks-desktop --all-targets
cargo run --locked -p ultratasks-desktop
cargo run --locked -p ultratasks-automation -- --database local.db init
```

- [ビルド・起動・テスト・性能計測](docs/development.md)
- [初期実装の検証記録・実画面・計測結果](docs/validation/README.md)
- [コアの操作APIと制約](crates/ultratasks-core/README.md)
- [CLI・MCP・AIの使い方](apps/automation/README.md)
- [セルフホスト同期・認証・復旧](apps/server/README.md)
- [JSON・ICS・バックアップ](docs/data-portability.md)
- [通知の仕組みとOS上の条件](docs/notifications.md)
- [Androidホストのビルド](docs/android-system.md)・[JNI操作API](docs/android-system-api.md)
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

Rust 2024 / Rust 1.99.1、GPUI Kit 0.9.0、SQLite（rusqlite）、chrono / chrono-tz、UUID v7、Tokio・Axum・reqwest/rustls、公式rmcp、JNI、serde、proptestを採用。依存はCargo.lock、Androidの外部成果物はSHA-256付きlockへ固定する。コアの検証には故障注入・更新競合・操作列のプロパティテストを含む。

初期同期プロトコルの容量上限は削除記録込み10万レコード・ドメインJSON約32MiB。履歴・同期ログ等のディスク使用量とは別の上限で、超える変更は原子的に拒否する。保持済みの削除記録を捨てて古い端末のデータを復活させる実装にはしない。

UltraTasks本体の公開・配布ライセンスは未決定。第三者素材・ライブラリにはそれぞれのライセンスが適用される。
