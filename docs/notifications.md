# リマインダーとOS通知

通知の定義は共通coreの`Reminder`、配信状態は端末専用のDBに保存する。通知DBを同期・JSON出力へ混ぜない。`At`は指定した瞬間に1回、`Before`は対象の開始・期限からの分数で、繰り返し系列では各回に展開する。完了済みのタスク、削除した対象、スキップした回、無効にしたリマインダーは新しい通知計画に含めない。

日付だけの期限・終日は、呼出側が渡すIANAタイムゾーンの深夜を基準にする。深夜が夏時間移行で欠ける場合は同日の最初の有効な分、重複する場合は早い瞬間を選ぶ。丸一日存在しない日付はエラーにする。単発・繰り返しの計算はcoreと共有し、通知専用の繰り返し規則を増やさない。

## Windows

`ultratasks-notifications`はWinRTの`ScheduledToastNotification`へ登録する。将来の通知時刻はWindowsが保持するため、登録後にUltraTasksのプロセスを閉じてもアプリ内タイマーには依存しない。デスクトップは起動時・保存後・同期等の変更後・アイドル時の定期処理で再照合する。

```powershell
ultratasks-notifications.exe --database C:\Data\ultratasks.db --timezone Asia/Tokyo --once
ultratasks-notifications.exe --database C:\Data\ultratasks.db --timezone Asia/Tokyo --install-worker
ultratasks-notifications.exe --database C:\Data\ultratasks.db --remove-worker
```

`--install-worker`は明示的に実行した時だけ、現在のユーザーの対話ログオン時とログオン中の1時間ごとに実行するWindowsタスクを登録する。管理者実行・パスワード保存を要求しない。実行ファイルを移動したら再登録する。`--remove-worker`で解除する。通知を無効化する時はデスクトップの`--disable-notifications`、または通知CLIの同じオプションで一度照合し、登録済みの将来の通知を取り消す。ワーカーを使っている場合は無効化した設定で再登録するか解除する。

予定する範囲は既定で先30日、APIでは1〜90日。ワーカーを登録せず30日以上アプリを起動しなければ、それ以後の繰り返し通知はOSへまだ登録されていない。Windowsの通知設定、集中モード、ログオン状態、OSの制約による表示・音までは保証しない。登録失敗や権限拒否はエラーとして報告し、DBへの保存成功と区別する。

正規化したDBパスごとに異なるAppUserModelIDを使うため、テスト用DBや別プロファイルの通知を取り消さない。各通知には16文字の安定IDを割り当て、衝突はOS操作前に検出する。編集時は同じIDの将来の通知を置換する。OS処理とSQLite確定の間で停止しても、再実行は同じIDへ登録する。既にOSが時刻を管理していた通知は、起動後にキューから消えていても再表示しない。

新たに見つかった過去の通知は既定15分以内なら一度表示し、それより古ければ見送る。メタデータだけの変更で同じ通知時刻を再表示しない。配信音の厳密なexactly-onceは保証しない。通知を受け取った後の履歴はユーザーが消すまで保持し、15分の取得猶予を通知センターの保存期限として使わない。

## Android

[ネイティブシステムホスト](android-system.md)が`AlarmManager`、通知チャンネル、通知権限、起動・時刻・タイムゾーン変更のReceiverを持つ。Rustの同じ通知計画を利用する。Android 12以降の正確なアラーム権限がない場合はOSの非正確なアラームを使い、状態に`inexact_os_alarm`を記録する。権限要求用のAPIは明示的なユーザー操作から呼ぶ。

Doze等で実際のアラーム配信が遅れた場合、そのアラームIDだけは最大24時間まで受け付ける。ほかの通知の起動時取得は15分。通知直前にも対象をcoreで再確認し、削除・完了・時刻変更を反映する。12時間ごとの保守アラームで将来範囲を延長する。再起動・パッケージ更新後にも再登録する。OS設定でアプリを強制停止した場合や端末メーカー独自の省電力制限について、実機検証は未実施。

## 検証

Linux上で、通知計画と永続キューのテストを実行した。完了・無効化・猶予期限、繰り返し例外、日付だけのタイムゾーン、再起動、OSキュー消失、時刻変更、権限拒否、登録後中断、古い通知の再表示防止を確認する。

```sh
cargo test -p ultratasks-notifications
cargo run -p ultratasks-notifications -- --database test.db --timezone Asia/Tokyo --plan
```

Windowsトーストの実表示と、Androidのスリープ・再起動・権限変更を含む実機配信は未確認。ビルドや決定的テストの成功と、OS上の配信確認を分けて扱う。
