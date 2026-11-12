# UltraTasks

<!-- impeccable:product-schema 1 -->

## Platform

Windows desktop。Rust/GPUIによるネイティブアプリ。AndroidはPC版の後続工程であり、今回の実装対象ではない。

## Users

最初の対象は利用者自身の個人利用。タスクを整理し、一日の予定と作業時間を見ながら、追加・編集・完了を繰り返す。

## Product Purpose

仕事の内容と、その仕事に使う時間を一つの画面で扱う。Taskは仕事、Eventは独立した予定、TimeBlockはTaskに割り当てた作業時間。同じTaskに複数の作業枠を付けられ、完了しても作業枠を失わない。

## Operating Context

ネットワークに依存せずSQLiteへ保存する。タスク一覧と日単位の時間軸を同時に見ながら操作する。日本語で入力し、日付と時刻を区別する。Undo/Redoで操作を取り消し、再起動後もデータを復元する。

## Capabilities and Constraints

Rustの画面非依存coreとGPUI Kitのdesktopの2クレート。WebView、Electron、Tauri、Reactへの置換は行わない。既存の保存・版管理・履歴・検索・仮想化・非同期DBワーカーの契約をUI改修でも保つ。

開発PCでRustビルド・check・test・Clippy・bench、fixture生成、重い計測を実行しない。GitHub-hosted Windows runnerまたは明示選定したremote hostを使う。ローカルは編集・軽量確認・検証済み実行ファイルの少数操作に限定する。

同期、通知、繰り返し、入出力、外部カレンダー、AI/MCPは今回の範囲外。性能の数値は実測環境と対象区間を示し、core処理時間を実GUIの提示速度と混同しない。

## Brand Commitments

製品名はUltraTasks。利用者は2026-11-12に、Linearの製品画面とローカルの `REA/linear-desktop` を主要な参照とし、Geist、React向けデザインシステム、OSSコンポーネントを積極的に再利用・再実装してUIを改善するよう指定した。独自性のために既存の操作規約を作り直さない。

実装はGPUIへ適合させる。出典とライセンスを確認し、利用条件の分かる素材と部品を優先する。参照製品の商標・ロゴ・独自フォントや画像を許諾なしに再配布しない。

## Evidence on Hand

[プロトタイプの状態](docs/prototype-status.md)にWindows release版のsource、CI、実画面確認、core性能、残る確認を記録している。`design/`に既存の意味付きトークンとRadix/Lucide由来の資産、`THIRD_PARTY_NOTICES.md`に通知がある。今回のUI改修は動く既存製品の改善であり、未実装の機能を画面の装飾として足さない。

## Product Principles

- タスクの内容と割り当てた時間の関係が読める。
- 内容を主役にし、操作は頻度と重要度に応じて配置する。
- 実績ある部品の操作・状態表現を揃え、目先の見た目のために使い方を変えない。
- 保存、再試行、取り消しの状態を失わず、入力中の内容を保つ。
- 速さと操作品質は実際の成果物で確認する。

## Accessibility & Inclusion

日本語入力、キーボードfocus、読み上げ用の名前、長い文字、空・取得失敗・入力エラー、light/darkと高コントラスト、reduced motionを考慮する。IME変換中、Narrator、全DPIなどの未確認項目は確認済みと記載しない。
