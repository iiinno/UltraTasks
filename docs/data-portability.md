# 入出力とバックアップ

UIから独立したRust APIとCLIで利用できる。予定やタスクの変更と同じ検証、関連の検証、トランザクション、Undo、同期用の変更記録を通す。

## 全データのJSON

`Engine::export_json()`は`format: "ultratasks"`、`version: 1`、`records`を持つJSONを返す。Task、TaskList、Calendar、Event、TimeBlock、Reminder、繰り返し規則・例外、ID・更新版・削除記録を保存する。出力は一つの読み取りトランザクションで確定し、保存値と検索用の時刻列が一致することも確認する。

`Engine::import_json(input, mode)`には二つのモードがある。

- `ImportMode::Empty`: 削除記録も含めてレコードが存在しないDBへ取り込む。
- `ImportMode::Merge`: 新しいIDだけを追加する。同じID・内容・更新版・削除状態なら再取り込みを無視する。いずれかが異なる既存IDは競合エラーになる。既存の編集や削除を上書きしない。

一回の取り込みは全体で一つの操作になる。後半の入力不正、関連切れ、重複ID、保存エラーがあれば全体を戻す。成功後は一回のUndoで取り込みを取り消せる。同期の送信待ち記録も同じトランザクションに含む。

上限は32 MiB、100,000レコード。ファイルを読み込むホストでも32 MiBを上限にする。未知の形式番号・フィールドは拒否し、新しいアプリが追加した情報を古いアプリが黙って消すことを防ぐ。

JSONには端末ごとの操作履歴、同期カーソル、通知の配信済み記録、認証情報を含めない。操作履歴まで復元したい場合はDBバックアップを使う。

## SQLiteのバックアップと復元

`Engine::backup_to(new_path)`はSQLite Online Backup APIを使う。稼働中のDBがWALを使っていても確定済みの変更を取り込む。DBファイルの単純コピーではない。

`Engine::restore_backup(source, new_path)`は元ファイルを読み取り専用で開き、新しい保存先に復元する。既存ファイルを上書きするAPIは用意していない。元ファイルのアプリ識別子、対応スキーマ、SQLiteの整合性・外部キー、保存された各エンティティと検索用の時刻列の一致を検証する。対応する古いスキーマはコピー側で移行する。未知の新しいスキーマやUltraTasks以外のDBは拒否する。

バックアップ・復元とも同じディレクトリ内の非公開の一時ファイルで作業し、検証と書き出しを終えてから完成ファイルを公開する。公開には上書きしないハードリンクを使うため、保存先のファイルシステムにハードリンクの対応が必要。WindowsのNTFSと一般的なLinuxファイルシステムで使用できる。競合中のバックアップが無期限に待たないよう、60秒で中止する。

DBバックアップは操作履歴、同期の送信待ち状態も保存する。別の端末に復元する場合、同期サーバーでその端末用の認証情報を発行する。同じ端末用トークンを複数の稼働中クライアントで共有しない。サーバー復元後の世代不一致は同期プロトコル側で検出・再照合する。認証トークンのファイルや環境変数はDBバックアップに含まれない。

## カレンダーのICS

`Engine::export_ics()`は`IcsExport { text, notices }`を返す。`notices`を利用者に表示すること。`Engine::import_ics(input, mode)`はJSONと同じ原子的な取り込みを行う。

対応する範囲は意図的に限定している。任意のiCalendarファイルに完全対応したとは扱わない。

| 項目 | 対応 |
| --- | --- |
| 単発VEVENT | タイトル、UTCまたはIANA TZID付き日時、終日DATE、終了を含まない範囲 |
| 日時 | 秒単位。日付だけの予定は時差で動かさない。TZIDは搭載IANAタイムゾーンDBで解決する |
| 終日DURATION | 正の`PnD`。省略時は1日 |
| 繰り返しVEVENT | DAILY / WEEKLY / MONTHLY、INTERVAL、COUNTまたはUNTIL、週次BYDAY、月曜WKST |
| 例外 | EXDATE、RECURRENCE-IDによる個別変更・取消。系列全体に対応するmasterが同じファイルに必要 |
| 非繰り返しVTODO | タイトル、任意の期限、未完了／完了。0または100%の進捗 |
| テキスト | UTF-8、75オクテット以内の折り返し、バックスラッシュ・カンマ・セミコロンのエスケープ |
| UID | UltraTasksのUUIDは維持する。外部UIDからは種類別の決定的UUIDを生成し、同じ入力の再取り込みを識別する |

対応外のRRULE、独自VTIMEZONE、浮動時刻、VALARM、DESCRIPTION、LOCATION、参加者、部分完了、VTODOの繰り返し等はエラーにする。IANA TZIDの参照を使い、独自VTIMEZONEを埋め込む形式には対応していないため、受け手も同じIANA識別子を扱える必要がある。作成・更新時刻やSEQUENCEなど配送上のメタデータは入力検証後に破棄する。

ICSで表現できないサブ秒の時刻や、DST重複時間の二回目をTZIDローカル時刻として出す場合は、別の瞬間に変換してしまわないよう出力を拒否する。EXDATEやRECURRENCE-IDも日付・壁時計の一致だけでなく、実際の生成開始時刻を照合する。

TimeBlock、Calendar／List所属、Reminder、更新版、削除記録、操作履歴はICSの対象外。出力時の`notices`で対象外の情報を示す。繰り返しタスクは期限だけを持つUltraTasksのモデルと、開始・終了・所要時間を持つVTODOの意味が一致しないため、ICSではなくJSONを使う。

検証ケースは[transfer.rs](../crates/ultratasks-core/tests/transfer.rs)と[system_adversarial.rs](../crates/ultratasks-core/tests/system_adversarial.rs)にある。JSONのID・関連・繰り返し例外、再取り込み、失敗時の全体取消、Undo、WALと履歴の復元、非対応スキーマ、DSTを跨ぐICS系列、例外の瞬間、UTF-8折り返しを実際のDBで確認する。
