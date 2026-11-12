---
name: UltraTasks Windows
description: 仕事の内容と割り当てた時間を読む、Windowsネイティブのタスク・時間割UI。
colors:
  light-canvas: "#fcfcfd"
  dark-canvas: "#111113"
  light-surface: "#f9f9fb"
  dark-surface: "#18191b"
  light-surface-hover: "#f0f0f3"
  dark-surface-hover: "#212225"
  light-border: "#e0e1e6"
  dark-border: "#2e3135"
  light-text-primary: "#1c2024"
  dark-text-primary: "#edeef0"
  light-text-secondary: "#60646c"
  dark-text-secondary: "#b0b4ba"
  light-focus-ring: "#0d74ce"
  dark-focus-ring: "#70b8ff"
  light-action-background: "#0d74ce"
  dark-action-background: "#0090ff"
  light-action-text: "#fcfcfd"
  dark-action-text: "#111113"
  light-selection-background: "#e6f4fe"
  dark-selection-background: "#0d2847"
  light-selection-text: "#113264"
  dark-selection-text: "#c2e6ff"
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
    fontFamily: Yu Gothic UI
    fontSize: "14px"
    lineHeight: "20px"
  caption:
    fontFamily: Yu Gothic UI
    fontSize: "12px"
    lineHeight: "18px"
  task-title:
    fontFamily: Yu Gothic UI
    fontSize: "14px"
    lineHeight: "18px"
  task-meta:
    fontFamily: Yu Gothic UI
    fontSize: "12px"
    lineHeight: "16px"
rounded:
  control: "6px"
  task-row: "4px"
  agenda-card: "3px"
  popup: "10px"
  large: "12px"
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
    height: "32px"
  button-primary-dark:
    backgroundColor: "{colors.dark-action-background}"
    textColor: "{colors.dark-action-text}"
    rounded: "{rounded.control}"
    height: "32px"
  button-ghost-light:
    backgroundColor: transparent
    textColor: "{colors.light-text-primary}"
    rounded: "{rounded.control}"
    height: "32px"
  button-ghost-dark:
    backgroundColor: transparent
    textColor: "{colors.dark-text-primary}"
    rounded: "{rounded.control}"
    height: "32px"
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

## Overview

2026-11-12時点のWindowsネイティブ実装を記録する。対象はRust/GPUIとGPUI Kitで描く業務画面で、WebViewやWeb版、iOS版、Android版の規約ではない。製品の条件は[PRODUCT.md](PRODUCT.md)、参照元の調査と再利用範囲は[UI参照調査](docs/ui-reference-study.md)を参照する。

仕事の内容と割り当てた時間を先に読めるようにする。Linearの製品UIにある控えめなナビゲーション、弱い境界、操作帯の整理と、Geist・Radixの公開部品規約を参照し、既存のGPUI部品へ適合させている。具体的な寸法はUltraTasksの実装値であり、参照製品から実測した寸法とは呼ばない。

前書きのトークンは現行実装から抽出した値であり、本文はその適用先と例外を示す。実行時の根拠は[theme/tokens.rs](apps/desktop/src/theme/tokens.rs)、Kitへの投影は[theme/mod.rs](apps/desktop/src/theme/mod.rs)、状態とレイアウトの根拠は[views/](apps/desktop/src/views/)にある。値の変更時は実装・[design/tokens.json](design/tokens.json)・本書を一緒に照合する。設計JSONの`draft`、`motion_proposal`、Android向け値や未使用の値を実装済みの規約へ昇格させない。

本書はソースの静的読解に基づき、実画面の独立した見た目の合否を判定しない。画面幅・テーマ・再起動を含む操作確認、CI、残る確認の記録は[プロトタイプの状態](docs/prototype-status.md)に分ける。IME変換中、Narrator、全DPI、大規模データでのGUI性能についても、本書の記載を確認完了の証拠とはしない。

## Colors

Radix Slateで面と文字を分け、Blueで操作・選択・focusを示す。エラーはTomato、競合などの警告はAmberを使う。light/darkの具体値は前書きの同名トークンに固定し、出典の色値は[palettes.json](design/palettes.json)にある。

