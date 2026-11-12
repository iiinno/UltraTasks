[CmdletBinding()]
param(
    [string[]]$Sizes = @('1000', '10000'),
    [string]$Binary,
    [string]$MeasureBinary,
    [string]$BuildManifest,
    [ValidateRange(1, 21600)][int]$RunTimeoutSeconds = 1800,
    [ValidateSet('Core', 'Observe')][string]$Mode = 'Core',
    [ValidateSet('spread', 'dense')][string]$Distribution = 'spread',
    [UInt64]$Seed = 42,
    [string]$Date = '2026-11-11',
    [string]$Zone = 'Asia/Tokyo',
    [ValidateRange(0, 100000)][int]$Warmup = 100,
    [ValidateRange(1, 100000)][int]$Samples = 1000,
    [ValidateRange(1, 100)][int]$Trials = 5,
    [ValidateRange(1, 3600000)][int]$TimeoutMs = 5000,
    [ValidateRange(0, 2147483647)][int]$ProcessId = 0,
    [ValidateSet('idle', 'input', 'scroll', 'motion', 'manual')][string]$Observation = 'idle',
    [ValidateRange(1, 3600)][int]$Seconds = 60,
    [ValidateRange(100, 60000)][int]$IntervalMs = 1000,
    [ValidateSet('system', 'reduced', 'none', 'unrecorded')][string]$Motion = 'unrecorded',
    [ValidateRange(0, 1000)][double]$MonitorHz = 0,
    [string]$WorkspaceRoot,
    [string]$TargetDir,
    [string]$EvidenceDir
)

. (Join-Path $PSScriptRoot 'windows-env.ps1') -WorkspaceRoot $WorkspaceRoot -TargetDir $TargetDir -EvidenceDir $EvidenceDir

function Read-CimObservation {
    param([string]$Class, [string[]]$Properties)
    try {
        return [ordered]@{ status = 'OBSERVED'; data = @(Get-CimInstance -ClassName $Class -ErrorAction Stop | Select-Object -Property $Properties); error = $null }
    } catch {
        return [ordered]@{ status = 'NOT_OBSERVED'; data = @(); error = $_.Exception.Message }
    }
}

function Read-FileIdentity {
    param([string]$Path)
    if (-not $Path) { return $null }
    $item = Get-Item -LiteralPath $Path -ErrorAction Stop
    if ($item.PSIsContainer) { throw "Expected a file: $Path" }
    return [ordered]@{ path = $item.FullName; bytes = $item.Length; sha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash }
}

