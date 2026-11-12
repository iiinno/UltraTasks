# Windows版の開発と検証

`crates/ultratasks-core`と`apps/desktop`の2クレートを使用する。coreは公開型と`Store`を公開し、SQLite・履歴処理は内部に閉じる。desktopはStoreを所有するDBスレッドとGPUIの入力・表示を接続する。検証済みの範囲は[プロトタイプの状態](prototype-status.md)、動く版の合格条件と並行PRのbase/head運用は[Windows基準版](windows-baseline.md)を参照する。CI構成が存在することと、そのSHAの検証が成功したことは区別する。

## 実行場所

開発PCではRust build、cargo check/test/clippy/bench、rustfmt、fixture生成、大規模計測を行わない。ジョブ数の削減やバックグラウンド実行で代替しない。ローカル作業は編集、軽量な構文検査、GitHubの読取、利用者が許可したビルド済み成果物の起動に限定する。

標準の検証先はGitHub-hostedの`windows-2025`。必要なRust、MSVC、SDK、Pythonはrunner上で準備する。開発PCにbuild toolsを入れる必要はない。`scripts/windows-env.ps1`はGitHub-hosted Windows環境、または明示選定したremote Windows host以外からのbuild・fixture・計測入口を拒否する。guardはtarget/evidenceの作成、コンパイラ検出、キャッシュ初期化より前に実行する。

## Windows CI

`.github/workflows/windows.yml`は`main`と`prototype/pc-first`へのpush、pull request、入力なしの手動実行を入口とする。`prototype/pc-first`の初回pushだけで処理が始まり、PR作成やmainへの導入は不要。rusqliteのmanifest版とlock版が一致すれば通常のfullへ進み、不一致なら下記のlock更新専用runに分岐する。他のfeature branchのpushは対象外。手動dispatchはdefault branchへのworkflow登録後に使える。lock一致時は単一jobで`verify.ps1 -Mode full -GuiMode Skip`を1回実行する。`iiinno/UltraTasks`の`main`または`prototype/pc-first`を対象とするpush/dispatchだけが、検証成功後に配布package・core本測定・release artifactを作成する。PRはfull検証とログだけを残し、配布や本測定は行わない。fast/fullの重複matrixはない。concurrencyはsource SHA単位で、PRとbranch実行を別グループにする。PRが配布runを取り消すことはなく、同じSHAのpush/dispatchが重なった場合は先行runを取り消す。すでに完了したrunを再実行しない仕組みではない。

- runnerはGitHub-hosted `windows-2025`のみ。self-hosted、有料の大型runnerは設定しない。
- workflow権限は`contents: read`のみ。checkoutの認証情報は保存しない。
- Rust `1.99.1-x86_64-pc-windows-msvc`、release、`--locked`、Python `3.12`を使う。Windows imageのMSVC/SDKをvswhereで検出し、cl/link/rc/fxcを検査する。
- fullはfmt check、workspace全targetのClippy（`-D warnings`）、全targetのテスト、製品exeとmeasure exampleのbuild、Windows向けmetadata/feature tree、依存検査を行う。test-supportは製品/計測exeのbuildで有効にしない。
- native smokeは`NOT_RUN`。hosted runnerのデスクトップ操作は検証済みと扱わず、IME、Narrator、focus、DPI、ドラッグ、実画面提示も未検証のまま残す。
- 全体180分、toolchain準備10分、full検証90分、各分布の計測step35分、各artifact upload10分が上限。計測process自体は30分で停止し、部分sampleを集計する。取消やjob全体のtimeoutでは最後のログ転送まで完了できない場合がある。

検証成功後に`verified-build.json`へcheckout SHA、Cargo.lockのSHA-256、toolchain/triple/profile、run ID/attempt、exeのサイズ・SHA-256、native smokeの状態を保存する。追跡ファイルに変更があるcheckoutの成果物は認定しない。`performance.ps1 -MeasureBinary ... -BuildManifest ...`がこれらを照合し、コンパイラ初期化と再buildを省く。1千/1万件、spread/dense、100 warmup、1000 samples、5 trialsの両分布で同じmeasure exeを使用する。

GitHub Actionsで得た時間・CPU・メモリはrunnerの値であり、開発PCの性能ではない。基準機との比較やUI応答の合格判定には別の実測が必要。[計測方法と限界](performance-method.md)を参照する。

## rusqliteのlockをremoteで更新する

`prepare-rusqlite-lock.py`はGitHub-hosted Windows runner専用で、Python 3.12のTOML readerによりroot manifestの`workspace.dependencies.rusqlite`の厳密な`=MAJOR.MINOR.PATCH`と、既存lock内の単一のcrates.io rusqliteを照合する。更新版はmanifestから読み、workflowに別の版を固定しない。features/default-featuresもmanifestの値をCargoへそのまま渡し、来歴JSONへ記録する。