| 意味付きトークン | Radixの対応 | 主な適用先 |
| --- | --- | --- |
| `canvas` / `surface` / `surface-hover` | Slate 1 / 2 / 3 | 作業領域、sidebar・操作帯・入力・popup、hover |
| `border` | Slate 5 | paneの区切り、入力の通常境界 |
| `text-primary` / `text-secondary` | Slate 12 / 11 | タイトル・本文、時刻・所属・補助ラベル |
| `focus-ring` | Blue 11 | キーボードfocus、時間軸の現在時刻、作業枠の左線 |
| `action-background` / `action-text` | lightはBlue 11、darkはBlue 9 / Slate 1 | 保存・適用などのprimaryボタン |
| `selection-background` / `selection-text` | Blue 3 / 12 | 選択タスク、関連作業枠、適用中の絞り込み |
| `error-background` / `error-text` | Tomato 3 / lightは12、darkは11 | 入力不正、保存失敗、削除の確認 |
| `warning-background` / `warning-text` | Amber 3 / lightは12、darkは11 | 編集競合と下書き保持の説明 |

通常のsidebar選択は`surface-hover`と`text-primary`で静かに示す。タスク行や関連作業枠の選択はBlueの組を使う。選択中の行へhoverしても選択色を保ち、hoverと選択を同じ意味にしない。primaryのhover/activeには現在と同じaction色を割り当てている。

Jadeの色階調と`positive_*`は設計資産に存在するが、現行のRust `Tokens`に対応フィールドはない。完了タスクをJadeで塗る規約や、予定カテゴリ別の色分けは実装していない。完了はアイコン・文字と補助文字色、Event/TimeBlockの違いは種類名・アイコン・左線で示す。

高コントラストでは[settings.rs](apps/desktop/src/settings.rs)がWindowsのシステム色を取得し、明示したlight/darkより優先する。canvas・surface・hoverはOSのcanvas、本文と補助文字はOSのtext、選択と主操作はhighlight/highlight textの組に置き換える。通常文字をdisabled色へ減光しない。Kitのdisabled用`muted_foreground`にはOSのgray textを渡す。hoverは塗りの差が消えるため、共通ボタンとタスク行にfocus色の輪郭を出す。

Kit Input/ListItemは選択専用の文字色を持たないため、高コントラスト時の選択背景はhighlightのalpha 0.35を使用し、通常文字色を保持する。タスク行など、文字色も指定できる要素はOSの選択色の組を使う。エラーと警告はOSのcanvas/textへ置換しても、説明文やアイコンで意味を残す。色の切り替えで密度や寸法を変えない。

本文・placeholderは4.5:1、必要な非文字状態は3:1を設計基準とする。トークンの文字色対・focus・scrollbarを対象にするテストは実装にあるが、任意の合成色、全Kit状態、全OS設定に対する実画面の適合判定とは区別する。

## Typography

フォントはWindowsにある`Yu Gothic UI`をrootとKitの双方へ指定する。独自のブランドフォントやフォントファイルは同梱しない。日本語本文は前書きの`body`、補助説明は`caption`、タスク行は`task-title`と`task-meta`を使用する。

タスク・編集paneの見出しは16pxのsemibold、location barと時間割の見出しは14pxのsemibold。popup見出しは16px・行高24px。sidebarと編集用の単行入力は13px、通常の時間軸カードのタイトルは13px・行高20pxのmediumを使う。これは大きなdisplay見出しを設けない、日常操作向けの階層である。設計JSONにある24pxのtitle値を現在のpane見出しに適用しない。

タスクのタイトルは2行まで折り返し、期限・作業時間・所属の補助行は1行で省略する。完全なタイトルは編集面で扱い、一覧の読み上げ用ラベルにも含める。sidebar名は省略表示とtooltipを併用する。時間軸では枠の実際の高さに応じて1行・2行・短い枠用の表示を選び、文字の都合で時間の長さを伸ばさない。

## Layout

寸法の`px`はGPUIのlogical pxであり、画面の物理ピクセルではない。以下は[現在のプランナー画面](apps/desktop/src/views/today.rs)の構成で、将来のすべての画面に同じ三列構成を強制する規則ではない。Windows標準タイトルバーを維持し、その下にsidebarと、location bar・作業pane・status barを置く。

| viewport幅 | 左ナビゲーション | Task / Editor | Agenda |
| --- | --- | --- | --- |
| 1000px以上 | sidebar 192px | 360px固定 | 残りの幅 |
| 780px以上1000px未満 | icon rail 52px | 360px固定 | 残りの幅 |
| 780px未満 | icon rail 52px | 「タスク／編集」と「時間割」の切替。表示中paneが残りの全幅を使う | 同左 |