function Read-HostEnvironment {
    param($Context, [string]$MeasureBinary)
    $power = [ordered]@{ status = 'NOT_OBSERVED'; value = $null; exit_code = $null }
    try {
        $power.value = (& "$env:SystemRoot/System32/powercfg.exe" /GETACTIVESCHEME 2>&1 | Out-String).Trim()
        $power.exit_code = $LASTEXITCODE
        if ($LASTEXITCODE -eq 0) { $power.status = 'OBSERVED' }
    } catch { $power.value = $_.Exception.Message }
    $processes = @()
    try {
        $processes = @(Get-Process -ErrorAction Stop | Sort-Object CPU -Descending | Select-Object -First 8 ProcessName, Id, CPU, WorkingSet64)
    } catch { $processes = @([ordered]@{ error = $_.Exception.Message }) }
    $sources = @('crates/ultratasks-core/examples/measure.rs', 'scripts/performance.ps1', 'scripts/summarize-performance.py', 'crates/ultratasks-core/src/fixtures.rs')
    $hashes = foreach ($relative in $sources) { Read-FileIdentity (Join-Path $Context.Root $relative) }
    return [ordered]@{
        captured_utc = [DateTime]::UtcNow.ToString('o')
        source_sha = Get-WorkspaceRevision $Context
        execution_host = $(if ($env:RUNNER_ENVIRONMENT -eq 'github-hosted') { 'github-hosted' } else { 'explicit-remote-host' })
        repository = $env:GITHUB_REPOSITORY; run_id = $env:GITHUB_RUN_ID; run_attempt = $env:GITHUB_RUN_ATTEMPT
        runner_image = $env:ImageOS; runner_image_version = $env:ImageVersion
        performance_scope = 'Remote host only; these results do not describe the user PC.'
        verified_build = $script:VerifiedBuild
        run_timeout_seconds = $RunTimeoutSeconds
        os = Read-CimObservation 'Win32_OperatingSystem' @('Caption', 'Version', 'BuildNumber', 'OSArchitecture', 'LastBootUpTime')
        computer = Read-CimObservation 'Win32_ComputerSystem' @('Manufacturer', 'Model', 'TotalPhysicalMemory', 'NumberOfLogicalProcessors')
        cpu = Read-CimObservation 'Win32_Processor' @('Name', 'NumberOfCores', 'NumberOfLogicalProcessors', 'MaxClockSpeed', 'LoadPercentage')
        gpu = Read-CimObservation 'Win32_VideoController' @('Name', 'DriverVersion', 'AdapterRAM', 'CurrentHorizontalResolution', 'CurrentVerticalResolution', 'CurrentRefreshRate')
        disks = Read-CimObservation 'Win32_DiskDrive' @('Model', 'InterfaceType', 'MediaType', 'Size')
        volumes = Read-CimObservation 'Win32_LogicalDisk' @('DeviceID', 'DriveType', 'FileSystem', 'Size', 'FreeSpace')
        active_power_scheme = $power
        logical_processors_for_cpu_percent = [Environment]::ProcessorCount
        top_processes_by_lifetime_cpu_seconds = $processes
        load_note = 'WindowsにUnix load averageはない。CPU.LoadPercentageはCIMの瞬間的参考値。上位processのCPUは生涯累積秒であり現在使用率ではない。'
        monitor_hz_operator_recorded = $(if ($MonitorHz -gt 0) { $MonitorHz } else { $null })
        monitor_note = 'GPU.CurrentRefreshRateはadapter報告値であり、対象windowのmonitor Hzの証明ではない。MonitorHzは主担当の観測値を明示入力する。'
        workspace = $Context.Root; target = $Context.Target; evidence = $Context.Evidence
        toolchain = $Context.Toolchain; target_triple = $Context.Triple; profile_requested = 'release'
        lock = Read-FileIdentity (Join-Path $Context.Root 'Cargo.lock')
        product_binary = Read-FileIdentity $Binary
        measure_binary = Read-FileIdentity $MeasureBinary
        sources = @($hashes)
        powershell = $PSVersionTable.PSVersion.ToString()
        rustflags = $env:RUSTFLAGS; cargo_encoded_rustflags = $env:CARGO_ENCODED_RUSTFLAGS
        fixture = [ordered]@{ sizes = @($script:ParsedSizes); seed = $Seed; date = $Date; zone = $Zone; distribution = $Distribution }
        worker_queue_wait = 'NOT_OBSERVED'; actual_presentation_time = 'NOT_OBSERVED'
    }
}

function Read-WindowMetrics {
    param([IntPtr]$Handle)
    if ($Handle -eq [IntPtr]::Zero) { return [ordered]@{ status = 'NOT_OBSERVED'; reason = 'MainWindowHandleがありません' } }
    try {
        if (-not ('UltraTasksPerformance.WindowRead' -as [type])) {
            Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace UltraTasksPerformance {
    public static class WindowRead {
        [StructLayout(LayoutKind.Sequential)]
        public struct Rect { public int Left, Top, Right, Bottom; }
        [DllImport("user32.dll", SetLastError = true)]
        public static extern bool GetClientRect(IntPtr handle, out Rect rect);
        [DllImport("user32.dll")]
        public static extern uint GetDpiForWindow(IntPtr handle);
        public static int[] Read(IntPtr handle) {
            Rect rect;
            if (!GetClientRect(handle, out rect)) throw new System.ComponentModel.Win32Exception();
            return new int[] { rect.Right - rect.Left, rect.Bottom - rect.Top, (int)GetDpiForWindow(handle) };
        }
    }
}
'@
        }
        $values = [UltraTasksPerformance.WindowRead]::Read($Handle)
        return [ordered]@{ status = 'OBSERVED'; client_width = $values[0]; client_height = $values[1]; window_dpi = $values[2]; units = 'GetClientRectの呼出し側DPI contextの単位。logical/physical pixelの断定なし'; hwnd = $Handle.ToInt64() }
    } catch {
        return [ordered]@{ status = 'NOT_OBSERVED'; reason = $_.Exception.Message }
    }
}

