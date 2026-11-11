# 性能計測の方法

この計測器はremote Windows release版coreの処理時間と、remoteの指定PIDのCPU・メモリを記録する。開発PCではbuild、fixture生成、計測を行わない。標準の実行先はGitHub-hosted `windows-2025`であり、そのCPU・メモリ・処理時間を利用者のPC性能と混同しない。比較対象の測定、律速解析、実画面の提示時刻は含まない。「速くなった」「120Hzを達成した」という結論を出す機能ではない。実行結果は各runの証拠で確認する。以下の本測定は同じSHAのfull検証成功後、他のbuildの終了を待って直列で実行する。

## 実行

`.github/workflows/windows.yml`は`iiinno/UltraTasks`の`main`または`prototype/pc-first`へのpush/dispatchに限り、`verify.ps1 -Mode full -GuiMode Skip`成功後にspread/denseを直列測定する。PRではfull検証とログ回収だけを行い、本測定・配布package・release artifactを作らない。rusqliteのlock更新専用runも本測定をSkipし、候補lockをcommitした次のSHAでfull検証に成功してから測定する。1千/1万件、warmup 100、samples 1000、trials 5を明示する。full検証で作ったmeasure.exeと`verified-build.json`を同じjobで使い、計測ごとのコンパイルとキャッシュ初期化を省く。native smokeは`NOT_RUN`であり、core計測成功で補完しない。

remote Windows hostで手動実行する場合も[開発手順](development.md)のremote専用設定を先に行う。以下の例はそのhost上だけで使う。PowerShell 5.1以降、Rust 1.99.1/MSVC、Python 3.12を使用する。`performance.ps1`はUTF-8 BOM付きで保存する。

```powershell
# 選定済みremote Windows hostでfull検証が成功してから実行。
$workspace = 'C:/build/UltraTasks'
$output = 'C:/build-output'
$target = "$output/target"
$release = "$target/x86_64-pc-windows-msvc/release"
$binary = "$release/ultratasks-desktop.exe"
foreach ($distribution in @('spread', 'dense')) {
    & "$workspace/scripts/performance.ps1" -WorkspaceRoot $workspace -TargetDir $target `
        -EvidenceDir "$output/evidence/full-$distribution" -Mode Core -Sizes '1000,10000' `
        -Distribution $distribution -Seed 42 -Date 2026-11-11 -Zone Asia/Tokyo `
        -Warmup 100 -Samples 1000 -Trials 5 -TimeoutMs 5000 -RunTimeoutSeconds 1800 `
        -Binary $binary -MeasureBinary "$release/examples/measure.exe" -BuildManifest "$release/verified-build.json"
    if ($LASTEXITCODE -ne 0) { throw "Core measurement failed: $distribution (exit $LASTEXITCODE)" }
}
```

既定は`-Mode Core -Seed 42 -Date 2026-11-11 -Zone Asia/Tokyo -Distribution spread -Warmup 100 -Samples 1000 -Trials 5 -TimeoutMs 5000`。`-Distribution dense`は同日密集fixtureの別実行。双方の結果を同じ母集団へ混ぜない。`-Binary`は製品exeの識別・hash記録用であり、起動や入力は行わない。coreだけの実行では省略でき、その場合製品exeは未記録となる。

`-WorkspaceRoot`、`-TargetDir`、`-EvidenceDir`を上書きできる。相対パスは呼出元基準。実行ごとに証拠ディレクトリを新規作成する。`-MeasureBinary`は既存計測exeの絶対パスで、`-BuildManifest`との同時指定が必須。full検証成功、checkout SHA、lock SHA-256、release/MSVC/toolchain、exeのhash/サイズを照合する。CIではrun ID/attemptも一致させる。不一致時は再buildへfallbackせず失敗する。指定しない場合だけremote環境を初期化してrelease/lockedの計測器buildを行うが、CIと本測定では再利用経路を使う。最初の失敗exit code、実際のargv、stdout/stderr、toolchainを保存する。

