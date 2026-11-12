---
name: UltraTasks Windows
description: Linearの公開UIを参照してGPUIで再実装する、Windowsネイティブのタスク・時間割UI。
colors:
  light-canvas: "#f9f9fa"
  dark-canvas: "#121213"
  light-sidebar: "#efeff0"
  dark-sidebar: "#09090a"
  light-surface: "#ffffff"
  dark-surface: "#121213"
  light-surface-hover: "#f0f0f3"
  dark-surface-hover: "#171718"
  light-border: "#e2e2e2"
  dark-border: "#212224"
  light-text-primary: "#1c2024"
  dark-text-primary: "#e2e4e7"
  light-text-secondary: "#60646c"
  dark-text-secondary: "#8a8f98"
  light-focus-ring: "#60646c"
  dark-focus-ring: "#8a8f98"
  light-action-background: "#1c2024"
  dark-action-background: "#e2e4e7"
  light-action-text: "#ffffff"
  dark-action-text: "#121213"
  light-selection-background: "#e8e8ec"
  dark-selection-background: "#1b1b1c"
  light-selection-text: "#1c2024"
  dark-selection-text: "#e2e4e7"
  light-error-background: "#feebe7"
  dark-error-background: "#391714"
  light-error-text: "#5c271f"
  dark-error-text: "#ff977d"
  light-warning-background: "#fff7c2"
  dark-warning-background: "#302008"
  light-warning-text: "#4f3422"
  dark-warning-text: "#ffca16"
typography:
  body:
    fontFamily: Inter
    fontSize: "13px"
    lineHeight: "19.5px"
    fontWeight: 450
  caption:
    fontFamily: Inter
    fontSize: "12px"
    lineHeight: "18px"
  task-title:
    fontFamily: Inter
    fontSize: "13px"
    lineHeight: "20px"
    fontWeight: 510
  task-meta:
    fontFamily: Inter
    fontSize: "12px"
    lineHeight: "18px"
  editor-title:
    fontFamily: Inter
    fontSize: "20px"
    lineHeight: "26.6px"
    fontWeight: 590
rounded:
  control: "6px"
  navigation: "8px"
  task-row: "8px"
  agenda-card: "3px"
  main: "12px"
  create-dialog: "21px"
  dialog: "12px"
spacing:
  space-4: "4px"
  space-8: "8px"
  space-12: "12px"
  space-16: "16px"
  space-24: "24px"
components:
  button-primary-light:
    backgroundColor: "{colors.light-action-background}"
    textColor: "{colors.light-action-text}"
    rounded: "{rounded.control}"
    height: "28px"
  button-primary-dark:
    backgroundColor: "{colors.dark-action-background}"
    textColor: "{colors.dark-action-text}"
    rounded: "{rounded.control}"
    height: "28px"
  button-ghost-light:
    backgroundColor: transparent
    textColor: "{colors.light-text-primary}"
    rounded: "{rounded.control}"
    height: "28px"
  button-ghost-dark:
    backgroundColor: transparent
    textColor: "{colors.dark-text-primary}"
    rounded: "{rounded.control}"
    height: "28px"
  input-light:
    backgroundColor: "{colors.light-surface}"
    textColor: "{colors.light-text-primary}"
    rounded: "{rounded.control}"
    height: "32px"
  input-dark:
    backgroundColor: "{colors.dark-surface}"
    textColor: "{colors.dark-text-primary}"
    rounded: "{rounded.control}"
    height: "32px"
---

# Design System: UltraTasks

## 対象と確認状況

2026-11-12時点の実装 `a0fdec56b8c7fd30f7a612c91cb21ce597ebd56c` を静的に照合した仕様である。利用者の指定「自作はしないでください。コードレベルで再実装するくらい真似て作って」に従い、Linearの公開UIと部品定義をGPUIへ対応付けている。製品の条件は[PRODUCT.md](PRODUCT.md)、公開URL・採用値・確認範囲は[Linear UIの参照記録](docs/linear-ui-reference.md)に記載する。

機能基盤ではTask・Event・TimeBlockの区別、SQLite保存、非同期DBワーカー、未保存入力、Undo/Redo、検索と仮想化を維持する。これらの既存機能と今回のUIの確認状況は分けて扱う。GitHub CI run `43165128118` ではfmtが失敗した。整形反映後のCI結果は本書に未記録で、新しいUIのGUI確認も未実施である。本書はUIの完成、Linearとの同等性、ビルド・テストの成功を判定しない。