function Read-ProcessSnapshot {
    param([int]$Id, [long]$ExpectedStartTicks)
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $targetProcess = Get-Process -Id $Id -ErrorAction Stop
    try {
        $targetProcess.Refresh()
        if ($targetProcess.HasExited -or $targetProcess.StartTime.ToUniversalTime().Ticks -ne $ExpectedStartTicks) {
            throw '対象PIDが終了または再利用されました。'
        }
        $snapshot = [ordered]@{
            cpu_total_ms = $targetProcess.TotalProcessorTime.TotalMilliseconds
            private_bytes = $targetProcess.PrivateMemorySize64
            working_set_bytes = $targetProcess.WorkingSet64
            observed_utc = [DateTime]::UtcNow.ToString('o')
            observer_read_ms = $watch.Elapsed.TotalMilliseconds
        }
        return $snapshot
    } finally { $targetProcess.Dispose() }
}

function Write-ProcessSample {
    param($Row, $JsonWriter, $CsvWriter)
    $JsonWriter.WriteLine(($Row | ConvertTo-Json -Compress -Depth 6))
    $cells = foreach ($value in $Row.Values) {
        $text = if ($null -eq $value) { '' } elseif ($value -is [IFormattable]) { $value.ToString($null, [Globalization.CultureInfo]::InvariantCulture) } else { [string]$value }
        '"' + $text.Replace('"', '""') + '"'
    }
    $CsvWriter.WriteLine(($cells -join ','))
    $JsonWriter.Flush()
    $CsvWriter.Flush()
}