- 版が一致し、lockの`libsqlite3-sys`が0.39.0以上である通常runは従来の`--locked` full検証へ進む。ほかの依存やfeatureだけのlock不整合は通常のlocked検証で失敗させ、全依存の自動updateには切り替えない。
- 版が不一致なら、公式repoの許可branchへのpush/dispatchだけで`cargo +1.99.1-x86_64-pc-windows-msvc update --manifest-path <root Cargo.toml> --package rusqlite --precise <manifestの採用版>`を実行する。必要な推移依存の変更はあり得るが、無指定の全依存updateやlock全体の再生成は行わない。
- 解決された`libsqlite3-sys`の実際の版・crates.io source・lock内checksumを記録する。単一のcrates.io packageと64桁のchecksumを要求し、0.39.0未満なら候補を拒否してlock artifactを作らない。通常runも下限未達のlockではfullへ進めない。
- 準備済みnotice/licenseは`libsqlite3-sys 0.39.1`向け。別の版なら`notice_review_required=true`とし、親担当が実際の版・checksumと通知原文の整合を再確認してから候補を受け入れる。下限判定の成功をnoticeの検証成功とは扱わない。
- PRや未許可branchの不一致は失敗として停止し、更新候補を作らない。欠落・複数版・非crates.ioなど、この小さい更新経路で判断できないlockも停止する。
- lock更新runではtarget cache復元、full検証、本測定、release artifactをSkipする。結果は`GENERATED_UNVERIFIED`、`full_verification=NOT_RUN`、`release_eligible=false`。run自体の成功は候補生成の成功であり、製品の検証成功ではない。

Cargo更新processは10分、stepは12分が上限。失敗・timeoutは来歴JSONとstdout/stderrに残し、成功した候補だけを`windows-lock-update-<元SHA>-<attempt>`へアップロードする。内容は`Cargo.lock`と`lock-update.json`のみ。JSONには元checkout SHA、run ID/attempt、全workspace manifestのhash、元/生成lockのhash、旧/採用版、features、更新前後の`libsqlite3-sys`の版・checksum・下限判定・notice再確認要否、正確なargv、開始/終了時刻、終了コード、追加/削除されたpackageを記録する。下限未達で拒否した場合も、実際の解決版とchecksumを失敗JSONへ残す。

取得前に下記と同じGitHub APIのrepository/head repository/event/branch/SHA/path/statusを照合し、対象run IDとartifact名を明示して新しい空ディレクトリへdownloadする。生成lockのSHA-256とJSONを照合し、元SHAおよび現在のmanifest hashが一致することを確認してから、親担当が差分をレビューしCargo.lockだけを適用・commitする。開発PCではCargoを実行しない。次のSHAのrunがmanifest/lock一致で通常のlocked検証へ進み、その検証に成功して初めて配布可能になる。既存`get-windows-artifact.ps1`はexe配布専用で、このlock候補を選択したり適用したりしない。

## キャッシュと成果物

公式`actions/cache`を使い、Cargoのregistry index/cache/srcとgit依存だけをソースキャッシュにする。`CARGO_HOME`全体、認証情報、環境設定、ユーザーデータは保存しない。targetはreleaseのhost側/Windows target側を保存し、image version、Rust、triple、profile、lockとbuild設定をキーに含める。SHA/run/attemptごとの保存キーと同条件の復元prefixを使う。`.ultratasks-workspace`と過去の検証manifestは保存せず、復元後はworkspace packageだけをcleanし外部依存を再利用する。targetのDB、SQLite sidecar、credentials、`.env`は除外する。

restore/saveを分離し、検証の直後、配布・長い計測の前にキャッシュ保存を試みる。検証失敗時も取消されていなければ条件に従って保存する。配布packageは日本語を含むスクリプトを正しく解釈するためPowerShell 7（`shell: pwsh`）で作成し、release artifactはpackage直後にアップロードする。これにより本測定の完了前に検証済みexeが揃う。ただし通常の取得スクリプトはrun全体の`completed/success`を引き続き要求する。

runごとに以下のartifactを14日間保持する。

