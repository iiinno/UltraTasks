# PCプロトタイプの状態

2026-11-12更新。Windows PC版はリモートでのビルド・検証を通過し、配布したrelease版で少数データの実画面回帰を確認した。Androidは後続工程。元の設計文書とdesign資産を保持し、Rust/GPUIによるネイティブ実装を継続している。

## 配布版と起動

- 検証したsource: `7935d2dcc334c31576e81773cb724d4add277142`。
- [Windows CI run 43111292958](https://github.com/iiinno/UltraTasks/actions/runs/43111292958)、attempt 1、`completed / success`。full検証・package・配布upload・spread/dense測定・証拠uploadが成功。依存cache保存はskipped、release build cacheは保存成功。
- artifact: `windows-release-7935d2dcc334c31576e81773cb724d4add277142-1`。
- リポジトリは公開済み。実装は[`prototype/pc-first`](https://github.com/iiinno/UltraTasks/tree/prototype/pc-first)にあり、mainへのマージはしていない。

取得したartifact内の `Start-UltraTasks.cmd` で同梱exeを起動する。Rust、元checkout、ローカルビルドは不要。DBの既定は `%LOCALAPPDATA%\UltraTasks\demo\demo.sqlite`。新規保存先では空のDBから始まり、既存DBはそのまま開く。常に `--demo none` を使い、fixtureを生成しない。表示日は `Asia/Tokyo` の今日。取得・選択方法は[開発手順](development.md#成果物を取得する)を参照する。

今回ワークスペースへ取得した版は `out/ultratasks/downloads/release-43111292958-preview/app/` にある。APIの実行元・source・検証step、ZIP digest、全44ファイルのhashを確認済み。ZIPのSHA-256は `f31b46c21ac429ab0fe3cc3f5429c10c6527f8f9e98aaa4464a98ed3573fbe4d`。この先行取得ではAppDataの `current.json` を変更していないため、同フォルダー内の起動入口を使う。GitHubのartifact保持期間は14日であり、恒久的なGitHub Releaseではない。

配布後の追補として、Windows PowerShell 5.1で起動設定の確認 `-WhatIf` だけが失敗する問題をソース側の `scripts/start-demo.ps1` で修正した。読取streamからhashを計算し、WhatIfに抑制される内部のpath処理を通さない。実配布exeのpreviewと正常な合成入力は終了0、改ざん入力はpreview/通常経路とも終了1で拒否し、DB・ログ・アプリの生成がないことを軽量検査した。証拠は `out/ultratasks/evidence/whatif-launcher-fix-20261112/`。配布済みbundleのファイルは変更しておらず、その同梱launcherで `-WhatIf` を使う場合は既知の制限が残る。通常起動は終了コード0まで確認済み。この追補はRust本体を変更せず、上記のCI・性能値は引き続きsource `7935d2d` のものとして扱う。

## 統合した実装

| 範囲 | 実装 |
| --- | --- |
| 基盤 | Rust 1.99.1 MSVC、2クレート、固定lock、remote Windows検証、GPUI Kit 0.9.0。WebViewなし |
| 保存 | `Store`のCRUD、SQLite WAL/FULL、期待版、単一DBワーカー、関連削除、Undo/Redo、結果不明時の停止と再照会 |
| 日付 | 日付期限・時刻期限・終日予定・作業枠を区別。同梱tzdb、欠落/重複時刻、日跨ぎ、終了排他 |
| 画面 | 今日のタスクと時間割、追加・編集・完了・削除、リスト/カレンダー、検索・絞り込み、日付移動、時刻指定、最小ドラッグ、可視行描画 |
| 表示 | light/dark/system、OS高コントラスト、system/reduced/noneの動き設定、入力面、focus輪郭。設定はDB横へ保存 |
| 検証器 | 公開APIの独立受入、remote合成fixture、core計測、Windowsプロセス観測、元sampleの統計集計 |

保存ファイルのhardlink別名による二重openはSQLite接続前に拒否する。Unicode時刻入力のpanic、破棄した保存要求と別フォームの混同、保存後の古い親Task版、密集予定の占有範囲、短い作業枠と時刻ラベルの見切れを修正した。入力エラーは入力付近の日本語表示にした。依存はrusqlite 0.41.0 / libsqlite3-sys 0.39.1 / SQLite 3.54.1へ更新し、progress handlerの登録・解除失敗も扱う。[公開前の確認](security-review.md)に監査範囲と限界を記録している。

## リモート検証

GitHub-hosted `windows-2025`、Rust 1.99.1/MSVC、releaseでfmt、Clippy、全targetのテスト、製品/計測exeのbuild、依存検査を通過した。Rustのテスト結果は **101成功・0失敗・2 ignored**。ignoredの2件は親テストが環境とDBを準備して別プロセスで呼ぶ専用入口であり、通常の一括列挙では直接実行しない。

CIのnative smokeは `NOT_RUN`。以下の手動回帰と区別する。開発PCでRustビルド・check・test・Clippy・bench、fixture生成、重い計測は実行しない。検証済み成果物の起動と少数操作だけを行う。

## 配布版の実画面確認

source `7935d2d` の配布exeを、専用の空DBから起動して確認した。

- 日本語Taskの追加とローカル保存。
- 開始時刻「あ」でクラッシュせず、入力を保持して日本語エラーを表示。
- 「9時」を保存して09:00–09:30の作業枠を表示。30分枠の一行タイトルと09:00ラベルが見切れないことを確認。
- 完了後も作業枠を保持し、Undoで未完了へ復帰。
- 通常終了と再起動後にTask・作業時間・履歴を復元。再起動後のRedo/Undoも確認。
- `--window-size 900x720` で一覧と時刻編集を確認。日付/時刻欄、保存操作、右の時間割が収まることを確認。
- 同梱 `Start-UltraTasks.cmd` からも同じ専用DBを正常起動し、Taskと作業枠の復元を確認。
- 確認用アプリはすべて通常終了し、対象プロセスが消えたことを確認。

記録はワークスペース直下 `out/ultratasks/evidence/gui-verified-7935/interaction-review-final.md`。日本語の値設定はIME変換中の確認ではなく、900指定の一画面は全DPIの検証ではない。

初期統合版では、Task編集、同一Taskへの二枠、日本語検索、light/dark切替、独立した終日Eventも確認した。記録は `out/ultratasks/evidence/gui-initial/interaction-review.md` と `persisted-result.json`。今回の配布版で全操作を再実行したという意味ではない。

## 性能と残る確認

配布版と同じrun `43111292958` で、GitHub-hosted Windows runner上のcore本測定を完了した。1千/1万Task、spread/dense、10操作の40条件。各条件5試行×1000回、計200,000回がすべて成功・検査済みで、failure、timeout、deadline超過、未実行、処理時間の欠測は0。別途20,000回のwarmupは統計から除外した。

1万Taskでの処理時間p95（ms、5試行合算、失敗・外れ値を除外しない母集団）は次のとおり。

| 操作 | spread | dense |
| --- | ---: | ---: |
| 検索200件＋COUNT | 14.6559 | 17.8808 |
| COUNTのみ | 2.1469 | 2.2132 |
| 時刻予定取得＋COUNT | 2.9060 | 4.9102 |
| 終日予定取得＋COUNT | 0.5061 | 1.3397 |
| 追加 | 1.0313 | 1.0084 |
| 編集 | 0.8988 | 0.9401 |
| 完了 | 0.9030 | 0.9187 |
| Undo | 0.8248 | 0.8447 |
| Redo | 0.7939 | 0.8136 |
| 2枠目追加 | 1.0961 | 1.0862 |

40条件の合算p95最大は17.8808msで20ms未満。ただしdense・1万件検索の第1試行p95は **20.4053ms**、同条件の合算p99は22.7010ms、最大31.6249msだった。全条件の最大観測はdense・1千件・完了の41.6042ms。timeoutの判定値は5,000msであり、timeoutゼロは20ms超過ゼロを意味しない。全試行で20ms目標を達成したとは言えず、次の性能調査は検索の内訳と試行変動を優先する。

環境は `windows-2025` / image `win25-vs2026 / 20261103.298.1`、Windows Server 2025 build 26100、AMD EPYC 7763、VM割当2 core/4 logical CPU、RAM約16GiB、Microsoft Virtual Disk、High performance、Rust 1.99.1/MSVC/release。前runのEPYC 9V74とは環境が異なるため、速度の改善率・退行率を算出しない。上位プロセスCPU情報は取得エラーで欠測。

証拠artifactは `windows-evidence-7935d2dcc334c31576e81773cb724d4add277142-1`。取得先はワークスペース直下 `out/ultratasks/evidence/run43111292958-summary-4nlw349_/`。API由来とZIP digestを照合し、必要なsummary/run/環境の16ファイルを確認した。raw全量のローカル再解析は行っていない。

[計測方法](performance-method.md)に処理時間の範囲、統計、fixture、未観測項目を記載している。Storeの同期呼出しを測り、検索は3条件の混合、取得は先頭最大200件とCOUNT、更新は同じTaskへの反復。fixture生成・呼出し後の正当性検査・後片付け・ログ書込みは計時外。queue waitと画面提示は200,000回すべて未観測であり、0msとして扱わない。

IME確定と送信、貼付、全キーボード経路、ドラッグ取消、DPI/文字拡大、Narrator、1千/1万件の実GUIでの入力・スクロール・起動・アイドルは残る確認。coreの処理時間はUIの画面提示やユーザーPCの性能ではない。120Hzや世界最速を達成したとは主張しない。

同期、通知、繰り返し、入出力、外部連携、AI/MCPは今回の実装範囲に含めない。