function Invoke-ProcessObservation {
    param($Context, $Environment)
    $run = Join-Path $Context.Evidence ('process-' + [guid]::NewGuid().ToString('N'))
    [void][System.IO.Directory]::CreateDirectory($run)
    $operation = 'process_' + $Observation
    $plannedSamples = [int][Math]::Ceiling($Seconds * 1000.0 / $IntervalMs)
    $identity = $null
    $identityError = $null
    try {
        $targetProcess = Get-Process -Id $ProcessId -ErrorAction Stop
        try {
            $identity = [ordered]@{
                process_id = $ProcessId; start_utc = $targetProcess.StartTime.ToUniversalTime().ToString('o')
                start_ticks = $targetProcess.StartTime.ToUniversalTime().Ticks
                executable = $targetProcess.Path
                window = Read-WindowMetrics $targetProcess.MainWindowHandle
            }
            if ($Binary -and -not [string]::Equals((Get-AbsolutePath $Binary), (Get-AbsolutePath $identity.executable), [StringComparison]::OrdinalIgnoreCase)) {
                throw '指定BinaryとPIDの実行ファイルが一致しません。'
            }
            $identity.binary = Read-FileIdentity $identity.executable
        } finally { $targetProcess.Dispose() }
    } catch { $identityError = $_.Exception.Message }
    $manifest = [ordered]@{
        schema_version = 1; kind = 'process'; sizes = @($script:ParsedSizes)
        seed = $Seed; date = $Date; zone = $Zone; distribution = $Distribution
        fixture_provenance = '利用者申告。PIDのDB内容は外部観測から確認しない'
        samples = $plannedSamples; warmup = 0; trials = $Trials; operations = @($operation)
        seconds_per_trial = $Seconds; interval_ms_requested = $IntervalMs
        observation = $Observation; motion_operator_recorded = $Motion
        identity = $identity; identity_error = $identityError; environment = $Environment
        executable = $(if ($identity) { $identity.executable } else { $null })
        lock_file = (Join-Path $Context.Root 'Cargo.lock'); harness_source = $PSCommandPath
        cpu_definition = '100 * delta(process CPU ms) / observed interval ms。全論理CPU基準はさらにEnvironment.ProcessorCountで割る。'
        verified_definition = '同一PID・開始時刻、単調CPU時間、有効間隔を確認。UI操作成功の判定ではない。'
        idle_definition = 'idleは主担当が入力/animation終了後に開始する観測ラベル。操作有無を自動判定しない。'
        profile = '外部PIDのrelease/debugは自動判定できない。指定binary hashで製品build証拠に照合する'
        notify_count = $null; frame_request_count = $null; input_latency_ms = $null; presentation_ms = $null
    }
    Write-Utf8 (Join-Path $run 'run.json') ($manifest | ConvertTo-Json -Depth 15)
    $jsonWriter = [System.IO.StreamWriter]::new((Join-Path $run 'process-samples.jsonl'), $false, $script:Utf8)
    $csvWriter = [System.IO.StreamWriter]::new((Join-Path $run 'process-samples.csv'), $false, $script:Utf8)
    $csvWriter.WriteLine('schema_version,size,distribution,trial,phase,operation,sample,process_id,observed_utc,interval_ms,cpu_delta_ms,cpu_one_core_pct,cpu_all_cores_pct,private_bytes,working_set_bytes,observer_read_ms,verified,status,error')
    $failed = $false
    $trialResults = New-Object 'System.Collections.Generic.List[object]'
    try {
        for ($trial = 1; $trial -le $Trials; $trial++) {
            $index = 0
            try {
                if ($identityError) { throw $identityError }
                $watch = [System.Diagnostics.Stopwatch]::StartNew()
                $previous = Read-ProcessSnapshot $ProcessId $identity.start_ticks
                $previousTime = $watch.Elapsed.TotalMilliseconds
                $baselineTime = $previousTime
                for ($index = 0; $index -lt $plannedSamples; $index++) {
                    $due = $baselineTime + [Math]::Min(($index + 1) * $IntervalMs, $Seconds * 1000.0)
                    $sleepMs = [Math]::Max(0, [Math]::Ceiling($due - $watch.Elapsed.TotalMilliseconds))
                    if ($sleepMs -gt 0) { [Threading.Thread]::Sleep([int]$sleepMs) }
                    $current = Read-ProcessSnapshot $ProcessId $identity.start_ticks
                    $now = $watch.Elapsed.TotalMilliseconds
                    $elapsed = $now - $previousTime
                    $cpuDelta = $current.cpu_total_ms - $previous.cpu_total_ms
                    if ($elapsed -le 0 -or $cpuDelta -lt 0) { throw 'CPUまたは観測時計が単調ではありません。' }
                    $oneCore = 100.0 * $cpuDelta / $elapsed
                    $row = [ordered]@{
                        schema_version = 1; size = $script:ParsedSizes[0]; distribution = $Distribution
                        trial = $trial; phase = 'sample'; operation = $operation; sample = $index
                        process_id = $ProcessId; observed_utc = $current.observed_utc
                        interval_ms = $elapsed; cpu_delta_ms = $cpuDelta; cpu_one_core_pct = $oneCore
                        cpu_all_cores_pct = $oneCore / [Environment]::ProcessorCount
                        private_bytes = $current.private_bytes; working_set_bytes = $current.working_set_bytes
                        observer_read_ms = $current.observer_read_ms; verified = $true; status = 'success'; error = $null
                    }
                    Write-ProcessSample $row $jsonWriter $csvWriter
                    $previous = $current
                    $previousTime = $now
                }
                $trialResults.Add([ordered]@{ trial = $trial; completed = $true; observed_duration_ms = $previousTime - $baselineTime; samples = $plannedSamples })
            } catch {
                $failed = $true
                $errorText = $_.Exception.Message
                $row = [ordered]@{
                    schema_version = 1; size = $script:ParsedSizes[0]; distribution = $Distribution
                    trial = $trial; phase = 'sample'; operation = $operation; sample = $index
                    process_id = $ProcessId; observed_utc = [DateTime]::UtcNow.ToString('o')
                    interval_ms = $null; cpu_delta_ms = $null; cpu_one_core_pct = $null; cpu_all_cores_pct = $null
                    private_bytes = $null; working_set_bytes = $null; observer_read_ms = $null
                    verified = $false; status = 'failure'; error = $errorText
                }
                Write-ProcessSample $row $jsonWriter $csvWriter
                $trialResults.Add([ordered]@{ trial = $trial; completed = $false; error = $errorText })
                break
            }
        }
    } finally {
        $jsonWriter.Dispose(); $csvWriter.Dispose()
        Write-Utf8 (Join-Path $run 'completion.json') (([ordered]@{ completed = -not $failed; trials = @($trialResults.ToArray()); failure_count = [int]$failed; timeout_count = 0 }) | ConvertTo-Json -Depth 8)
    }
    $Context.Steps.Add([pscustomobject]@{ name = 'observe-pid'; status = $(if ($failed) { 'FAILED' } else { 'PASSED' }); exit_code = [int]$failed; evidence = $run })
    if ($failed -and $Context.ExitCode -eq 0) { $Context.ExitCode = 1 }
    Write-Host "PROCESS_SAMPLES: $run"
}

