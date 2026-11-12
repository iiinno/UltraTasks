# Linear構成へ変更したUIの検証

2026-11-12。サイドバー、一覧、詳細、作成ダイアログを再実装したWindows版の記録。外観の参照先と採用値は[参照記録](linear-ui-reference.md)、現在の仕様は[DESIGN.md](../DESIGN.md)にある。

## 検証対象

| source | GitHub Actions | 実画面の確認 |
| --- | --- | --- |
| `1c3b617b4c4be58d944301f9e95a77c8ff617bfe` | [run43165996956](https://github.com/iiinno/UltraTasks/actions/runs/43165996956) completed/success | 1280x800、darkとlight。PID24972 |
| `c0e2ad735d2ada61c9bfc9ead9cc77123dff1e3c` | [run43170164258](https://github.com/iiinno/UltraTasks/actions/runs/43170164258) completed/success | 1280x800と900x800、light。PID24312・34848 |
| `a50b5cc284804c0a182216ce78fd00b1e73f5cef` | [run43172464500](https://github.com/iiinno/UltraTasks/actions/runs/43172464500) completed/success | 時間割の初回幅を修正。後続cf1f21eで実画面を確認 |
| `0486043bf534ec30970d81998c741e63c001783a` | [run43181239813](https://github.com/iiinno/UltraTasks/actions/runs/43181239813) completed/success | 実アプリ参照後のdark表示。1920x1200の最大化画面、PID24456 |
| `cf1f21e6b0761ade0decc149c4b1cd6eceff760d` | [run43186313466](https://github.com/iiinno/UltraTasks/actions/runs/43186313466) completed/success | 最終実行版。780x800、light、PID22920。通常起動先へ登録済み |

CIではfmt、Clippy、全テスト、release build、依存検査を実行する。CIのnative GUI欄はNOT_RUNであり、下表は配布物のAPI由来・SHA・run/attempt・ファイルhashを確認した後の手動操作である。初回の整形前run43165128118では105件成功・0件失敗・2件ignored、fmtのみ失敗だった。

実画面はWindowsの125%表示環境で確認した。ウィンドウ寸法はGPUIのlogical px。1280幅の取得画像はtitlebar込み1280x830、900幅の画像は1127x1039で、画像寸法をlogical pxの実測値へ置き換えていない。

## 操作結果

普段のDBを開かず、既存の専用QA DBを新しい確認場所へ複写した。開始時はTask4件、Event1件、TimeBlock2件、List2件、Calendar1件。fixture生成やRust処理は開発PCで実行していない。

| 操作 | 読み戻した結果 | sourceと証拠 |
| --- | --- | --- |
| 一覧表示 | 常設の追加欄がない一行一覧、サイドバー、属性を表示 | 1c3b617、s-29／frame-d32bb6cf |
| 起動直後Ctrl+N | 中央の作成ダイアログが開き、タイトル欄へfocus | 1c3b617、s-31 |
| 未保存の終了確認を繰り返して取消し | 同じ新規ダイアログと入力内容へ戻る | 1c3b617、s-37・39・41 |
| 新規作成と詳細編集 | 日本語のタイトルと期限2026-11-13を保存し、一覧で確認 | 1c3b617、s-43・53・55 |
| Undo/Redo | 元のタイトル・期限なしへ戻り、再度編集後の値へ戻る | 1c3b617、s-57・59 |
| Ctrl+K・日本語検索 | 「請求書」で対象1件だけになり、検索欄へfocus | 1c3b617、s-61・63 |
| 検索後の選択 | 非表示になった前の選択をEnterで開かない。下キーで選ぶと表示中の対象を開く | 1c3b617、s-65・67 |
| 日付操作 | Ctrl+Dで日付ボタン起点のPopoverを表示し、取消しで時間割へ戻る | 1c3b617、s-72・74 |
| テーマ保存 | lightと動きなしを適用し、保存完了を表示 | 1c3b617、s-78 |
| 再起動復元 | 編集したタイトル、期限、light設定を読み戻す | c0e2ad7、s-81／frame-9c363771 |
| 作成欄のfocus | 透明な余白が灰色に塗られず、細い境界だけを表示 | c0e2ad7、s-99／frame-be6c5241 |
| メニュー起点 | 追加先、絞り込み、詳細操作をAXPressで開く | c0e2ad7、s-85・115・124 |
| キーボードのメニュー操作 | Tab・Enterで起動。Escapeで起点へ戻り、Spaceで再度開く | c0e2ad7、s-91・95・97 |
| 追加先と保存 | 通常クリックで「仕事」を選択。Ctrl+Enterで保存し、一覧で所属を確認 | c0e2ad7、s-89・91・103 |
| 作業枠 | Taskに関連する13:00–13:30のTimeBlockを作成し、一覧と時間割で確認 | c0e2ad7、s-107・109・111・113 |
| 900幅と再起動 | 作業枠・所属を読み戻し、詳細は本文の下に属性を配置 | c0e2ad7、s-120・122／frame-e16156b8・frame-a6e928ac |
| 実アプリ参照後の空状態・作成画面 | Linearと同じ1920x1200で比較。青紫のCTA、状態pill、枠なしのタイトルを確認 | 0486043、s-148・150／frame-99f36998・frame-e656b4ae |
| 状態viewの切替 | 作成した行を「すべて」で表示、完了後は「完了」で表示し、「未完了」から消える | 0486043、s-152・154・156・158・160・163 |
| 780幅の一覧 | rail表示と保存データの復元、長い日本語と属性の配置を確認 | cf1f21e、s-166／frame-fbc3f20f |
| 時間割の初回幅 | 初めて開いた時点でカードが主領域の幅に収まり、3件の時間と関連を表示 | cf1f21e、s-168／frame-029152ee |
| 最終の作成画面と終了 | 狭い幅に収まり、pill形状のCTAを表示。Xで時間割へ戻り、アプリを通常終了 | cf1f21e、s-170・172・173／frame-839c5fc9 |

観測IDはこの作業のComputer Use記録であり、公開画像ファイルのパスではない。操作APIが受理しただけのケースを成功に含めていない。

## 見つけて直した問題

- 透明な入力枠のshadowが内側まで塗られていたため、共通Button・Input枠・一覧のfocusを1px borderへ変更した。
- メニュー起点はButtonのmouse-downを親へ伝播させず、公開on_clickから既存PopoverのConfirmへ接続した。マウスの1操作で二度切り替わる経路を避ける。
- 作成ダイアログへ移した入力が起動時のfocus先として残っていたため、常時表示するrootへ変更した。未保存確認を取り消すと作成ダイアログへ戻る状態も保持する。
- 時間割側の280px一覧では補足表示を省いてタイトル幅を確保した。補足は読み上げ用の名前に保持する。
- 時間割の初回表示で前のフレームのscroll boundsを使っていたため、a50b5ccで現在の列幅からカード幅を求めるよう変更した。

途中の6323e34はButtonに存在しないon_a11y_actionを指定したためCIでコンパイルが失敗した。c0e2ad7では公開on_clickを使い、CIと上記の実機確認を行った。失敗した版の実行物は確認に使用していない。

## 実アプリの参照と最終起動版

インストール済みLinear 1.32.4をComputer Useで起動した。初回は本文が黒かったが、Ctrl+R後にInbox、All issues、Create issueを取得できた。実画面はs-134・136・138、作成画面の入力部品はs-140で確認した。Linear上でIssueの入力・作成や設定変更は行っていない。REAの解析shellは1.32.8であり、同じ版とは扱わない。

この比較と公開定義を使い、主ヘッダー44px、作成タイトル18px/600、elevated面、青紫CTA、状態pill、空状態を修正した。作成画面は上部帯42pxと13vhの余白を合わせて配置する。sRGBへ変換したprimaryの一部には、文字コントラスト4.5:1を維持する明度補正がある。計算した38組の結果は[contrast-report.json](../design/contrast-report.json)を参照する。

最終版cf1f21eは公式取得スクリプトでAPI由来とpackageの全ファイルhashを照合し、通常の起動先へ登録した。`scripts/start-demo.ps1 -WhatIf` は同じSHAを選び、`Demo none` と対象zoneの今日を表示した。実行やDB作成を伴う確認ではない。実機操作には上表の専用QA DBだけを使った。

## 未解決・未検証の範囲

この環境のAXPressでは、追加先メニューの項目選択が反応しないケースを確認した。通常クリックによる選択は成功した。詳細のSelectもAXExpandでは開かず、通常クリックでは開き、Escapeでfocusを戻せた。Selectの候補行にはAXNameが空のものがある。メニュー起点の修正を、すべての支援技術操作の解決として扱わない。

IME変換中、Narrator、高コントラストの実機、全DPI、大規模GUI、入力から描画までの遅延は未検証。今回の再実装後のdragも上表のTimeBlock入力・保存とは別の未確認項目である。

Interの静的faceとvariable weight450・510・590の厳密な一致、認証後のLinear本体テーマのcomputed color、全画面・全状態の一致は未確認。公開コード、公式サンプル、インストール済みアプリの実画面を参照した実装であり、UI完成やLinearとの完全な同等性を示す記録ではない。
