# UltraTasks

予定とタスクを、一つの流れで扱うネイティブアプリ。

一番こだわるのはUIUX。Linearの情報密度と整った見た目、Google系アプリの迷わず触れる操作感を参考に、入力・移動・完了が気持ちよくつながる道具を作る。

最初に作るのはカレンダーとToDo。WindowsとAndroid、RustとGPUI、WebViewなし。ローカルだけで使えて、必要ならセルフホストで同期できる構成を目指す。ドキュメント、カンバン、AI、MCPは、この核の出来栄えを見ながら広げる。

## 現在地

設計とOSS素材の初期基盤。**実行できるアプリはまだない。** 現在はシステムを先に実装する計画を立てている。UIの監修・仕上げはユーザーが後から行い、まず変更しやすい仮画面と性能確認用の最小アニメーションを用意する。GPUIのWindows / Android実機検証、保存・同期・AI接続は未実施。

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

UltraTasks本体の公開・配布ライセンスは未決定。第三者素材にはそれぞれのライセンスが適用される。
