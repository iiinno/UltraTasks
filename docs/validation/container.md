# セルフホスト用コンテナの検証

2026-11-09、Linux x86_64、Docker 29.9.2 / vfs、Rust 1.99.1 releaseで実施した。サーバーのソースをDocker内でコンパイルし、完成した実行イメージを起動して検証した。外部公開はしていない。

完成イメージは88,224,237 bytes（約84.1 MiB）。ID、固定したベースイメージのdigest、確認項目の機械可読記録は [server-container.json](server-container.json)。異なるOS・CPUのサイズや性能を保証する測定ではない。

## ビルド

[Dockerfile](../../apps/server/Dockerfile)は公式Rust/Debianイメージをdigest固定し、`cargo +1.99.1 build --locked --release -p ultratasks-server` を実行する。BuildKitでソースを読み取り専用マウントし、registryとtargetはキャッシュマウントに置く。vfs環境でもソースのCOPYごとに大きなビルダーの複製を作らない構成にした。

管理環境では既存プロキシ設定を維持し、セッションCAを `--secret id=proxy_ca,src="$CODEX_PROXY_CERT"` で渡した。CargoのTLS検証を有効にしたままダウンロード・コンパイルが成功した。完成イメージの公開CA束にセッションCAが含まれないこと、ビルド用secret・ソース・Cargo toolchainが存在しないことも確認した。

## 実コンテナで確認したこと

- UID/GID 10001、読み取り専用root filesystem、全capability削除、`no-new-privileges`、専用データvolumeで起動する。
- ホスト側の公開は `127.0.0.1` のランダムポートだけ。`/health` が成功し、認証なし・無効トークンのAPI要求は401になる。
- コンテナ内CLIで発行した端末トークンを用い、HTTP同期でタスクの作成・取得・変更・削除を行う。削除はtombstoneとして残る。
- 同じ操作ID・本文の再送は最初の結果を返し、同じIDで本文を変更すると拒否する。
- 古い版の編集・削除済み項目の復活を競合として返す。
- `docker stop` の既定SIGTERMで正常終了コード0。再起動でデータと認証は保持され、サーバーのincarnationは変わる。
- トークンを失効すると、同じ起動中サーバーへの次の要求が401になる。

検証用トークンは値をログへ出していない。使用したコンテナ・volume・資格情報ファイルは終了後に削除した。

## 未検証

Caddyを含めた外部DNS・証明書発行・公開TLS運用、別CPUアーキテクチャ、大規模運用時の負荷は今回のコンテナ検証に含めない。逆プロキシ設定は [サーバー手順](../../apps/server/README.md)で準備できるが、そのホストで別途確認する必要がある。