実行時の値は[theme/tokens.rs](apps/desktop/src/theme/tokens.rs)、Kitへの設定は[theme/mod.rs](apps/desktop/src/theme/mod.rs)、配置と状態表示は[views](apps/desktop/src/views/)にある。[design/tokens.json](design/tokens.json)には出典と適合時の変更を記録しているが、描画時に読み込むファイルではない。前書きは現在のRust指定値を示す。設計JSONに残るAndroid向け密度、`motion_proposal`、未使用の役割はWindowsの実装値と区別する。

## 画面構成

Windows標準タイトルバーの下にサイドバーと一つの主領域を置く。主領域はタスク一覧、詳細編集、時間割を切り替える。新規タスクは中央の作成ダイアログで入力する。時間割は専用画面としてタスク一覧と時間軸を並べる。

[Navigation](apps/desktop/src/app.rs)が画面の移動を扱う。`Tasks`と`List`は一覧、`Calendar`は時間割、`Task`・`ListTask`・`Entity`は詳細、`QuickInput`は作成ダイアログへ進む。`SearchInput`は一覧へ移動し、検索欄を開いて入力focusを移す。詳細を開くと主領域全体を編集面へ切り替え、時間軸も一覧と一緒に置き換える。

| viewport幅 | サイドバー | 通常のタスク一覧 | 時間割 |
| --- | --- | --- | --- |
| 900px以上 | 244px | 主領域の幅を使う | タスク一覧280pxと、残りの幅の時間軸 |
| 900px未満 | icon rail 48px | 主領域の幅を使う | タスクと時間割を高さ36pxの切替帯で切り替える |

判定は[views/today.rs](apps/desktop/src/views/today.rs)の `width < 900` に従う。900pxちょうどでは通常サイドバーと時間割の2領域を表示する。詳細内の縦配置は別の条件で決まり、後述する `available_width` を使う。寸法のpxはGPUIのlogical pxであり、物理ピクセルではない。

| 領域 | 現在の指定値 |
| --- | --- |
| 主領域 | 上下と右の外余白8px、角丸12px、境界1px |
| 主ヘッダー | 高さ58px、左右余白8px。詳細ヘッダーだけ左右16px |
| サイドバー | 左右余白12px。rail時は8px |
| ナビゲーション行 | 高さ28px、上下margin各1px、左右padding各7px、角丸8px |
| 一覧の絞り込み・検索帯 | 高さ44px、左右余白12px |
| 一覧のグループ見出し | 高さ36px、左右と上の外余白8px、左右padding各8px |
| タスク行 | 固定高44px、左右padding各8px、要素間8px、角丸8px |
| アプリのstatus bar | 最小高さ28px、左右余白12px。エラー文は2行まで |

主ヘッダーの58pxはLinear本体の `ContentViewHeader` の既定値 `57 + thinPixel` を1xで解釈した値である。別のsubheaderに使われる `44 - thinPixel` と混同しない。一覧の44px操作帯と行高も、主ヘッダーとは別に指定している。

## 色と状態

[配色の根拠](design/tokens.json)は公開起動shell、本体の静的定義、ホームページのデモを分けている。前書きの色をすべてLinear本体の実測色とは呼ばない。

| 色の役割 | 取得元と適用 |
| --- | --- |
| `sidebar`・`canvas`・`border` | 公開起動shellのdarkとlightの値を採用。認証後のテーマ出力は未確認 |
| darkの本文と補助文字 | ホームページのデモから `#e2e4e7` と `#8a8f98` を採用 |
| darkのhoverと選択 | デモの白overlay `#ffffff05` と `#ffffff0a` をshellの `#121213` へsRGBで合成し、不透明な `#171718` と `#1b1b1c` に丸めた値 |
| lightの本文・補助文字・hover・選択 | 本体の解決済みテーマ色が未確定のため、既存のRadix Slate値を維持 |
| `surface` | darkはcanvasと同じ値。lightは本体テーマ生成器の白い基底色に対応する `#ffffff` |
| 主操作とfocus | 主操作は中立色の文字と面を反転し、focusは補助文字色を使うUltraTasks側の対応。Linearからの実測色ではない |
| エラーと警告 | 既存のRadix TomatoとAmberの組を維持 |

通常のサイドバー選択はhover面と本文色、タスク行の選択は選択面と選択文字色を使う。選択行にhoverしても選択色を維持する。完了はLucideの状態アイコンと補助文字色でも示す。Jadeの階調は設計資産に残るが、現行 `Tokens` に完了専用色はない。