判定は`width < 1000`と`width < 780`であり、1000pxちょうどはsidebar、780pxちょうどは二つの作業paneを表示する。900pxではTask/EditorとAgendaを残す。編集面は中央の一覧を置き換え、非compact時は右の時間軸を維持する。compactの切替帯は36px。起動コードの最小ウィンドウサイズは620×480pxである。

| 領域 | 現在の寸法・余白 |
| --- | --- |
| location/date bar・sidebar上端 | 高さ48px。location barの左右余白12px |
| pane toolbar | 高さ40px、左右余白12px |
| アプリ全体のstatus bar | **最小**高さ32px、左右12px・上下4px。エラーは2行まで表示でき、固定高ではない |
| Taskの検索・追加領域 | 左右12px、上8px・下4px、主な縦間隔8px |
| Editor本文 | 全周16px、主要field群の間隔20px。日付と時刻の間は8px、時刻欄の幅112px |
| Editorの保存操作帯 | 左右16px・上下12px。アプリstatus barとは別の領域 |
| 共通control・通常の単行Input | 前書きのcomponent高さを使用。検索Inputは28px、編集タイトルのTextareaは72px |
| タスク行 | 最小高さ44px、左右6px・上下1px、子要素間4px。行内iconボタンは28px角 |

shellがpane全体へ一律のpaddingを重ねる方式は使わず、各paneがtoolbarと本文の余白を所有する。前書きのspacingに加えて、実装にはfield内の6px、editorの20pxなど用途に応じた局所値がある。

タスク行は`min_h(row_height)`と実内容の高さで決まる。タイトルが2行になると自然に増え、固定の「長文行高」は設定しない。例えばタイトル2行×18px＋補助行16px＋上下padding計2px＋border計2pxなら56pxになるが、これは算出例であり固定高や追加の最小高ではない。`Tokens::long_row_height`と設計JSONの旧長文行高キーは削除済み。設計JSONは実行時に読み込まれず、描画はRustのトークンと各viewの指定に従う。

sidebarのリスト、タスク一覧、editor本文、時間軸はそれぞれ独立してスクロールする。toolbarと保存操作帯は本文と分離され、popupも高さを制限した独立scrollを持つ。

タスク一覧はGPUI `list` / `ListState`で可視行を構築する。[TaskCache](apps/desktop/src/presentation/task_list.rs)はTask IDと行内offsetをanchorに更新前後の位置を復元し、変更行を差し替える。取得は200件単位、前後ページの保持は最大600件であり、全件を一度に描く方式へ戻さない。

時間軸はscroll位置付近だけを取得・描画する。現在のコードはviewport高を使い、描画の前後60px、取得の前後90pxを余分に確保する。「候補一覧」は取得済み候補を通常の縦scrollへ並べ、続きを明示的に取得する。候補一覧・sidebar・editorをタスク一覧と同じ仮想化方式だとは記載しない。

## Elevation & Depth

常設paneはcanvas/surfaceの面差と細い境界で分ける。タスク一覧やeditorを、影付きカードでさらに囲わない。Kitのtheme tokenは単色へ再投影し、読み込まれた背景gradientも除く。

popupはsurfaceの面にGPUIの`shadow_lg()`を付け、黒の半透明overlay（`0x00000030`）越しに作業画面との前後関係を示す。高コントラスト時はpopupの影を境界線へ置換し、Kitの通常のshadowも無効にする。`shadow_lg()`の実体を独自のCSS影値へ翻訳しない。

focus表示は装飾の影と別の役割を持つ。[共通focus_outline](apps/desktop/src/views/mod.rs)はoffset・blurを0とし、focus色のspread 4pxとcanvas色のspread 2pxを重ねる。focusしてもborder幅やcontrolの寸法を変えない。共通ButtonとInputの外枠、タスク一覧、時間軸カードがこれを使う。Kitの通常focus ringはアプリ設定で無効化されるため、この共通focus表現を削らない。Select/Radio等は既存部品の振る舞いも確認対象に含める。

## Shapes

角丸は前書きの用途別トークンに従う。control/Inputは`control`、タスク行は`task-row`、時間軸カードは`agenda-card`、自前popupは`popup`。`large`はKitの`radius_lg`に設定している値で、全paneへ適用するものではない。

