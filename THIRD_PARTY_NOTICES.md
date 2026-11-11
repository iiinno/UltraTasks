# Third-party assets

実際に取り込んだ素材のみを記録する。正確なcommit SHA、upstreamのファイル、ローカルの配置、変更内容は [`design/upstream-lock.json`](design/upstream-lock.json) に固定する。

| Project | Imported material | License notice |
| --- | --- | --- |
| [Radix Colors](https://github.com/radix-ui/colors) | Slate / Blue / Jade / Amber / Tomatoのlight・darkの12段階の色。値を変えずJSONへ抽出 | [MIT license](third_party/radix-colors/LICENSE) |
| [Lucide](https://github.com/lucide-icons/lucide) | 選定した11個のSVGを無変更でコピー | [Full license](third_party/lucide/LICENSE): ISC、Feather由来部分のMIT通知を含む |

## Windowsネイティブ実装で追加した依存

元のdesign資産と通知は変更していない。以下はcrates.ioの固定版をCargo.lockのchecksumで検証する依存であり、upstream実装の直接改変はない。`licenses/<crate>-<version>/`の原文通知は同版のregistryソースから無変更でコピーした。SQLite依存の更新分は対応する固定commitの公開ソースから取得しており、出典と未反映のCargo.lockについては後述する。

| 依存・出典 | 固定版・commit | 保持した通知 |
| --- | --- | --- |
| [GPUI Kit](https://github.com/longbridge/gpui-kit) | 0.9.0、`7591a35eced761e3613c01e71ff04c10129dae1a` | [Apache-2.0](licenses/GPUI-KIT-APACHE-2.0.txt)、[assetsのLucide ISC/MIT](licenses/LUCIDE-ISC-MIT.txt) |
| [GPUI](https://github.com/zed-industries/zed) | gpui-pre 0.3.11、元のZed rev `f816e1f9263e3a8ef63c4baaa02e1441ce1c6038` | [Apache-2.0](licenses/GPUI-APACHE-2.0.txt) |
| [rusqlite / libsqlite3-sys](https://github.com/rusqlite/rusqlite) | 更新予定 0.41.0 / 0.39.1、commit・checksum・lock反映状況は下記 | [rusqlite MIT](licenses/rusqlite-0.41.0/LICENSE)、[binding MIT](licenses/libsqlite3-sys-0.39.1/LICENSE) |
| [Jiff](https://github.com/BurntSushi/jiff) | 0.2.37、`4100a7c71125b9523029566d1d18f8b227ecd18c` | `licenses/jiff-0.2.37/`のCOPYING・LICENSE-MIT・UNLICENSE |
| [Jiff tzdb](https://github.com/BurntSushi/jiff) | 0.1.8、`bc70277861e54973e8a9be91bc92c44de56719b5` | `licenses/jiff-tzdb-0.1.8/`のCOPYING・LICENSE-MIT・UNLICENSE |
| [UUID](https://github.com/uuid-rs/uuid) | 1.29.2、`5205a6ea18425e6781f2ac09e896d88153d9cd2d` | `licenses/uuid-1.29.2/`のApache-2.0・MIT |
| [Serde](https://github.com/serde-rs/serde) | 1.0.229、`7fc3b4c30c94f73a96ebd1553f2b090d928fc3a8` | `licenses/serde-1.0.229/`のApache-2.0・MIT |
| [serde_json](https://github.com/serde-rs/json) | 1.0.154、`20572a6d066958542d79cdd6cf1a7cd9f0c8e1f8` | `licenses/serde_json-1.0.154/`のApache-2.0・MIT |
| [futures](https://github.com/rust-lang/futures-rs) | 0.3.34、`705e6b5c0f06535b1aac1cb1989a172b3d45be8c` | `licenses/futures-0.3.34/`のApache-2.0・MIT |
| [chrono](https://github.com/chronotope/chrono) | 0.4.45、`170338250e836976a211e64728ec956e45e78a39` | [LICENSE.txt](licenses/chrono-0.4.45/LICENSE.txt) |
| [windows](https://github.com/microsoft/windows-rs) | 0.62.2、`32c3144490c016fe496a0aed769bce60987a2e9d` | `licenses/windows-0.62.2/`のApache-2.0・MIT |
| [windows-sys](https://github.com/microsoft/windows-rs)（Windows版coreのDBリンク数検査） | 0.61.2、`32c3144490c016fe496a0aed769bce60987a2e9d`。`Win32_Storage_FileSystem`のみ明示指定 | `licenses/windows-sys-0.61.2/`のApache-2.0・MIT |
| [windows-link](https://github.com/microsoft/windows-rs)（windows-sysのリンク用依存） | 0.2.1、`d468916ac27a36fb8a12bafc1bf5c0ec2fe92238` | `licenses/windows-link-0.2.1/`のApache-2.0・MIT |
| [tempfile](https://github.com/Stebalien/tempfile)（開発時） | 3.27.0、`5c8fa12eb584931b4f1bccfde87eb72fbfa7dc61` | `licenses/tempfile-3.27.0/`のApache-2.0・MIT |

2026-11-12のSQLite依存更新では、[RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html)の修正版`libsqlite3-sys >=0.39.0`に対応する最小の安定版として、manifestの`rusqlite`を`=0.41.0`へ変更した。[公式依存情報](https://crates.io/api/v1/crates/rusqlite/0.41.0/dependencies)でWindows向けの要求は`libsqlite3-sys ^0.39.0`。remoteで生成するlockは同じ互換範囲の`0.39.1`（SQLite `3.54.1`）を予定する。`bundled`・`hooks`を維持し、既存の`prepare_cached`を使うために新版で独立した[`cache` feature](https://github.com/rusqlite/rusqlite/blob/a60e76b5e86ee60442213efe79758a6c2ca8cf3d/src/lib.rs#L123)と、`u64`のSQL引数を引き続き扱うための[`fallible_uint` feature](https://github.com/rusqlite/rusqlite/blob/a60e76b5e86ee60442213efe79758a6c2ca8cf3d/src/types/to_sql.rs#L275)を明示的に有効にする。SQLCipher機能は有効にしない。

**Cargo.lockは現在未更新**で、`rusqlite 0.32.1` / `libsqlite3-sys 0.30.1` / SQLite `3.46.0`のまま。lockの手編集は行わず、remoteの正規`cargo update`で解決してchecksumと差分を確認し、ビルド・テストを行う必要がある。下表はcrates.ioの公開metadataにあるcrateアーカイブのSHA-256であり、更新済みlockや取得したlicense単体のchecksumを示すものではない。

| 更新対象・公式metadata | `.cargo_vcs_info.json`のcommit | crate archive SHA-256 |
| --- | --- | --- |
| [rusqlite 0.41.0](https://crates.io/api/v1/crates/rusqlite/0.41.0) | [`a60e76b5e86ee60442213efe79758a6c2ca8cf3d`](https://docs.rs/crate/rusqlite/0.41.0/source/.cargo_vcs_info.json) | `9c026b4444579e8d754556c6c75d040d69c4a44283cdb648ab8a8d5965c2cfa8` |
| [libsqlite3-sys 0.39.1](https://crates.io/api/v1/crates/libsqlite3-sys/0.39.1) | [`fb7c711f4e988db149f43cac68f7865b25d0b10e`](https://docs.rs/crate/libsqlite3-sys/0.39.1/source/.cargo_vcs_info.json) | `14a5f01b87b92705d07b433c06f96cbc764637d81a0bffb5734ce17cf0f9a2b3` |

新版のMIT原文は、rusqliteの固定commitの[`LICENSE`](https://github.com/rusqlite/rusqlite/blob/a60e76b5e86ee60442213efe79758a6c2ca8cf3d/LICENSE)と、bindingの固定commitの[`LICENSE`](https://github.com/rusqlite/rusqlite/blob/fb7c711f4e988db149f43cac68f7865b25d0b10e/LICENSE)から無変更でコピーした。bindingの[`libsqlite3-sys/LICENSE`](https://github.com/rusqlite/rusqlite/blob/fb7c711f4e988db149f43cac68f7865b25d0b10e/libsqlite3-sys/LICENSE)は同じcommitの`../LICENSE`を指す。更新前のcommit `499cc7bb986e04cc66e6ed762522f7ea449178d1`に対応する[rusqlite 0.32.1 MIT](licenses/rusqlite-0.32.1/LICENSE)と[libsqlite3-sys 0.30.1 MIT](licenses/libsqlite3-sys-0.30.1/LICENSE)は、未更新lockの通知として保持する。

SQLite本体はpublic domain（[公式説明](https://sqlite.org/copyright.html)）。更新予定の3.54.1は上記binding commitの[`libsqlite3-sys/sqlite3/sqlite3.h`](https://github.com/rusqlite/rusqlite/blob/fb7c711f4e988db149f43cac68f7865b25d0b10e/libsqlite3-sys/sqlite3/sqlite3.h#L149)の版定義で確認し、その冒頭コメントを無変更で[新版SQLITE-BLESSING.txt](licenses/libsqlite3-sys-0.39.1/SQLITE-BLESSING.txt)に保持する。旧3.46.0の[SQLITE-BLESSING.txt](licenses/SQLITE-BLESSING.txt)も保持する。Jiffに同梱するIANA tzdbの由来は[同梱tzdb README](licenses/jiff-tzdb-0.1.8/README.md)も参照する。

初期の`apps/desktop/src/main.rs`はローカルnative-spikeを基礎にした。起動とInputの使用法は上記Kit commitの`examples/hello_world/src/main.rs`と`examples/input/src/main.rs`を参照した。現在の`apps/desktop/src/`ではTask・予定の編集、DBワーカー、表示設定とnative smokeの判定を実装し、初期スパイクを置き換えている。素材の色値とライセンス原文は保持した。ZedのGPL UI crateはコピーしていない。

React Aria、Radix Themes、Material、shadcn/ui、FullCalendarは調査先のまま。アプリ自身の配布ライセンスは未選定。全推移依存を含む配布物の通知収集・配布審査は今後の工程とする。Cargo metadataにlicense情報を保存するが、依存検査の合格だけで配布条件の確認完了とはしない。