remoteで短い動作確認が必要な場合は、同じコマンドの`-Sizes 1000 -Warmup 1 -Samples 3 -Trials 1`だけを変える。これは性能評価の既定規模を満たさない。生sampleとsummaryに実際の回数が残る。短縮試行の成功と本測定での性能評価は別に記録する。

直接CLIもremote host上だけで使う。ビルド済み`measure.exe`へ`--sizes 1000,10000 --seed 42 --date 2026-11-11 --zone Asia/Tokyo --warmup 100 --samples 1000 --trials 5 --distribution spread --timeout-ms 5000 --environment <環境JSON> --out <絶対パス>`を渡す。直接CLIではPowerShellのguardやmanifest照合、host環境採取、process timeoutを利用できないため、本測定は上のPowerShell入口を使う。

集計は信頼したcheckoutの`python -I scripts/summarize-performance.py --input <対象run>`で行う。ダウンロードしたartifactをcwdやスクリプト配置先にせず、入力パスとして渡す。`--environment`が無い直接CLIではGPU、RAM、ストレージ、電源、Hz、DPIなどは未観測であり、PowerShell経由と同じ環境証拠が揃ったとは扱わない。過去runも含むディレクトリを再集計すると過去結果が入るため、比較時は対象実行を明示する。

## fixtureと正当性

生成は`fixtures::seed_empty`を共用する。1千/1万Task、同数のTimeBlock、Task数の1/10のEvent。既定はseed 42、2026-11-11、Asia/Tokyo、30日分散。denseは同日密集。fixtureの内容はこのseed/date/zoneで決まり、DB/主体/既定所属先ID、履歴の実時刻、OS cacheまで同一にするものではない。

各サイズ・各試行にremote側の新規ディレクトリと合成DBを作る。既存DBの削除・上書き・WALファイルのコピーを行わない。DBはそのhostの一時領域に残すが、GitHub artifactには含めずrunnerの破棄とともに失われる。実データをCIへ持ち込まない。生sample、fixture準備記録、manifest、統計、失敗ログはDBなしで保持する。サイズ順は試行ごとに1000→10000、10000→1000と反転する。これはサイズ間の順序偏りを減らすもので、異なる実装のA/B比較ではない。

fixture生成、Store open、全件の検査用snapshot作成、各操作の入力組み立て、正当性検査、後片付け、JSON/CSV書き込みは処理時間から除外する。検査用snapshotは公開APIの全頁を読み、件数・重複・親子投影を検査する。検索の期待結果は無条件Task一覧をRustでliteral部分一致により絞る。agendaは32日窓の全Event/TimeBlockをRustで半開区間判定して当日に絞り、開始・種別・IDで独立に整列する。検索と同じ条件のquery結果をそのまま期待結果に流用しない。ただしsnapshot自体はcore API由来であり、独立受入テストの代替ではない。

| sample名 | 処理時間に含める実操作 | 成功条件と負荷 |
| --- | --- | --- |
| `search_and_count` | `Store::tasks`の先頭200件と正確なCOUNT | 期待する値・順序・件数・次頁有無・世代を比較。`日本語`、`資料`、literal `_%`をsample index % 3で循環 |
| `count_only` | `Store::task_rows(query, &[])` | 同じ検索条件のCOUNT。rows/matching_idsは空、totalと世代は期待値。空ID入力でも実際にCOUNTが走る |
| `agenda_timed` / `agenda_all_day` | 選択日の各先頭200件とCOUNT | 期待する値・順序・件数・次頁有無・世代を比較。終日の一致件数が0のfixtureはその0を記録し、非空だったとは呼ばない |
| `create` | 新しいTaskの保存1回 | 1千/1万件を背景に1件追加。保存ID、値、変更通知、履歴、保存済み値、件数N+1を検査。その後の削除とN件への復帰は計時外 |
| `edit` | 同一Taskのタイトル更新1回 | 毎回異なるタイトル。版増加、実変更、保存済み値、既存TimeBlock不変を検査 |
| `complete` | 未完了Taskの完了1回 | Open→Done、版増加、実変更、保存済み値、既存枠不変を検査 |
| `undo` / `redo` | 直前の完了の取消/再適用各1回 | 最新の型付きtargetを使用。値の逆転/復帰、版増加、保存済み値、既存枠不変を検査 |
| `second_block` | 既存一枠のTaskへ重なる二枠目を追加1回 | 二枠の値と親、Taskの版増加、Task件数Nを検査。二枠をまとめた一TXの時間とは呼ばない |