$context = $null
$script:VerifiedBuild = $null
try {
    Assert-RemoteExecution
    if ($MeasureBinary -and ($Mode -ne 'Core' -or -not $BuildManifest)) { throw 'MeasureBinary requires Mode Core and BuildManifest from a successful full verification.' }
    if ($BuildManifest -and -not $MeasureBinary) { throw 'BuildManifest requires MeasureBinary.' }
    $script:ParsedSizes = @($Sizes | ForEach-Object { $_.Split(',') } | ForEach-Object { [int]$_.Trim() })
    if ($script:ParsedSizes.Count -eq 0 -or @($script:ParsedSizes | Where-Object { $_ -notin @(1000, 10000) }).Count -gt 0 -or @($script:ParsedSizes | Select-Object -Unique).Count -ne $script:ParsedSizes.Count) {
        throw 'Sizesは重複のない1000,10000を指定してください。'
    }
    if ($Mode -eq 'Observe' -and ($ProcessId -eq 0 -or $script:ParsedSizes.Count -ne 1)) { throw 'ObserveにはProcessIdと単一のSizesが必要です。' }
    if ($Mode -eq 'Core' -and $ProcessId -ne 0) { throw '指定PIDの観測にはMode Observeを使ってください。' }
    if (-not $WorkspaceRoot) { $WorkspaceRoot = Split-Path $PSScriptRoot -Parent }
    $root = Get-AbsolutePath $WorkspaceRoot
    $parent = Split-Path $root -Parent
    $work = Split-Path $parent -Parent
    $base = if ((Split-Path $root -Leaf) -eq 'repository' -and (Split-Path $work -Leaf) -eq 'work') { Split-Path $work -Parent } elseif ((Split-Path $root -Leaf) -eq 'prototype') { $parent } else { Join-Path $root 'out/ultratasks' }
    if (-not $TargetDir) { $TargetDir = Join-Path $base 'build/performance' }
    if (-not $EvidenceDir) { $EvidenceDir = Join-Path $base 'evidence/performance' }
    $context = New-WorkspaceContext $root $TargetDir $EvidenceDir 'performance'
    if ($Binary) {
        if (-not [System.IO.Path]::IsPathRooted($Binary)) { throw 'Binaryは絶対パスで指定してください。' }
        $Binary = Get-AbsolutePath $Binary
        [void](Read-FileIdentity $Binary)
    }
    $measure = $null
    if ($Mode -eq 'Core') {
        if ($MeasureBinary) {
            if (-not [System.IO.Path]::IsPathRooted($MeasureBinary) -or -not [System.IO.Path]::IsPathRooted($BuildManifest)) { throw 'MeasureBinary and BuildManifest must be absolute paths.' }
            $manifest = [System.IO.File]::ReadAllText((Get-AbsolutePath $BuildManifest)) | ConvertFrom-Json
            $lockHash = (Get-FileHash -LiteralPath (Join-Path $context.Root 'Cargo.lock') -Algorithm SHA256).Hash
            if ($manifest.schema_version -ne 1 -or $manifest.verification -ne 'full-passed' -or
                $manifest.source_sha -ne (Get-WorkspaceRevision $context) -or $manifest.lock_sha256 -ne $lockHash -or
                $manifest.toolchain -ne $context.Toolchain -or $manifest.target_triple -ne $context.Triple -or $manifest.profile -ne 'release') {
                throw 'BuildManifest does not match this verified commit, lock file or release toolchain.'
            }
            if ($env:GITHUB_ACTIONS -eq 'true' -and ($manifest.run_id -ne $env:GITHUB_RUN_ID -or $manifest.run_attempt -ne $env:GITHUB_RUN_ATTEMPT)) {
                throw 'CI must reuse the measurement binary verified in this run and attempt.'
            }
            foreach ($entry in @(@{ Path = $MeasureBinary; Name = 'measure.exe' }, @{ Path = $Binary; Name = 'ultratasks-desktop.exe' })) {
                if (-not $entry.Path) { continue }
                $identity = Read-FileIdentity $entry.Path
                $expected = @($manifest.files | Where-Object { $_.name -eq $entry.Name })
                if ($expected.Count -ne 1 -or $identity.sha256 -ne $expected[0].sha256 -or $identity.bytes -ne $expected[0].bytes) { throw "Verified binary differs: $($entry.Name)" }
            }
            $script:VerifiedBuild = $manifest
            $measure = Get-AbsolutePath $MeasureBinary
            Add-SkippedStep $context 'build-measure' 'Explicit verified MeasureBinary reused; no compiler or cache initialization.'
        } elseif (Initialize-WindowsEnvironment $context) {
            $build = Invoke-WorkspaceCargo $context 'build-measure' @('build', '-p', 'ultratasks-core', '--example', 'measure', '--features', 'fixtures', '--release', '--locked', '--target', $context.Triple)
            if ($build.exit_code -eq 0) { $measure = Join-Path $context.Target ($context.Triple + '/release/examples/measure.exe') }
        }
    }
    $environment = Read-HostEnvironment $context $measure
    $environmentPath = Join-Path $context.Evidence 'performance-environment.json'
    Write-Utf8 $environmentPath ($environment | ConvertTo-Json -Depth 15)
    if ($Mode -eq 'Core' -and $measure) {
        [void](Invoke-LoggedStep $context 'measure-core' $measure @('--sizes', ($script:ParsedSizes -join ','), '--seed', $Seed.ToString(), '--date', $Date, '--zone', $Zone, '--warmup', $Warmup.ToString(), '--samples', $Samples.ToString(), '--trials', $Trials.ToString(), '--distribution', $Distribution, '--timeout-ms', $TimeoutMs.ToString(), '--environment', $environmentPath, '--out', $context.Evidence) -TimeoutSeconds $RunTimeoutSeconds)
        Add-SkippedStep $context 'ui-startup-input-scroll-idle' 'このModeはcoreのみ。GUI操作は主担当が行い、Observeで指定PIDを外部観測する。'
    } elseif ($Mode -eq 'Observe') {
        Invoke-ProcessObservation $context $environment
    } else {
        Add-SkippedStep $context 'measure-core' 'ビルドまたは開発環境の準備が失敗したため未実行。'
    }
    Write-Utf8 (Join-Path $context.Evidence 'environment-after.json') ((Read-HostEnvironment $context $measure) | ConvertTo-Json -Depth 15)
    $manifests = @(Get-ChildItem -LiteralPath $context.Evidence -Filter 'run.json' -Recurse -File)
    if ($manifests.Count -gt 0) {
        [void](Invoke-LoggedStep $context 'summarize-performance' (Get-Command python.exe -ErrorAction Stop).Source @('-I', (Join-Path $context.Root 'scripts/summarize-performance.py'), '--input', $context.Evidence))
    } else {
        Add-SkippedStep $context 'summarize-performance' '測定manifestなし。未実行を成功sampleにしない。'
    }
} catch {
    if ($null -eq $context) { Write-Error $_; exit 1 }
    Save-RunError $context $_
} finally {
    if ($null -ne $context) { Save-RunResult $context }
}
exit $context.ExitCode