高コントラストではWindowsのシステム色が通常テーマより優先される。サイドバーを含む各面はOSのcanvas、通常文字はOSのtext、選択と主操作はhighlightとhighlight textを使う。hoverの塗りが同じになる箇所は輪郭で示す。KitのInputとListItemには選択専用の文字色がないため、選択背景をhighlightのalpha 0.35とし、通常文字色を保つ。エラーと警告は文字とアイコンでも区別する。

本文・placeholderの4.5:1、必要な非文字状態の3:1は設計基準である。関連テストがソースにあることを、新しい色や全Kit状態の合格記録へ置き換えない。

## 文字とフォント

Inter 4.1の静的TrueType版400・500・600・700を同梱する。日本語はWindowsの `Yu Gothic UI` へfallbackし、このフォント自体は同梱しない。rootとKitのfont family、ダイアログの本文にInterを設定している。

| 用途 | 現在のRust指定 |
| --- | --- |
| 本文 | 13px、行高19.5px、weight 450 |
| 共通ボタン | weight 500、高さ28pxが既定 |
| ナビゲーション・一覧のグループ名 | 13px、weight 510 |
| タスク名 | 13px、行高20px、weight 510、1行省略 |
| 一覧の属性 | 12px、行高18px、1行省略 |
| 詳細のタイトル・関連タスク名 | 20px、行高26.6px、weight 590 |
| 新規タスクのタイトル入力 | 20px、weight 590、入力高40px |
| ダイアログの見出し | 16px、行高24px、semibold |

本体CSSの本文weight 450を指定しているが、Windows DirectWriteでの静的face選択とvariable fontの見た目の一致は未検証である。510と590も画面側の要求値を示す。設計JSONのnavigation 500・title 600は役割の定義であり、これらのviewには510・590の明示指定がある。設計JSONのtitle tracking `-0.24` も、現在のeditorで適用済みとは扱わない。

一覧ではタイトルと属性を横に並べる。全文は詳細のTextareaと一覧の読み上げ用ラベルで扱う。sidebar名は省略表示とtooltipを併用する。行高を固定したことによる長い日本語の読みやすさや、fallback後の字形・行高はGUIでの確認対象である。

## 詳細画面

[views/editor.rs](apps/desktop/src/views/editor.rs)は主領域を本文と属性へ分ける。本文にはタイトルまたは関連タスク名と作業時間、属性には状態、期限、日付・時刻、所属を置く。リストとカレンダー自体の編集には属性列を設けない。

`available_width = viewport幅 - sidebar幅 - 10` とし、sidebar幅は900px未満で48px、それ以外で244pxとする。`available_width < 900` なら本文の下へ属性を並べる。900pxちょうどでは横配置であり、ウィンドウ幅900pxの判定とは異なる。

| 詳細の領域 | 指定値・計算式 |
| --- | --- |
| 属性列の幅 | `(available_width × 0.26087 + 66.087).clamp(280, 400)` px |
| 列の間隔 | `(available_width × 0.08696 - 55.304).clamp(16, 56)` px |
| 本文の最大幅 | 本文フォント13pxの `0` の字幅×80。CSSの80chに対応 |
| 本文・属性を包む領域の上padding | 21.5px |
| タイトルを含む本文の上margin | 通常32px。`available_width <= 768` では18px |
| 横配置の左右padding | 左は列間隔と同じ値、右は列間隔に14pxを加える |
| 縦配置 | 左右padding16px、属性の上padding24pxと境界1px |

保存ボタンと未保存・保存中の表示は上部ヘッダーにある。「取消」と「削除」はヘッダーのDropdownMenuに置く。本文は独立してスクロールする。入力エラーは該当欄と近接した説明、競合は下書きを残した説明とコピー・最新データを開く操作で示す。

## 作成と補助操作

[app.rs](apps/desktop/src/app.rs)の `Popup` とGPUI Kit 0.9.0の既存部品を使う。開閉の状態と保存処理の状態は別に保持する。

| 操作 | 実装している部品と指定 |
| --- | --- |
| 新規タスク | `WindowExt::open_dialog` の中央Dialog。幅750px、角丸21px。新規ボタンとCtrl+Nから開く |
| 追加先リスト | 作成Dialog内のDropdownMenu。同じ入力Entityを保持して追加先を変更する |
| 絞り込み | 起点のButtonに付けたPopover。幅384px、`Anchor::TopLeft`、offset 6px |
| 時間割の日付 | 編集中でない時間割では起点付きPopover。幅344px、`Anchor::TopLeft`、offset 6px |
| 表示設定・リスト管理・未保存確認・削除確認 | 中央Dialog。幅400px、角丸12px。時間割以外の日付操作もこの経路 |
| 詳細の取消・削除 | `dropdown_menu_with_anchor(Anchor::TopRight)` |

