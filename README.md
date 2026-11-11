# UltraTasks

予定とタスクを、一つの流れで扱うネイティブアプリ。

UIUXを重視し、Linearの情報密度と整った見た目、Google系アプリの迷わず触れる操作感を参考に、入力・移動・完了がつながる道具を作る。

現在はWindows PC向けプロトタイプを実装している。Rustの画面非依存coreとGPUI Kitのdesktopの2クレートで、SQLiteへローカル保存する。WebViewは使用しない。Android、同期、繰り返し、通知、入出力、外部カレンダー、AI、MCPは後続の段階とする。

## 現在地

Task・Event・TimeBlock・リスト・カレンダーの編集、Undo/Redo、検索と絞り込み、日付移動、時刻指定と最小ドラッグを実装し、正式テーマとcoreを接続した。タスク一覧は可視行を描画し、保存は専用DBスレッドで処理する。テーマはsystem/light/dark、motionはsystem/reduced/noneを選べる。

統合時の確認は型確認・整形・差分確認まで。統合版の全テスト、release実行ファイルの生成、native smoke、GUI操作と性能測定は後続検証である。IME、Narrator、DPI・文字拡大、保存と再起動を実操作で検証するまで、操作品質の完成や性能目標達成は主張しない。[PCプロトタイプの状態](docs/prototype-status.md)に範囲を記載した。

## Windowsで起動する

Windows x64、Visual Studio C++ Build ToolsとWindows SDK、Rustup、Python 3が必要。PowerShellでこのリポジトリのルートから実行する。

```powershell
rustup toolchain install 1.99.1-x86_64-pc-windows-msvc --profile minimal --component rustfmt --component clippy --target x86_64-pc-windows-msvc
$workspace = (Get-Location).Path
$output = Join-Path (Split-Path $workspace -Parent) 'out/ultratasks'
$db = Join-Path $output 'data/dev/demo.sqlite'
powershell.exe -NoProfile -File scripts/dev.ps1 -WorkspaceRoot $workspace -TargetDir "$output/build/integrated" -EvidenceDir "$output/evidence/dev" -DbPath $db -Demo small -Seed 42 -Date 2026-11-11 -Zone Asia/Tokyo
```

スクリプトはMSVCを検出し、release build後に同じ実行ファイルを起動する。`-Demo small`は未使用のDB向け。既存の非空DBにはseedせず、再起動では同じ保存先と`-Demo none`を指定する。

```powershell
powershell.exe -NoProfile -File scripts/dev.ps1 -WorkspaceRoot $workspace -TargetDir "$output/build/integrated" -EvidenceDir "$output/evidence/dev" -DbPath $db -Demo none -Date 2026-11-11 -Zone Asia/Tokyo
```

`--db`は絶対パス、`--zone`はIANA timezoneが必須。直接起動する場合の実行ファイルは`$output/build/integrated/x86_64-pc-windows-msvc/release/ultratasks-desktop.exe`。`--date`を省略すると指定zoneの今日を表示する。DB横の`.ui.json`に表示設定を保存する。

## 検証と開発資料

```powershell
powershell.exe -NoProfile -File scripts/verify.ps1 -WorkspaceRoot $workspace -TargetDir "$output/build/integrated" -EvidenceDir "$output/evidence/verify" -Mode full -GuiMode Native
```

fullは整形、Clippy、全テスト、製品build、依存検査、native smokeを段階別に記録する。smokeはウィンドウ・初回frame・終了の確認で、GUI操作の受入とは別。[開発手順](docs/development.md)にfast、受入テスト、fixture生成とログの見方を記載した。

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