| artifact | 内容 |
| --- | --- |
| `windows-release-<checkout SHA>-<attempt>` | desktop/measure exe、検証/配布manifest、`Start-UltraTasks.cmd`と既存exe専用`start-demo.ps1`、短い`USAGE.txt`、`THIRD_PARTY_NOTICES.md`と`licenses/`・素材の原文通知・upstream記録。上記のtrusted push/dispatchでfull検証成功後のみ作成 |
| `windows-evidence-<checkout SHA>-<attempt>` | environment、argv、stdout/stderr、summary、失敗記録。通常のtrusted push/dispatchでは計測manifestと生JSONL/CSVも含む。lock更新専用runではlockの照合/更新記録を含む。PRを含め失敗時も回収を試みる |
| `windows-lock-update-<元SHA>-<attempt>` | trustedなlock更新専用runが生成した未検証のCargo.lockとlock-update.json。適用・commit後の別runでlocked検証が必要 |

成果物は上記の許可リストで選ぶ。DBそのもの、WAL/SHM、実カレンダーデータ、資格情報、Cargo設定やホーム全体はアップロードしない。計測の合成DBはrunnerの一時領域にだけ残り、runner破棄で失われる。検査・集計ではダウンロードしたファイルを含むディレクトリをcwdやスクリプト配置先にしない。信頼したcheckoutのツールへ入力パスとして渡す。同梱launcherは対象run・SHA・配布hashを確認した後に利用者が明示的に起動するためのもので、取得や検査中に実行しない。

## 成果物を取得する

`github.com/iiinno/UltraTasks`にアクセスできる公式GitHub CLIが必要。privateの場合、読取・downloadには対象repositoryへのアクセスとActions read権限が必要になる。fine-grained PATなら対象repositoryのActions read（repo情報の読取にはContents readも必要）を用いる。公開後も取得スクリプトは認証済みaccountを確認する。トークンはchat、スクリプト、artifactへ書かない。

run IDを明示してGitHub APIの`head_sha`を確認する。ExpectedShaの根拠にartifact名やartifact内部のmanifestを使わない。「最新」runを自動選択せず、選んだrunのAPI情報が次のすべてを満たすことを要求する。

- `repository.full_name`と`head_repository.full_name`が両方とも`iiinno/UltraTasks`。
- `event`が`push`または`workflow_dispatch`、`head_branch`が`main`または`prototype/pc-first`。
- APIの`head_sha`が指定した40文字の`ExpectedSha`と一致する。
- `path`が`.github/workflows/windows.yml`、`status`が`completed`、`conclusion`が`success`。

公式ghの導入・認証・active accountを確認したshellで、対象を固定してAPIを読む。

```powershell
$env:GH_HOST = 'github.com'
$env:GH_REPO = 'github.com/iiinno/UltraTasks'
$env:GH_PROMPT_DISABLED = '1'
$runId = '<確認するrun ID>'
gh api --hostname github.com "repos/iiinno/UltraTasks/actions/runs/$runId" --jq '{repository: .repository.full_name, head_repository: .head_repository.full_name, event, head_branch, head_sha, path, status, conclusion}'
# API情報を確認してから、そのhead_shaを明示する。
./scripts/get-windows-artifact.ps1 -RunId $runId -ExpectedSha <APIで確認した40文字のhead_sha>
```

取得スクリプト自身もghの有無・version・auth status・accountを確認し、`GH_HOST=github.com`、`GH_REPO=github.com/iiinno/UltraTasks`を固定して同じrun APIを再照合する。fork由来・別repository・別branch・PRは、artifactが信頼済みSHAを名乗っていても拒否する。同一repositoryのPRも配布経路では受け付けない。判定に成功してから、APIのSHAを使って毎回新しい空ディレクトリ`%LOCALAPPDATA%\UltraTasks\artifacts\run-...`へ取得する。manifestのSHA/run/attempt、exeのhash/サイズ、同梱launcherと通知を含む配布ファイルのhashが一致してから`current.json`を更新する。失敗時は既存の選択を保持する。ダウンロードしたexeやスクリプトは自動実行しない。

`artifact-policy.ps1`はAPIオブジェクトだけを判定する純粋な関数で、`test-artifact-policy.ps1`は合成JSONを使い許可・拒否・欠落・SHA偽称を検査する。CIでもbuild前に実行する。この検査はnetwork、download、build、fixture、アプリを実行しないため、開発PCでも軽量な回帰確認ができる。

認証が無ければ`gh auth login --hostname github.com --git-protocol https --web`を利用者が行う。権限変更、workflowの転送・dispatchはdownloadとは別操作。workflowファイルをGit経由で追加/更新するOAuth tokenには通常`workflow` scopeも必要で、`repo`だけでは拒否されることがある。dispatchにはActions write権限が必要。この資料の取得スクリプトは認証変更、push、dispatchを行わない。

## ビルド済みアプリを開く