読み取り群のwarmup→本測定を先に実行し、その後書き込み群のwarmup→本測定を行う。各群では表の順で1操作ずつ循環し、操作ごとに既定100 warmup＋1000 sampleを得る。検索3条件は同じ操作の混合負荷で、条件別に解釈する場合はindex % 3とtotalで分ける。

書き込み群は同じ未完了Taskを使い、完了・二枠・タイトルを各回で復元する。作成対象は毎回異なるIDで、他の計測時は生存Task数が常にN。`live_tasks_before`に開始時の件数を記録する。墓標・履歴・WALは試行内で増減し、毎sample新品DBという意味ではない。後片付けと検査もcacheやWAL checkpointに影響する。この反復はホットな単一Taskを測り、ランダムな全Task編集、実利用のthink timeや高並列負荷を代表しない。

不正な応答、no-op、coreのエラーをfailureとして生sampleへ保存し、その試行を中止する。後続試行は別DBで実行する。計時外のfixture/後片付け失敗はtrial JSONに残し、未実行sampleを成功で補わない。最後にDBを再openして件数を検査する。

## 時間、成功、統計の定義

`std::time::Instant`でStoreメソッド呼び出し直前から同期返却直後までを`processing_ns`に記録する。SQLだけでなくcore内の検証・履歴・TX・応答生成を含む。後続の保存値読み直しなどは`verification_ns`へ別記する。UI入力、ワーカー投入、待ち、応答配送、render callback、画面提示はここには含まれない。`queue_wait_ns`と`presentation_ns`はnullで、0ではない。

`TimeoutMs`は呼び出しが返った後に判断する壁時計の期限。処理をキャンセルするhard timeoutではなく、ハングを停止する保証もない。成功かつ検査済みでも期限超過ならstatus=`timeout`、API/検査失敗ならstatus=`failure`、それ以外は`success`。`deadline_exceeded`、`api_succeeded`、`verified`を別々に保存する。失敗と期限超過が重なった場合statusはfailureで、deadline_exceeded件数にも残る。`QueryBudgetExceeded`はVM予算の失敗であり、時間制限と同一視しない。coreを中断した場合もflush済みsampleを残し、completion欠落を未完了として集計する。

別に`-RunTimeoutSeconds`（既定1800秒）がmeasure process全体のhard timeoutを設定する。期限超過はprocess treeを停止してexit 124とし、保存済みsampleの集計を試みる。これは操作単位の`TimeoutMs`と異なる。CIの各計測stepは35分、jobは180分。step/jobのtimeoutや取消で集計・upload自体が完了しない場合も、未完了を成功とは扱わない。

出力は次の構成。

- `run.json`: seed/date/zone、サイズ、操作、warmup/sample/試行数、CLI、環境、未観測項目。
- `core-samples.jsonl`と`core-samples.csv`: warmupを含む全attemptの生時間、検査結果、エラー、行数/件数。ログ書き込みは各sample後にflushする。
- `fixture-setup.jsonl`: 計時外のfixture生成・snapshot準備時間と件数。
- `trial-*.json`と`completion.json`: 試行の完了/中止と原因。失敗を含む終了は非0。
- `summary-*/summary.json`と`summary.csv`: Python再集計。既存出力を上書きせず新規ディレクトリを使う。環境はJSONに保持し、CSVのrun列から対応付ける。

