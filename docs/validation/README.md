# 初期ローカル実装の検証記録

2026-11-09。実行した内容と、まだ確認できていない範囲を記録する。

## 自動検証

- `cargo fmt --all -- --check`: 成功。
- `cargo test --locked -p ultratasks-core --all-targets`: 22件成功。受け入れ14件、保存・DB移行の回帰8件。プロパティテストは64ケースの操作列を含む。
- `cargo test --locked -p ultratasks-desktop`: 4件成功。夏時間・曖昧な時刻・日跨ぎ・欠落した日付の境界を扱う。
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: 成功。
- Linuxネイティブの`cargo build --locked -p ultratasks-desktop`: 成功。
- `scripts/check_native_dependencies.py`: 選択されたデスクトップ依存グラフにWebView描画パッケージなし。

## 実画面の操作

Linux x86_64、Xvfb 1280×900、Mesa llvmpipeソフトウェアVulkan、Noto Sans CJK JP、debug buildで確認した。日本語の貼り付けは確認したが、日本語IMEの変換操作を実機で確認したという意味ではない。

1. 「保存・Undo の確認」を画面から日本語で追加し、SQLiteの保存内容と照合した。
2. Asia/Tokyoの2026-11-09 09:00〜09:30を割り当て、UTCの00:00〜00:30とタイムゾーンが保存されたことを確認した。
3. タスク完了→Undo→Redo→Undoを画面で操作。完了状態と、作業枠が保持されることを確認した。
4. 作業枠の30分移動→Undoを操作し、時刻が戻ることを確認した。
5. アニメーションON/OFFを操作した。GPU実機のフレーム時間の測定ではない。
6. 検証用プロセスを終了し、最新バイナリで同じDBを再起動した。schema v1→v2移行後もタスク・revision・関連する作業枠・時刻が保持されたことを照合した。

[再起動後の実画面](desktop-smoke.png)。生成したコンセプト画像ではなく、動くGPUIアプリのスクリーンショット。データは検証用の架空データ。

Xvfbには通常のウィンドウマネージャーがない。検証用ウィンドウの強制破棄後はプロセスを明示的に終了したため、この手順だけでWindowsの通常終了動作を合格と扱わない。

## コアの性能

[生の測定結果](core-benchmark-10000.json)を保存した。Linux / AMD EPYC 9V74、Rust 1.99.1、release build、SQLite 3.51.1 / WAL、クラウド環境の一時ファイルDB、1万タスク＋100作業枠、ウォームキャッシュ100サンプルでの記録。

| 操作 | p95 |
| --- | ---: |
| タスク先頭50件 | 127µs |
| 末尾1件の部分一致検索 | 1,831µs |
| 作業枠100件 | 216µs |
| タスク完了と履歴の永続保存 | 1,226µs |

UI描画・ネットワーク・対象実機の値ではない。絶対的な性能保証には使わない。問い合わせプランで不要なソートを見つけ、`(deleted, id)`の索引とv1→v2の移行を追加した後の結果である。

## 残る検証

WindowsのビルドはGitHub Actionsで別途確認する。Windows実機のIME・読み上げ・DPI・通知・GPU性能、Androidのビルド・実機操作は未確認。Androidの依存解決を実行した結果は[別記録](../android-spike.md)にある。