paneとtoolbarの区切りは1px。タスク行は通常時にも透明の1px境界を確保し、高コントラスト時のhoverで輪郭を付けても寸法がずれない。時間軸カードの左線は1px、現在時刻の線は2px。予定の枠には時間の長さという意味があるため、一般的なカードの最小高を適用しない。

## Components

### Buttonと入力

共通ButtonはGPUI Kitの`ghost`・`small`・`compact`を基礎にする。移動・設定・行内操作の主張を抑え、保存・適用は`primary`、削除の最終確認は`danger`で示す。toolbarや行内操作には28px、status barのUndo/Redoなどには24pxの高さを明示している。すべてのボタンが共通の高さで描かれるわけではない。

検索は検索icon・消去操作を持つInputで、通常のborderを省く。quick addと日時Inputにはsurfaceと通常境界を用い、Inputの外側に共通focus枠を付ける。タイトル編集にはTextareaを使うが、保存時のタイトル制約は既存のvalidationに従う。単行InputとTextareaの見た目だけを理由にデータ型を変えない。

入力エラーは該当fieldの境界と近接する説明文で示し、説明は`Role::Status`と読み上げ用ラベルを持つ。日時欄には`YYYY-MM-DD · 9:00 / 9時`の手掛かりを表示する。無効な値でも下書きを残し、保存中・競合中・復旧待ちのdisabledは既存の状態に従う。

### Sidebarとタスク一覧

[sidebar](apps/desktop/src/views/sidebar.rs)は実在するプランナー、検索、追加、リスト、リスト・カレンダー管理、表示設定を配置する。項目にはキーボード操作を備えたButtonを使う。railでは文字を隠してもtooltipとaccessibility labelを残し、iconは16pxとする。

[タスク行](apps/desktop/src/views/task_list.rs)は完了操作、タイトルと補助情報、作業時間操作の順。Lucideの`Circle` / `CircleCheck`で未完了/完了を示す。補助行は選択日の作業枠・所要時間・所属、作業枠がなければ所属・期限を表示する。リストには矢印移動・選択タスクを開く・完了操作のactionが接続される。完了直後の行と一覧anchorを保持する既存の動作を外観の都合で消さない。

読み込み中、未取得・取得失敗、条件に合う結果なし、未完了なしを別の文言で表す。未取得を「0件」と同一視しない。保存状態と取得失敗の再試行は画面下のstatus barから読めるようにする。

### Agenda

[Agenda](apps/desktop/src/views/agenda.rs)は日単位の実際の時間を縦に配置する。1分を1 logical pxとし、日境界と日跨ぎをclipする。DSTで一日が24時間にならない場合も実際の時間幅を使う。EventはCalendar、TimeBlockはClockのiconと種類名で区別し、選択タスクに関連する枠は選択色にする。

40px未満の短い枠はタイトルと時刻を1行へまとめる。通常枠はタイトルと種類・時刻を分け、高さ60px以上ならタイトルを2行まで表示する。30分枠を文字のために長く描かない。tooltipと候補一覧で補足する。

[配置処理](apps/desktop/src/presentation/agenda.rs)では重なりをlaneへ分け、lane幅が120px未満なら重複グループを件数付きの候補入口へまとめる。この入口はグループの占有時間全体を保つ。終日欄は先頭3件と総件数から候補一覧へ進める。狭さや密集を理由に予定自体を隠して完了扱いにしない。

時間軸カードはfocus可能で、Enter/Spaceで編集へ進む。Task/TimeBlockのドラッグは仮配置を表示し、時刻の確認へつなぐ。行のClock操作からも作業時間を指定できる。時刻編集と入力保護の既存の経路を維持する。

### Editor・確認・保存状態

[Editor](apps/desktop/src/views/editor.rs)は見出しと未保存/保存中の表示、タイトルまたは関連タスク、状態・期限/時刻・所属、関連作業時間を縦に並べる。本文をscrollしても下部の削除・取消・保存操作を分離して扱える。競合時は警告色の説明と下書きコピー・最新データを開く操作を表示する。

未保存の編集から一覧や別のリストへ移動する操作は[共通navigate](apps/desktop/src/app.rs)を通す。確認には「保存して移動」「変更を破棄」「編集を続ける」を用意し、保存中はその状態に応じた文言にする。popupには`Role::Dialog`、focus trap、閉じた後のfocus復帰がある。EscapeはIME変換中の扱いを既存処理へ委ねる。これらの接続が存在することと、NarratorやIMEの実機受け入れ完了は別である。