集計はrun・サイズ・分散・phase・操作を分け、試行別と同条件のpooledを出す。成功数、失敗数、timeout数、検査成功数、予定/実sample数、未実行数、予定/観測試行数を保持する。中央値は偶数時に中央2値の平均、p95/p99はnearest-rank `sorted[ceil(n*p)-1]`。最小/最大と試行ごとの中央値の範囲も出す。`all_attempts`は失敗・期限超過・外れ値を含み、`success_only`を併記する。nullの未観測値は0で補完せず件数を明示する。20回のp99は最大値で尾部推定が粗く、短縮動作確認のp99を設計予算の合格判定へ使わない。

集計exit 0は予定した測定がエラーなしで完了した意味。p95目標20ms、キュー待ち20ms、起動500ms、UI仕事8.3msの達成判定ではない。比較を主張するには同じ最終バイナリ条件、同じfixture、5回以上の交互試行、試行変動、別の非報告profiling試行による律速根拠、実GUIの待ち時間が必要。

## 指定PIDの外部観測

対話可能なremote Windows desktop上で主担当が専用fixtureの製品exeを起動し、入力・scroll・motionの操作をComputer Useで行う。GitHub-hosted CIではこのGUI手順を実行せず、NOT_RUNとする。開発PCの禁止処理をこの手順で代替実行しない。計測器は起動、終了、入力、focus変更、scroll、ウィンドウ移動、OS設定変更を行わない。使用するPIDと開始時刻を固定して観測し、PIDが終了/再利用されたら失敗として停止する。

```powershell
powershell.exe -NoProfile -File scripts/performance.ps1 -Mode Observe -ProcessId 12345 -Sizes 10000 -Binary C:/absolute/ultratasks-desktop.exe -Observation idle -Seconds 60 -Trials 5 -IntervalMs 1000 -Motion none -EvidenceDir C:/absolute/idle-evidence
```

`-Sizes`は観測中の単一サイズを指定する。seed/date/zone/distribution/サイズ/motionは主担当の申告で、PIDからDBを開いて内容を照合しない。`-Binary`指定時はPIDの実行ファイルpathと照合し、hashでbuild証拠に結び付ける。外部PIDのrelease/debugを自動判定したとは報告しない。

`-Observation input|scroll|motion|manual`も同じ外部観測を行いラベルを変える。scrollは主担当が固定経路・30秒・5回、900/1280幅、motion有効/無効を交互に用意する。idleは主担当が入力・animation終了を確認してから60秒×5回で開始する。計測器はidleを自動検出せず、試行の間にも入力しない。

実測intervalとprocess累積CPU時間の差から、単一core換算`100 * ΔCPU時間 / Δ経過時間`、全論理CPU基準としてさらに`Environment.ProcessorCount`で割った率を記録する。マルチスレッドでは単一core換算が100%を超え得る。private bytesとworking setをbytesで採る。計測器自身の読取所要時間も別記する。指定のsleep時間を実intervalと仮定せず、遅延は実時間に残す。

`process-samples.jsonl/csv`を同じ集計器で統計化する。successは同一PID・開始時刻・有効間隔・単調CPU時間を確認した意味で、入力/保存成功ではない。操作の成否、文字欠落、IME、描画行数、notify数、frame要求数、分更新回数は主担当の操作記録/desktop診断が必要。外部CPU/メモリだけで「animation終了後のframe要求0」を証明しない。

## Desktop診断の集計

終了済みプロセスの診断ファイル、またはそのrunだけを含むディレクトリを指定する。標準Pythonのみで実行できる。

```powershell
python -I scripts/summarize-desktop.py --input C:/absolute/dev-run/diagnostics
```

新規`desktop-summary-<ID>`へ`summary.json`と元JSONLのバイト列の複製を保存し、元パス・SHA-256・request_id・欠落行も追跡できる。`--out`には未作成ディレクトリを指定する。ログファイルごとに分け、異なるサイズ・操作・起動を合算しない。実在し観測されたkindだけを集計し、未観測kindと未知kindを別記する。