取得後、リポジトリ直下の`Start-UltraTasks.cmd`をダブルクリックする。`scripts/start-demo.ps1`は選択済み成果物を使い、manifestとexeのhashを確認して起動する。取得したartifactの同名cmdを使う場合は、artifact直下の同梱exeが優先される。成果物フォルダー単体で動作し、Cargo.toml、Rust、元checkoutは不要。exeが無ければ取得案内を表示して停止する。dev.ps1やCargoは呼ばない。

```powershell
./scripts/start-demo.ps1 -WhatIf
./scripts/start-demo.ps1 -Binary C:/absolute/ultratasks-desktop.exe
./scripts/start-demo.ps1 -TargetDir C:/absolute/existing-target
./scripts/start-demo.ps1 -Date '' -Zone Asia/Tokyo
```

`-Binary`で明示したexe、または`-TargetDir`に既にある`x86_64-pc-windows-msvc/release/ultratasks-desktop.exe`も選べる。両方は指定できない。明示指定のexeにmanifestが無ければSHAは`NOT_VERIFIED`と表示する。暗黙に古いbuild cacheを探したり、欠けたexeをコンパイルしたりしない。`-WhatIf`は選択と保存先を確認するだけで、ファイル作成もGUI起動もしない。

DBは既定で`%LOCALAPPDATA%\UltraTasks\demo\demo.sqlite`。常に`--demo none`で開き、新規保存先なら空のアプリDBから始まる。fixtureやsmallデモをローカル生成しない。以前のDBは再seedせず利用し、DBが無いのにWALなどsidecarだけが残っている場合は停止する。`-DbPath`、`-EvidenceDir`で保存先を変えられる。ログは`demo/evidence`配下、表示設定はDB横の`.ui.json`に保存する。

日付の既定は指定timezoneの今日、timezoneは`Asia/Tokyo`。OS設定は変更しない。特定日を開く場合は`-Date YYYY-MM-DD`を指定する。既存DBのデータを選択日に作り直す機能ではない。

## 明示選定したremote Windows hostで実行する場合

以下は利用者が選定したVPSなどのremote Windows host上だけで実行する。開発PC上で許可フラグを設定してguardを回避してはならない。GitHub Actionsのself-hosted runnerをこのフラグで許可することもない。

remote hostにはPowerShell 5.1以降、Python 3.12、Rustup、Visual Studio C++ Build Tools、Windows SDKを用意する。固定toolchainの導入もremote側だけで行う。

```powershell
# 選定済みremote Windows host上の専用shellのみ。
$env:ULTRATASKS_REMOTE_HOST = '1'
rustup toolchain install 1.99.1-x86_64-pc-windows-msvc --profile minimal --component rustfmt --component clippy --target x86_64-pc-windows-msvc
$workspace = 'C:/build/UltraTasks'
$paths = @{ WorkspaceRoot = $workspace; TargetDir = 'C:/build-output/target'; EvidenceDir = 'C:/build-output/evidence/verify' }
& "$workspace/scripts/verify.ps1" @paths -Mode full -GuiMode Skip
if ($LASTEXITCODE -ne 0) { throw 'Full verification failed; do not use these binaries for measurement.' }
```

対話可能なremote Windows desktopでnative確認を別途実施する場合は`-GuiMode Native`を明示する。新規専用DB、30秒timeout、exit 0と`SMOKE_OK:`の両方を要求し、失敗buildの古いexeは使わない。これは初回frame callbackまでの確認であり、IMEや実画面提示の確認ではない。

`dev.ps1`、`prepare-fixture.ps1`、`acceptance.ps1`、`performance.ps1`もremote専用。remoteでfixtureが必要なら新しい保存先へ`prepare-fixture.ps1 -DbPath <絶対パス> -Profile 1000 -Distribution spread`などを使う。実データや稼働中DBをコピーしない。同一targetでのbuild/測定は並行しない。各runはenvironment.json、argv、stdout/stderr、MSVC初期化ログ、summary.jsonを残し、最初の非0終了コードを維持する。

## 固定依存

`rust-toolchain.toml`はRust 1.99.1/MSVC hostまで固定する。GPUI Kit 0.9.0／GPUI 0.3.11、rusqlite 0.41.0（default-features=false、bundled/cache/fallible_uint/hooks）、jiff 0.2.37と同梱jiff-tzdb 0.1.8を使う。Kit公式commitは`7591a35eced761e3613c01e71ff04c10129dae1a`。coreは`TimeZoneDatabase::bundled()`を使用し、SQLite DLLとOS tzdbには依存しない。`Cargo.lock`を保持し、上記のlock更新専用操作を除くCargoの依存解決・build・検証は`--locked`で行う。通知は[THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md)。
