# UI基盤の調査と採用方針

調査日: 2026-11-09。一次資料のソースとライセンスを確認した。以下の構成は採用候補であり、GPUIアプリの実ビルド、Windows／Android実機、読み上げ、パフォーマンスはこの調査では検証していない。

## 推奨する構成

**[GPUI Kit（旧gpui-component）](https://github.com/longbridge/gpui-kit)を部品実装の候補とし、Radixのトークン、Lucideのアイコンを揃える。日付操作はReact Ariaの振る舞いを参照する。**

Linearに近い情報密度と落ち着いた階層表現を目指し、Google系アプリからは操作への反応、状態変化の分かりやすさ、タッチ操作の設計を学ぶ。アクセント色、角丸、影、文字サイズを画面ごとに変えず、UltraTasksの共通トークンを通して使う。

| 対象 | 役割 | 再利用の形 |
| --- | --- | --- |
| GPUI Kit | ボタン、入力、メニュー、ポップオーバー等のネイティブ部品候補 | Rustの依存として評価。対応版と対象OSを実ビルドで検証する |
| Radix Themes / Colors | 色階調、余白、文字サイズ、状態表現 | 選定した値をRust側のトークンへ移植する |
| Lucide | 一貫した線幅と形状のアイコン | 必要なSVG資産を再利用する |
| React Aria / React Stately | 日付入力・選択のフォーカスと状態遷移 | 振る舞いを仕様化し、必要なロジックをGPUIへ移植する |
| Material | 操作の結果を伝える動き | 既存motion tokenを参考に、用途別の短い動きを検証する |

Radix、React Aria、shadcn/uiのReact部品はGPUIで直接実行できない。CSSやDOMの処理はRust側への移植が必要になる。ARIA属性を転記してもWindowsやAndroidの支援技術への対応にはならないため、ネイティブのアクセシビリティ機能へ対応付ける。

## 確認したOSSとソース

### Radix Themes / Colors — 共通トークンの出発点

- [ThemesのLICENSE](https://github.com/radix-ui/themes/blob/main/LICENSE): MIT、WorkOS。
- [ColorsのLICENSE](https://github.com/radix-ui/colors/blob/main/LICENSE): MIT、Modulz / WorkOS。Themesとは別のリポジトリとして記録する。
- [space.css](https://github.com/radix-ui/themes/blob/main/packages/radix-ui-themes/src/styles/tokens/space.css): 4・8・12・16・24・32・40・48・64の余白スケール。
- [typography.css](https://github.com/radix-ui/themes/blob/main/packages/radix-ui-themes/src/styles/tokens/typography.css): 文字サイズ、行高、ウェイトを対にして扱う参考。
- [dark.ts](https://github.com/radix-ui/colors/blob/main/src/dark.ts): Slate、Mauve等の暗色用パレットと透過色。

パレットの番号をUIから直接参照せず、`surface`、`surface_hover`、`border`、`text_primary`、`text_secondary`、`focus_ring`等の用途に割り当てる。明色・暗色で同じ用途を維持する。日本語の文字サイズ、行高、字幅、フォールバックフォントは実際の描画で調整する。

### Lucide — SVGの直接再利用候補

- [アイコンソース](https://github.com/lucide-icons/lucide/tree/main/icons)
- [LICENSE全文](https://github.com/lucide-icons/lucide/blob/main/LICENSE)

基本はISC。Feather由来のアイコンにはMITの通知も含まれる。`calendar`、`clock`、`check`等もその一覧に含まれるため、採用時はLICENSE全文を保持する。必要なアイコンを選び、サイズ・線幅・光学的な配置を共通部品で揃える。

### React Aria / React Stately — 日付操作の仕様と状態遷移

リポジトリと確認したソースはApache-2.0。

- [リポジトリとライセンス](https://github.com/adobe/react-spectrum)
- [DatePicker公式説明](https://react-aria.adobe.com/DatePicker): 入力欄とカレンダーの連携、ロケール、タイムゾーン、利用不可日など。
- [useCalendarGrid.ts](https://github.com/adobe/react-spectrum/blob/main/packages/react-aria/src/calendar/useCalendarGrid.ts): キーボードによる日付移動、Home / End / Escape、フォーカスの扱い。
- [useDatePicker.ts](https://github.com/adobe/react-spectrum/blob/main/packages/react-aria/src/datepicker/useDatePicker.ts): 入力欄、開閉ボタン、エラー、ダイアログ、カレンダーの連携。
- [useCalendarState.ts](https://github.com/adobe/react-spectrum/blob/main/packages/react-stately/src/calendar/useCalendarState.ts): 選択日と表示範囲、フォーカス、利用不可日の状態管理。

選択した日とキーボードフォーカスの位置を分けて扱うこと、編集途中と確定値を分けること、月末・うるう年・週開始曜日を扱うことを仕様に含める。予定の週表示と、日付を選ぶ月間ピッカーは異なる部品として設計する。

### shadcn/ui — 部品構成の比較対象

- [LICENSE](https://github.com/shadcn-ui/ui/blob/main/LICENSE.md): MIT。
- [calendar.tsx](https://github.com/shadcn-ui/ui/blob/main/apps/v4/registry/new-york-v4/ui/calendar.tsx): React DayPicker、ボタン、Lucideを組み合わせたカレンダー。
- [registry構成](https://github.com/shadcn-ui/ui/blob/main/apps/v4/registry/README.md): authored sourceと生成されたスタイルの区別。

日付セル、ナビゲーション、選択状態の整理を比較する。React・DOM・Tailwindへの依存があるため、ネイティブ版の実装基盤として直接採用する予定はない。GPUI側の既存部品で足りる箇所は、それを使って統一する。

### Material — 動きの参考

- [Material Web](https://github.com/material-components/material-web): Apache-2.0。READMEはmaintenance modeと記載。
- [motion tokenの実体](https://github.com/material-components/material-web/blob/main/tokens/versions/v0_192/_md-sys-motion.scss): Material Webのversioned source。
- [M3 Motion](https://m3.material.io/styles/motion/overview/how-it-works): 設計の参照先。今回のテキスト取得ではJavaScript要求のみで、本文を検証できていない。

確認したMaterial Webのtokenには50 / 100 / 150 / 200msの短いdurationと、standardの`cubic-bezier(0.2, 0, 0, 1)`がある。これは既存のWeb向けtokenであり、最新M3全体の仕様と同一とは断定しない。UltraTasksではチェック完了、ポップオーバー開閉、予定の移動を別々に評価し、入力を受け付けるまでアニメーション完了を待たせない。

### FullCalendar Standard — 予定表示の比較対象

- [リポジトリ](https://github.com/fullcalendar/fullcalendar)
- [Standard / Premiumのライセンス区分](https://fullcalendar.io/license)

Standardの非PremiumプラグインはMIT。Premiumは別条件なので区別する。週表示、重複する予定、複数日にまたがる予定、ドラッグの動作を調べる候補とする。JavaScriptの表示部品をGPUIで直接利用することはできない。

## 品質を確認する部品ギャラリー

画面を増やす前に、下表の状態を固定データで再現するギャラリーを作る。スクリーンショットだけでなくキーボード・マウス・タッチで操作し、結果を記録する。ここに記すものは受け入れ条件であり、現時点の達成実績ではない。

| 部品 | 比較する状態 | 受け入れ条件 |
| --- | --- | --- |
| ボタン・チェック | 通常、hover、pressed、focus、disabled、処理中 | 状態が色だけに依存しない。キーボードで押せる。連打で重複操作しない |
| タスク行 | 未完了、完了、選択、編集中、長いタイトル、複数行 | 完了後の移動先とUndoが分かる。長い日本語で操作部が押し出されない |
| 入力欄 | 空、編集中、IME変換中、検証エラー | 日本語変換確定のEnterでタスクを誤送信しない。エラー後も入力を保持する |
| 日付ピッカー | 選択日、今日、範囲外、利用不可、月末、うるう年 | フォーカスと選択が区別できる。矢印とHome / Endで移動できる。閉じると起点へフォーカスを戻す |
| メニュー・ポップオーバー | 開閉、スクロール、画面端、入れ子 | 画面外へ欠けない。Escapeで期待する一段だけ閉じる。フォーカスが迷子にならない |
| 週カレンダー | 予定なし、密な予定、重複、終日、日跨ぎ | 重複しても予定を識別・選択できる。時刻と日付の対応が維持される |
| タスクの時間割り当て | ドラッグ中、禁止位置、確定、取消、Undo | プレビューと確定後の時刻が一致する。ドラッグ以外でも時間を割り当てられる |
| 通知・エラー | 成功、Undo、保存失敗、オフライン | 通知で操作の流れを遮らない。失敗と未保存が識別でき、再操作できる |

画面サイズはWindowsの標準的なウィンドウと狭いウィンドウ、Androidの縦画面を用意する。日本語・英語、明色・暗色、文字拡大、Windowsの表示倍率125% / 150% / 200%、動きを減らす設定も確認する。支援技術はWindowsの読み上げとAndroid TalkBackで実機確認する。

リストは0件・数十件・1万件を固定fixtureで比較する。カレンダーは通常の一週間と予定が集中した一日を用意する。60Hz表示の1フレーム予算16.7msを参考に、入力反映とスクロールのフレーム時間を計測する。記録には端末、表示倍率、ビルド種別、データ量、計測方法を添える。計測前に「高速」「軽量」と達成報告しない。

## 画像生成を使う段階

同じ情報量・同じタスク・同じ一週間を使い、2〜3方向の構成案を比較する。「今日」と週カレンダーを対象に、情報の階層、余白、色、タスクと時間のつながりを選ぶ材料にする。

選んだ案からトークンと部品仕様を起こし、ネイティブの実装画面と並べて比較する。生成画像では日本語入力、フォーカス、読み上げ、ドラッグ、空状態、エラー、フレーム時間は検証できない。これらはギャラリーと実機で確認する。

## 再利用の記録

採用した素材ごとにupstream URL、commit SHA、ライセンス、取り込んだファイル、変更内容を記録する。ライセンス通知は素材と一緒に保持する。ソースURLの`main`は更新されるため、実際に取り込む時点でcommitへ固定する。

この文書は調査結果を示すもので、素材の取り込み完了や依存追加、GPUIアプリのビルド成功を示すものではない。実際の取り込み状況は別の素材台帳で管理する。