作成入力はダイアログに置き、一覧には常設しない。検索欄は検索操作で開く。共通ボタンは28px、status bar内は24px、新規作成の確定ボタンは32pxとする。検索Inputは28px、日時Inputは32px、詳細タイトルのTextareaは80pxである。

Dialogは外側クリックで閉じない設定とし、既定のEnter確認で閉じない。EscapeはIMEの変換状態を確認して既存の取消経路へ進める。Popoverは起点から配置し、Kitの開閉とfocus処理へ接続する。画面端での配置、focus復帰、IMEとの組み合わせは新しいGUIで未確認である。

共通focus表示はoffset・blurが0、spreadが1pxの単色輪郭である。ButtonはKit標準ringを個別に無効化し、`focus_visible` でこの輪郭を表示する。Kitテーマ全体のfocus ringは有効であり、他の部品まで無効にした扱いにはしない。タイトル入力のfocusは1px境界の色で示す。

## 保存と一覧の状態

詳細の未保存入力または保存中の状態があるとき、`navigate` は `Popup::Leave` を開く。「保存して移動」「変更を破棄」「編集を続ける」の既存の操作を維持する。Dialogを閉じたことだけではDB保存の成功としない。保存コマンド、`FormStamp`、遅れて届く応答の照合、再試行はアプリの既存処理を使う。

status barは保存結果、取得・保存の失敗、再試行、Undo/Redoを扱う。通常文は1行、エラーは2行まで表示し、tooltipにも全文を渡す。無効化の状態は保存中、復旧中、履歴の有無から決める。

タスク一覧はGPUIの `list` と `ListState` を継続する。[TaskCache](apps/desktop/src/presentation/task_list.rs)はTask IDと行内offsetを使ってスクロール位置を復元する。取得は200件単位で、前後ページの保持上限は600件である。固定高44pxの行へ変更しても `uniform_list` やDataTableへ置き換えていない。矢印移動、選択タスクを開く操作、完了操作、直後の完了行保持も既存の経路を使う。

読み込み中、未取得・取得失敗、条件に合う結果なし、未完了なしを別の文言で示す。表示の刷新を理由に、未取得を0件と扱ったり全件描画へ戻したりしない。

## 時間割と動き

時間割ではTaskの作業枠であるTimeBlockと、独立した予定であるEventを区別する。[Agenda](apps/desktop/src/views/agenda.rs)は実際の時間幅を縦に置き、日跨ぎとDSTを既存の計算で扱う。表示付近の取得・描画、重複候補、ドラッグの仮配置と時刻確認を維持する。時間枠を文字量に合わせて長くする規約は追加しない。

[MotionSpec](apps/desktop/src/theme/motion.rs)の通常値は詳細のfade 150msと時間軸の位置補間200msである。OSの動き軽減、アプリの `Reduced` または `None` では0msになる。Dialogには入口のアニメーションを指定しない。これらの指定値と、実際の操作時に確認した動きは分けて記録する。

## 資産と残る確認

Interは[公式4.1リリース](https://github.com/rsms/inter/releases/tag/v4.1)の静的TTFを無変更で使用し、SIL Open Font License 1.1の通知を保持する。正確なcommitとファイルhashは[fonts/source.json](apps/desktop/assets/fonts/source.json)、通知は[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)にある。Lucide、Radix、GPUI Kit、GPUIの既存通知も維持する。

Linearの公開CSS・JavaScript・画面資料は構造と値の参照に使い、生コードや参照画像を配布資産としてコピーしない。公開されていることをコード・商標・ロゴの再配布許諾とは扱わない。GPUIのApacheライセンスをZedのGPL UI crateへ適用しない。

CI結果と新しいGUIの判定は、対象SHAを付けて[プロトタイプの状態](docs/prototype-status.md)に別途記録する。旧実装で確認した保存や操作の記録は、新UIの外観・操作の証拠へ転用しない。一覧、作成、詳細、保存、再起動後の復元、検索、時間割、Undo/Redoに加え、狭い幅、dark・light、長い日本語、IME、focus、Narrator、高コントラスト、DPI、動き軽減、大規模データでのGUI性能は確認範囲を明示する。