アプリstatus barは保存結果・失敗・再試行・Undo/Redoを扱う。通常文言は1行、エラーは2行までとし、tooltipでも全文を補足する。保存したタスクへ戻る操作や履歴のdisabled状態も実際の状態から決める。

### Motion

[MotionSpec](apps/desktop/src/theme/motion.rs)の通常値はeditorのfade 150msと時間軸の位置補間200ms。editorを開く単位・保存後の移動単位でGPUIの有限Animationを使い、完了を入力待ちの条件にしない。常時繰り返す装飾アニメーションはない。

OSの動き軽減、アプリの`Reduced`または`None`では両方を0msにし、即時に最終状態へ進む。[設定適用](apps/desktop/src/settings.rs)はKitのduration/springも0へ設定し、caretをsteadyにする。通常時のcaretには3秒のidle timeoutを設ける。Kitの通常motion設定とアプリのfade/位置補間は別であり、設計JSONの`motion_proposal`にある100msのfeedbackやeasingを実装済みと扱わない。

### アイコンと資産の出典

UIはGPUI Kitの`IconName` / `Icon`を用い、sidebarは16px、検索や補助操作は14px、時間軸の種類iconは12pxを明示する箇所がある。すべてのiconを同じサイズと断定せず、iconだけの操作に名前とtooltipを付ける。

[main.rs](apps/desktop/src/main.rs)は`gpui_kit::assets::AllAssets`を登録する。GPUI Kit Assets 0.9.0のこの型はLucideとKitの全iconカタログを埋め込む。`design/icons/`に保存した選定SVGと、実行時にKitから解決するicon資産の出典を区別して管理する。

| 資産・部品 | 由来と保持する通知 |
| --- | --- |
| Radixの色 | commit `dbdb85470547c7d34b9001f48fddb08ded335979`。Slate/Blue/Jade/Amber/Tomatoのlight/darkを値を変えず抽出。[MIT](third_party/radix-colors/LICENSE) |
| `design/icons/`の選定SVG | Lucide commit `45aaec4b721c98aa86a2acd0ab7281bcf68b9c5e`の11ファイルを無変更で保持。[ISCとFeather由来部分のMIT](third_party/lucide/LICENSE) |
| 実行時のKitとicon資産 | GPUI Kit 0.9.0、commit `7591a35eced761e3613c01e71ff04c10129dae1a`。[Apache-2.0](licenses/GPUI-KIT-APACHE-2.0.txt)と[Lucide ISC/MIT](licenses/LUCIDE-ISC-MIT.txt) |
| GPUI | gpui-pre 0.3.11、元のZed rev `f816e1f9263e3a8ef63c4baaa02e1441ce1c6038`。[Apache-2.0](licenses/GPUI-APACHE-2.0.txt) |

正確な取得ファイルと変更内容は[upstream-lock.json](design/upstream-lock.json)、通知全体は[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)を参照する。GPUIのApacheライセンスをZedのGPL UI crateへ拡張解釈しない。

LinearとGeistは構成・操作規約の参照先であり、固有コード・商標・ロゴ・フォント・画像をコピーしていない。React向け部品のコードもそのまま移植していない。今回の業務画面に生成画像や比較用の製品画像を配布資産として追加しない。過去のコンセプト画像は実装画面の証拠ではない。

## Do's and Don'ts

- **Do** 新しい表示にも既存のsemantic tokenとGPUI Kitの公開部品APIを使い、通常・hover・focus・selected・disabled・errorを同じ状態モデルへ接続する。
- **Do** Task、Event、TimeBlockの意味、時間の占有範囲、下書き、Undo/Redoと保存状態を保つ。
- **Do** logical pxと物理ピクセル、ソース上の実装値と設計目標、静的確認と実画面での確認を区別する。
- **Do** 長い日本語、空・取得失敗・入力エラー、light/dark、高コントラスト、動き軽減を確認対象として残し、実施した範囲だけを報告する。
- **Don't** 見た目を揃えるために長い行を固定高へ押し込めたり、短い時間枠を伸ばしたり、全件描画へ戻したりしない。
- **Don't** focusを弱い通常境界へ埋めず、状態の識別を色だけに依存させない。
- **Don't** 未実装の機能・架空の情報・新しいpaletteを装飾目的で追加しない。
- **Don't** Linear/Geist固有資産、生成コンセプト、未確認のアクセシビリティや性能を配布資産・完成の証拠として扱わない。