| kind | 集計する値と境界 |
| --- | --- |
| `first_frame_callback` / `first_data_applied` | `elapsed_us`による診断開始から初回callback／初回データ状態反映まで。プロセス生成や画面提示を起点・終点にしない |
| `core_response` | DBワーカー処理時間。Open・fixture・設定保存を含み、操作別には分離できない。countのOk/Err件数を保持 |
| `queue_wait` / `response_receive` | ワーカー開始までの待ち／完了からUI受信までの経過時間 |
| `display_apply` / `input_handler_cpu` | 計装された受信反映／入力handler区間の経過時間。入力遅延全体ではない |
| `render_callback_cpu` | Renderの要素構築区間の経過時間とTaskキャッシュ行数。layout/paint/GPU/画面提示は含まない |
| `task_cache` / `task_row_built` | キャッシュ行数／行構築イベントの延べ件数。実表示行数や1フレームごとの表示行数は未観測 |
| `commit_applied` / `minute_notify` | changed件数／分更新notify件数。全notifyや全frame要求数ではない |
| `dropped_samples` | 終端のdrop件数。欠落・重複・不正終端ならnull、0としない |

中央値、nearest-rank p95/p99、最小・最大、件数、未観測件数を保存する。`cpu`というkind名でも計時は`Instant`による経過時間であり、OSのCPU使用時間ではない。元sampleのエラーや外れ値は除外しない。null・不正値・壊れたJSON行はissuesへ残し、集計可能な値は残す。終端欠落、drop>0、構文・数値不整合は集計出力を保存してexit 1。exit 0でも性能達成や操作成功を意味しない。

## Native GUIの後続測定手順

利用者が選定した対話可能なremote hostで、本測定依頼後に行う。GitHub-hosted CIと開発PCでは実行しない。各サイズ1000/10000とspread/denseを別の専用DBに準備し、生成時間を起動測定から外す。以下は1条件の例で、`$size`と`$distribution`を変えて4条件を直列に実施する。上の本測定コマンドと同じ`$workspace`、`$output`、`$target`、`$binary`を使う。

```powershell
$size = 1000
$distribution = 'spread'
$gui = "$output/evidence/parallel/gui-$size-$distribution-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $gui | Out-Null
$db = "$gui/fixture.sqlite"
& "$workspace/scripts/prepare-fixture.ps1" -WorkspaceRoot $workspace -TargetDir $target `
    -EvidenceDir "$gui/setup" -DbPath $db -Profile "$size" -Distribution $distribution `
    -Seed 42 -Date 2026-11-11 -Zone Asia/Tokyo
if ($LASTEXITCODE -ne 0) { throw 'Fixture preparation failed' }
$desktop = Start-Process -FilePath $binary -PassThru -ArgumentList @(
    '--db', "`"$db`"", '--demo', 'none', '--date', '2026-11-11', '--zone', 'Asia/Tokyo',
    '--diagnostics-dir', "`"$gui/diagnostics`"")
$observe = @{
    WorkspaceRoot = $workspace; TargetDir = $target; EvidenceDir = "$gui/process"
    Mode = 'Observe'; ProcessId = $desktop.Id; Sizes = "$size"; Binary = $binary
    Distribution = $distribution; Seed = 42; Date = '2026-11-11'; Zone = 'Asia/Tokyo'
    Trials = 5; IntervalMs = 1000; Motion = 'unrecorded'
}
# 操作者が入力・animation終了を確認してから開始する。
& "$workspace/scripts/performance.ps1" @observe -Observation idle -Seconds 60
if ($LASTEXITCODE -ne 0) { throw 'Idle observation failed' }
# 手動の固定scroll経路を30秒×5回。開始/終了時刻、操作成否を別途記録する。
& "$workspace/scripts/performance.ps1" @observe -Observation scroll -Seconds 30
if ($LASTEXITCODE -ne 0) { throw 'Scroll observation failed' }
# GUIを通常終了し、診断の終端をflushした後に実行する。
python -I "$workspace/scripts/summarize-desktop.py" --input "$gui/diagnostics"
```

