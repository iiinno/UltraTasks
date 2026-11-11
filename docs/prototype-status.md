# PCプロトタイプの状態

2026-11-12更新。今回の優先対象はWindows PC。Androidは後続工程とし、元の設計文書とdesign資産は保持している。保存、UI動作、画面、起動・計測基盤の分担作業を本体へ統合した。UI修正後のリモート検証は未実施。

| 範囲 | 統合した実装 |
| --- | --- |
| 基盤 | Rust 1.99.1 MSVC、2クレート、固定lock、Windows環境検出、fast/full、CI定義。WebViewを使わないGPUI Kit 0.9.0 |
| 保存 | `Store`の直接CRUD、SQLiteのWAL/FULL、期待版、単一DBワーカー、関連削除、Undo/Redo、結果不明時の停止と再照会 |
| 日付 | 日付期限・時刻期限・終日予定・作業枠を区別。同梱tzdb、欠落/重複時刻、日跨ぎ、終了排他 |
| 画面 | 今日のタスクと時間割、追加・編集・完了・削除、リスト/カレンダー、検索・絞り込み、日付移動、時刻指定、最小ドラッグ、可視行描画 |
| 表示 | 正式themeのlight/dark/system、OS高コントラスト、system/reduced/none、入力面、focus輪郭。設定はDB横へ保存 |
| 検証器 | 公開APIの独立受入、固定fixture、core計測example、Windowsプロセス観測、元sampleの統計集計 |

初期統合版は、80件のテスト、Clippy、releaseビルド、依存検査、実ウィンドウの起動確認を通過した。証拠はワークスペース直下の `out/ultratasks/evidence/integration/full-2/20261111T143042416Z-verify-full-9ecf6a07/` にある。作業コピーから古いcore生成物を持ち込む問題は、workspace識別と自作クレートだけの再生成で修正した。

その統合版を実画面で操作し、Taskの追加・編集、同一Taskへの二枠、完了、Undo/Redo、日本語検索、light/dark切替、再起動復元、独立した終日Eventを確認した。専用DBを読み取り検証し、Taskと作業枠のIDの関連、完了、日付範囲とDB整合性も確認した。記録は `out/ultratasks/evidence/gui-initial/interaction-review.md` と `persisted-result.json`。日本語の値設定を、IME変換中の操作確認とは扱っていない。

独立レビューで保存ファイルのhardlink別名に関する欠陥が見つかり、現在の本体は危険な複数リンクをSQLite接続前に拒否する。修正後のcore全73テストとClippyは通過した。新規回帰6件を含む。証拠は `out/ultratasks/evidence/parallel/storage/20261111T182511755Z-integrated-core-70d6bfc3/`。UIのUnicode時刻入力、失敗した保存要求の所有者、保存後の親更新版、密集予定、文字の見切れ、入力付近のエラー表示は本体へ統合済み。前任分を含むCLIの `--window-size` と起動時のウィンドウ寸法指定も統合した。所有領域の内容比較とAPIの静的確認までで、UI修正後のビルド・テスト・実画面の回帰は未検証。Windows PCではRustビルド・check・test・Clippy・bench、fixture生成、重い計測を実行せず、GitHub Actionsまたは選定したVPSでの検証を待つ。

起動入口 `Start-UltraTasks.cmd` とdesktop診断集計は本体へ統合済み。起動入口は専用デモDBを初回だけ生成し、以後は既存データを開く。限定テストと `-WhatIf` の引数確認までで、この入口からの実GUI起動は今後確認する。

残る確認は、統合したUI修正のリモート全体検証と実画面での回帰、IME確定と送信、貼付、全キーボード経路、ドラッグ取消、900px幅、DPI/文字拡大、Narrator、1千件・1万件の入力・スクロール・起動・アイドルである。性能計測器は短縮動作確認に成功したが、本測定は未実行。速度や120Hz達成の数値はまだ示せない。GitHub Actionsの実行、commit、pushはしていない。

起動・検証は[開発手順](development.md)、性能条件は[性能計測の方法](performance-method.md)を参照する。同期、通知、繰り返し、入出力、外部連携、AI/MCPは今回の実装範囲に含めない。
