# システム実装の検証記録

2026-11-09。実行した内容と、まだ確認できていない範囲を記録する。

## 自動検証

- `cargo fmt --all -- --check`: 成功。
- `cargo test --locked --workspace --all-targets`: Linuxで103件成功（追加のAI資格情報テストは別途成功）。core 71件には受け入れ、実v2スキーマからの移行、日付、繰り返し、容量、故障注入、入出力、独立レビューの回帰を含む。プロパティテストは64ケースの操作列を含む。
- `cargo test --locked -p ultratasks-desktop`: 4件成功。夏時間・曖昧な時刻・日跨ぎ・欠落した日付の境界を扱う。
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: 成功。
- Linuxネイティブの`cargo build --locked -p ultratasks-desktop`: 成功。
- `scripts/check_native_dependencies.py`: 選択されたデスクトップ依存グラフにWebView描画パッケージなし。
- 同期: 実TCPのHTTPサーバーと2端末DBで6件成功。オフライン競合、関連データの削除、応答消失、再起動、復旧、認証失効、リダイレクトへの資格情報非転送を検証。
- MCP・AI: 10件成功。実stdio子プロセスの初期化・権限・再送・更新版と、ローカルHTTP fixtureによる両AI方式・送信範囲・適用承認・連鎖削除の対象制限を確認。外部実モデルの品質評価とは区別する。
- 通知: 共通計画・配信台帳・Windows定期ワーカーのXMLと引数を10件検証。WinRT通知モジュールはWindowsターゲットでもコンパイル確認。OS実通知の成功を意味しない。
- Android: ARM64 Rust共有ライブラリとJavaホストをビルドし、APK署名・16KiB alignmentを検証。実Linux JVM→JNIで日本語、並列書き込み、再送、JSON/ICS、バックアップ、読み取り専用、パス境界、同期・AIの拒否条件を確認。Android端末での実行とは区別する。

## 実画面の操作

以下は最初の基盤コミット`a16055c`で実行した画面検証。Linux x86_64、Xvfb 1280×900、Mesa llvmpipeソフトウェアVulkan、Noto Sans CJK JP、debug buildで確認した。日本語の貼り付けは確認したが、日本語IMEの変換操作を実機で確認したという意味ではない。

1. 「保存・Undo の確認」を画面から日本語で追加し、SQLiteの保存内容と照合した。
2. Asia/Tokyoの2026-11-09 09:00〜09:30を割り当て、UTCの00:00〜00:30とタイムゾーンが保存されたことを確認した。
3. タスク完了→Undo→Redo→Undoを画面で操作。完了状態と、作業枠が保持されることを確認した。
4. 作業枠の30分移動→Undoを操作し、時刻が戻ることを確認した。
5. アニメーションON/OFFを操作した。GPU実機のフレーム時間の測定ではない。
6. 検証用プロセスを終了し、最新バイナリで同じDBを再起動した。schema v1→v2移行後もタスク・revision・関連する作業枠・時刻が保持されたことを照合した。

[再起動後の実画面](desktop-smoke.png)。生成したコンセプト画像ではなく、動くGPUIアプリのスクリーンショット。データは検証用の架空データ。

Xvfbには通常のウィンドウマネージャーがない。検証用ウィンドウの強制破棄後はプロセスを明示的に終了したため、この手順だけでWindowsの通常終了動作を合格と扱わない。

## コアの性能

[生の測定結果](core-benchmark-10000.json)を保存した。同期送信待ち・容量検査を追加したシステム実装で再計測した値。Linux / AMD EPYC 9V74、利用可能CPU 4、Rust 1.99.1、release build、SQLite 3.51.1 / WAL、クラウド環境の一時ファイルDB、1万タスク＋100作業枠、ウォームキャッシュ100サンプルでの記録。[初期基盤の旧結果](core-benchmark-foundation-10000.json)は別に保持する。

| 操作 | p95 |
| --- | ---: |
| タスク先頭50件 | 152µs |
| 末尾1件の部分一致検索 | 2,747µs |
| 作業枠100件 | 306µs |
| タスク完了・履歴・同期送信待ちの永続保存 | 3,725µs |

UI描画・ネットワーク・対象実機の値ではない。共有クラウドCPU上の変動を含む測定で、絶対的な性能保証には使わない。問い合わせプランで不要なソートを見つけ、`(deleted, id)`の索引とv1→v2の移行を追加した後の結果である。

[繰り返しの測定結果](recurrence-benchmark.json)も保存した。過去の単発予定1万件、日次100系列、COUNT 1万回・例外1000件の系列を含め、1日の101件取得がp95 8.252ms、例外の多い1系列の展開がp95 4.487ms。JSON約2.60MBの原子的取り込みは約792ms。いずれもrelease・100サンプルで、OS通知配信やGPUIフレーム時間は含まない。

## 残る検証

Windows・Linuxのシステムテスト、Windowsネイティブアプリ、AndroidシステムAPKはGitHub Actionsでも確認する。Windows実機のIME・読み上げ・DPI・通知・GPU性能、Android実機のTLS・資格情報保存・JobScheduler・Doze・アラーム配信は未確認。AndroidのGPUI画面の依存解決は[別記録](../android-spike.md)、システムホストのビルド条件は[Android資料](../android-system.md)にある。公開配布用の署名・更新配信、共有機能、常時外部カレンダー連携は今回の対象外。