初回callback／データ反映は同じ未変更fixtureの起動・通常終了を5回行い、毎回別diagnosticsディレクトリへ記録する。起動cacheは制御していないのでcold起動と呼ばない。入力検証は新しい専用fixtureで、日本語検索・追加・編集・完了・Undo/Redoを固定順に行う。CPU/メモリを併記する場合は同じObserveコマンドを`-Observation input -Seconds 30`にして5試行を採り、IME確定・文字欠落・保存成否・操作の開始/終了時刻を記録する。

幅900/1280、motion有効/無効を交互に実施し、実際のclient寸法、DPI、表示Hz、motion設定を記録する。確認済みの場合だけ`$observe.Motion`や`-MonitorHz`へ値を入れる。`--window-size 900x720/1280x800`のCLI対応は別担当の統合後に実在確認する後続事項であり、上のコマンドはこの引数を使わない。現行診断だけでは実表示行数、全frame要求数、input-to-photon、実画面提示は測れないため、未観測を保持する。

## 環境と残る観測

PowerShellは開始前/終了後にOS、CPU名・core数・論理CPU数・CIM LoadPercentage、GPU・driver、RAM、disk/volume、active power schemeを読み取る。負荷の参考として上位processの累積CPU秒を記録するが、累積値は現在CPU使用率ではない。設定を変更したり他processを止めたりしない。情報取得が拒否された項目はNOT_OBSERVEDと原因を残す。

checkout SHA、実行場所（github-hostedまたは明示remote host）、GitHub run ID/attempt、runner image versionを環境記録へ含める。CI値はそのrunner環境の観測であり、利用者PCの値として報告しない。計測時には実行ファイル、Cargo.lock、計測器、fixture sourceのSHA-256を記録し、既存exeの再利用時にはfull検証manifestも残す。

再集計でhashを再計算するのは、明示した入力root内の`run.json`と生sampleだけ。これらを`captured_at_summary`・`local_input_artifact`として記録する。実行時hashは`reported_runtime_hashes`へ、`run.json#/...`の出典を伴う未検証の申告値として保存する。実行時hashが無い場合は欠落のままとし、metadataの`executable`・`lock_file`・`harness_source`やenvironment内のpathをopen/stat/hashして補完しない。UNC・絶対パス・相対パスのいずれもmetadata文字列として保持し、参照先へアクセスしない。入力root・親ディレクトリ・入力ファイルのsymlink/reparse pointは拒否する。

CSVでは数式として解釈され得るprefixを持つ文字列をapostropheで保護する。空白・制御文字などを伴うprefixも検査する。JSONの元文字列と数値は維持し、統計の母集団・失敗・timeout・未実行・nullの扱いは変更しない。`scripts/test_summarize_performance.py`の軽い8ケースをCIのRust準備前に実行し、metadata pathへのI/O禁止、リンク拒否、CSV保護と既存統計を回帰確認する。

GPUのCurrentRefreshRateはadapterの参考値。対象monitorのHzは主担当が観測して`-MonitorHz`へ指定するか未観測にする。PIDからGetClientRectとGetDpiForWindowを読み、client寸法の単位は呼出し側DPI contextに依存すると記録する。無条件にlogical pixel/physical pixelへ変換しない。

検索中の更新投入、実ワーカーのqueue wait、初回frame・初回データ表示、UI handler/layout/paint、実際の画面提示はこのcore測定の対象外。desktopのrequest_idに紐付く投入/開始/完了/受信/状態反映を別記録で照合する。GPUI render callbackをinput-to-photonやGPU時間と呼ばない。提示時刻が必要なら主担当によるPresentMon/ETW等の別測定が必要で、それまでは未観測。起動cacheを制御していない試行をcold起動とも呼ばない。
