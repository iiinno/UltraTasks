# 公開前のセキュリティ確認

2026-11-12。対象はUltraTasksのPCプロトタイプと、リポジトリを公開したときに閲覧可能になる既存履歴・GitHub上の情報。ローカルのRustビルドは実行せず、読み取り、静的確認、小さい合成テストを使った。

## 秘密情報と公開対象

基準のmainは `2b73c1bba7a2aca6a38954615573815f00457abd`。mainと旧 `feat/local-first-foundation` の2ブランチ、タグ0、6コミットの199履歴blob、PR #1の自動mergeを確認した。mergeのtreeは検査した旧ブランチと同一だった。

基準時の122作業ファイル、画像3点、生成画像のmetadataも確認した。資格情報、秘密鍵、実接続文字列、実カレンダーデータ、個人メールの未解決候補は見つからなかった。テスト用URL、`.test`のカレンダーUID、checksum、公開証明書は秘密情報と区別した。以後の修正は公開前の増分確認対象とする。

GitHub側はPR本文と編集履歴、6 Actions run、22 job、39注記、15,658行の既存ログを確認した。認証値はGitHub側でマスクされていた。過去のPRとチェックには支払い失敗または利用上限に関する一般的な注記が残るが、金額や決済情報は含まれていなかった。

旧artifact7件は本人のリポジトリとブランチ、確認した3つのcommitに由来する。取り込み経路と関連ソース105 blobを確認し、ユーザーSecretsや実データを埋め込む経路は見つからなかった。バイナリ全域の逆解析・再現ビルドは行っていない。既存Issue、レビュー、commitコメント、Release添付はなく、WikiとDiscussionsは無効だった。

## 見つかった問題と対応

| 問題 | 対応と確認 |
| --- | --- |
| 外部PRのartifactが別の信頼済みSHAを名乗れる | `artifact-policy.ps1`でGitHub APIの実repository、head repository、event、branch、head SHA、workflow path、完了と成功を照合する。許可するのは本人のmainまたはprototype/pc-firstのpush/dispatch。取得前に拒否し、PR成果物を配布経路へ入れない。独立した担当が36ケースの合成検査を再実行して成功した。 |
| 計測ログ内のパスを再集計時に開いてしまう | metadataのパスを説明値として扱い、open/stat/hashしない。記録された実行時hashは未検証申告値とし、入力ファイルから計算したhashと分ける。symlink/reparseによる入力外への読み取りとCSV数式文字列も対処した。独立した担当が8ケースの合成検査を再実行して成功した。 |
| 同梱SQLiteが既知のDB/WAL情報漏洩問題に該当 | [RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html)を確認。細工されたDBとWALを開く条件の問題。rusqlite 0.41.0と修正済みlibsqlite3-sysへ更新する準備を行った。Cargo.lockへの反映と実行検証はリモートで行う。 |

現在のアプリは共有commands、パラメータ化したSQL、操作主体とDBを区別するUndoを使う。診断ログはタスク本文を収集せず、GPUIのHTTPクライアントを通信可能な実装へ差し替えていない。CIは読取権限と資格情報を保持しないcheckoutを使用する。

## 検証と配布の区別

ソース公開と、修正済み実行ファイルの配布は別に判定する。manifestを変更しただけではSQLite更新済みとは扱わない。初回のActionsでは必要なら対象依存のCargo.lock候補だけを生成し、未検証・配布不可と記録する。候補を確認してcommitした後のrunで、固定lockによるbuild・test・依存確認を行う。

配布物は成功した信頼済みrun、APIのSHA、実行ファイルとpackageのhashを照合して取得する。CIで実ネイティブ画面を操作していない場合はNOT_RUNと記録する。CIの性能値を利用者PCの性能と呼ばない。

検査は指定範囲の確認であり、未知の脆弱性や秘密情報の完全不存在を保証するものではない。不可達Git object、ignoredのローカルファイル、外部依存内部の完全監査は対象外。修正後のRustの型・実行検証はリモートCIの結果で判断する。
