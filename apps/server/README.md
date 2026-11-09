# 個人用セルフホスト同期

`ultratasks-server` は認証付きHTTP APIと、同じ同期クライアントを使うCLIを提供する。ローカル編集はネットワークがなくても確定し、同じトランザクションに送信待ちを記録する。UIはこのライブラリを呼び出せる。デスクトップ画面の同期設定UIは別途接続する。

```sh
cargo build --locked --release -p ultratasks-server
ultratasks-server --credentials devices.db add-device --name windows --token-output windows.token
ultratasks-server --database server.db --credentials devices.db serve
cargo build --locked --release -p ultratasks-sync
ultratasks-sync --database local.db --server http://127.0.0.1:8374 --token-file windows.token once
ultratasks-sync --database local.db --server http://127.0.0.1:8374 --token-file windows.token watch --interval-seconds 30
```

`watch` は画面と別のプロセスで動き、失敗時は最大5分まで待ち時間を延ばす。Ctrl+Cで終了する。`status` で送信待ちと競合の有無、`conflict` で双方の値を確認できる。トークンは引数やログへ出さず、ファイルのパスだけを渡す。`--no-default-features` でライブラリを組み込むと、モバイル向けにサーバーとCLIの依存を除外できる。

端末トークンはOSの乱数による256bit値。サーバーにはSHA-256ハッシュのみ保存し、`add-device` は平文を新規ファイルへ一度だけ書く。Unixでは0600で作成する。Windowsでは出力先を本人だけが読めるディレクトリに置く。ファイルやトークンをGitへ追加しない。端末ごとに発行し、`list-devices` と `revoke-device UUID` で失効できる。認証DBとデータDBを別のファイルにする。

サーバーの既定リスナーは127.0.0.1。遠隔同期はHTTPSが必須で、クライアントはループバック以外のHTTP、URL内の認証情報、リダイレクトを拒否する。TLS終端はCaddy等のリバースプロキシへ任せる。非ループバックで待ち受ける場合は `--behind-tls-proxy` が必要で、そのHTTPポートをインターネットへ公開しない。

```sh
export ULTRATASKS_DOMAIN=tasks.example.org
docker compose -f apps/server/compose.yaml up --build -d
docker compose -f apps/server/compose.yaml exec ultratasks ultratasks-server --credentials /data/devices.db add-device --name phone --token-output /data/phone.token
```

DNS、80/443の到達性、証明書発行、ディスク保護はホスト側で設定する。サーバーイメージはLinux上で実ビルドし、非root・読み取り専用ルートのコンテナで認証、同期CRUD、再送、競合、失効、再起動後の保存を検証した。Caddyの公開TLS運用・証明書発行は未検証。[検証記録](../../docs/validation/server-container.json)にイメージIDと実施内容を残す。自動更新の前にはイメージのdigest固定と検証を行う。

通常のビルドは `docker build -f apps/server/Dockerfile -t ultratasks-server .`。BuildKitを使用し、依存とコンパイル出力をキャッシュマウントへ保存する。Rust 1.99.1のDebian slimイメージでビルドし、実行イメージにはバイナリと公開CAルートだけをコピーする。実行ユーザーはUID/GID 10001、データは `/data` に置く。

管理プロキシに独自CAがある環境では、一時的なBuildKit secretでCargoへ渡せる。通常のホストではこのsecretは不要。検証環境では次の形式で実行した。

```sh
docker build --secret id=proxy_ca,src="$CODEX_PROXY_CERT" -f apps/server/Dockerfile -t ultratasks-server .
```

このCAはビルド中のHTTPS検証にだけ使い、完成イメージへ保存しない。HTTPS同期クライアントをコンテナから実行する場合の独自CAは、別途その実行時に読み取り専用でマウントして設定する。

## 同期と競合

- プロトコル版は1。更新はサーバー版を条件にした原子的なレコード集合として送信する。参照先は読み取り条件として照合し、無関係なタスクの編集で共有リストの版を進めない。
- 送信前に操作UUIDと本文をSQLiteへ固定する。応答消失や再起動では同じ要求を再送し、サーバーは最初の成功応答を返す。同じUUIDで本文を変える要求は拒否する。
- オフラインの競合は双方の値を保存し、黙って上書きしない。`resolve-conflict --keep local` または `--keep remote` 後に再同期する。判断後にサーバー側で変更があれば、版を再照合して再び競合する。
- 競合を表示した後にローカル編集が増えた場合、その古い判断を拒否する。ライブラリの `sync_refresh_conflict()` で新しい編集を含む要求を再作成する。競合履歴は解決後も保管する。
- 削除記録・変更ログ・成功応答は現在は期限を設けず保持する。古い端末の復活を防ぐ代わりにサーバー容量は増える。将来の圧縮は全端末の再同期手順と一緒に導入する。
- 1回の呼び出しは最大100送信・100取得ページ。残りがある場合は次回続行する。常時接続や自動ポーリングの頻度はホストが決める。

## バックアップとサーバー復旧

稼働中のSQLiteファイルだけをコピーしない。SQLiteオンラインバックアップAPI、またはサービス停止後の整合したファイル集合を使う。認証DBも別に保存する。データDBを別端末へ複製した場合は、その端末用の新しいトークンを発行する。

データだけを復旧する場合は現在の認証DBを維持する。認証DB自体を古いバックアップへ戻すと、その後に失効した端末の状態も巻き戻るため、復旧後は端末トークンを失効・再発行してから遠隔接続を再開する。

サーバー起動時にはincarnationを変える。これにより、古いバックアップが同じサーバーIDを復元しても見逃さない。通常の再起動で既知のレコードが変わっていなければ自動照合して継続する。サーバー復元や食い違いがある場合は送信待ちを保存したまま止まり、`resolve-reset --endpoint URL --token-file FILE --keep local|remote` で明示的に解決する。両側の全データを復旧履歴に残し、選択後の上書きにも版照合を適用する。

初期実装の上限は、削除記録を含めて10万レコード、ドメインデータのJSON表現が約32MiB。上限を超える編集・取り込みはトランザクションごと失敗する。HTTP本文上限は64MiB。同時要求4件、処理タイムアウト30秒で、大きな同期もプロセスを無制限に占有しない。ページ化された全体復旧を実装するまでは、この上限を明示して運用する。

専用クライアントCLIでは `resolve --keep local|remote`、`refresh-conflict`、`recover --keep local|remote` を使う。復旧確認からサーバー取得までの間にローカル編集が増えた場合は、古い確認による上書きを拒否する。サーバー側に新しい関連項目が増えた場合、元の削除要求を優先すると関連整合性により再び競合することがある。その場合はサーバー側を取り込んで関連項目を確認してから、必要な削除を新しいコマンドとして行う。

サーバーDBはクライアントDBと分ける。既存のローカルDBをサーバーにする場合は初回起動で全レコードを変更ログに登録し、その後のローカルコマンドもサーバーログへ記録する。サーバーになったDBを他のサーバーのクライアントとして接続することは拒否する。戻す場合はJSONを別のローカルDBへ取り込む。

共有ユーザー、チーム権限、E2EE、外部カレンダーとの常時同期は対象外。自分の端末には同じ個人データ全体への権限を与える。
