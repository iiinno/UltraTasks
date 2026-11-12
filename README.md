# UltraTasks

予定とタスクを、一つの流れで扱うネイティブアプリ。

UIUXを重視し、Linearの情報密度と整った見た目、Google系アプリの迷わず触れる操作感を参考に、入力・移動・完了がつながる道具を作る。

現在はWindows PC向けプロトタイプを実装している。Rustの画面非依存coreとGPUI Kitのdesktopの2クレートで、SQLiteへローカル保存する。WebViewは使用しない。Android、同期、繰り返し、通知、入出力、外部カレンダー、AI、MCPは後続の段階とする。

## 現在地

動くWindows基準版を固め、その上に小さい改善PRを並行して作る。[基準版の範囲とPR運用](docs/windows-baseline.md)に合格条件とbase/headの扱いを記載した。各変更の合否は、そのSHAのCIと実画面確認で判断する。

Task・Event・TimeBlock・リスト・カレンダーの編集、Undo/Redo、検索と絞り込み、日付移動、時刻指定と最小ドラッグを実装し、正式テーマとcoreを接続した。タスク一覧は可視行を描画し、保存は専用DBスレッドで処理する。テーマはsystem/light/dark、motionはsystem/reduced/noneを選べる。

UI改修版 `05c8fc2` は[CI run 43127481310](https://github.com/iiinno/UltraTasks/actions/runs/43127481310)でビルド・検証・配布・core本測定を完了した。その版のRustテストは102成功・0失敗。Linearの製品画面とRadix/Geistの部品規約を参考に、サイドバー、コンパクトな操作帯、可変高の一覧、時間軸を残す編集面へ組み直した。配布exeで日本語Task追加、時刻入力、検索、未保存のリスト移動、完了、Undo/Redo、再起動復元、1280/900幅とdark/lightを確認した。[デザイン規約](DESIGN.md)と[参照元](docs/ui-reference-study.md)に実装値と再利用範囲を記録している。

IME変換中、Narrator、各DPI、大規模データの実GUI性能は残る確認。[PCプロトタイプの状態](docs/prototype-status.md)に配布source、旧版のcore基準測定、未確認範囲を記載している。

## Windowsで起動する

Windows x64用のビルド済み成果物を使う。通常利用にRust、Build Tools、Pythonは不要。

1. [Windows CI](https://github.com/iiinno/UltraTasks/actions/workflows/windows.yml)から、対象のsourceで完了・成功したtrusted pushのrunを選ぶ。`windows-release-<head_sha>-<attempt>`を取得する。API由来とhashを検証する取得スクリプトは[開発手順](docs/development.md#成果物を取得する)を参照。PRの検証ログを配布exeと取り違えない。
2. ZIPを展開し、同梱の `Start-UltraTasks.cmd` を開く。起動入口は既存exeだけを使い、ローカルビルドを行わない。
3. `Asia/Tokyo` の今日を表示する。保存先の既定は `%LOCALAPPDATA%\UltraTasks\demo\demo.sqlite`。新しい保存先は空のDBから始まり、再起動では保存済みデータを開く。

起動時は常に `--demo none` を使い、デモ用fixtureを生成しない。表示設定はDB横の `.ui.json` に保存する。GitHubのartifact保持期間は14日。実装と最新の状態説明は `prototype/pc-first` ブランチにあり、mainへのマージはまだ行っていない。

## 検証と開発資料

開発PCではRustビルド・check・test・Clippy・bench、fixture生成、重い計測を行わない。GitHub-hosted `windows-2025`、または明示選定したremote Windows hostで実行する。

full検証は整形、Clippy、全targetのテスト、製品/計測exeのbuild、依存検査を記録する。GitHub-hosted CIのnative GUIは `NOT_RUN` とし、別途行った実画面確認と区別する。[開発手順](docs/development.md)にremote検証、成果物取得、起動、ログの見方を記載した。

- [性能計測の方法](docs/performance-method.md)
- [システム実装計画・決定事項・完了条件](docs/implementation-plan.md)
- [UIの方向性と制作手順](docs/ui-direction.md)
- [画面コンセプト01・02](design/concepts/README.md)
- [OSSの比較と部品の品質基準](docs/design-system-research.md)
- [構成案とWindows / Androidの成立性検証](docs/architecture.md)
- [配色プレビュー](design/palette-preview.svg)、[共通トークン](design/tokens.json)、[色](design/palettes.json)、[アイコン](design/icons)
- [素材の出典・固定コミット](design/upstream-lock.json)、[ライセンス通知](THIRD_PARTY_NOTICES.md)

既存の設計文書は設計時点の提案も含む。今回のPC優先範囲と実装・検証の状態は`docs/prototype-status.md`を参照する。初期対象は個人利用で、同期を追加する際は認証と通信保護を実装する。

素材の再取得はPython 3とGitHub CLIで行い、既存lockのcommitを使う。

```sh
python3 scripts/import_design_assets.py
python3 scripts/build_design_preview.py
```

UltraTasks本体の公開・配布ライセンスは未決定。第三者素材にはそれぞれのライセンスが適用される。
