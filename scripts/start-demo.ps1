[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [string]$Binary,
    [string]$DbPath,
    [string]$Date = '',
    [string]$Zone = 'Asia/Tokyo',
    [string]$WorkspaceRoot,
    [string]$TargetDir,
    [string]$EvidenceDir
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-LaunchPath {
    param([string]$Path)
    if (-not [System.IO.Path]::IsPathRooted($Path)) { $Path = Join-Path (Get-Location).Path $Path }
    return [System.IO.Path]::GetFullPath($Path)
}

function Quote-LaunchArgument {
    param([string]$Value)
    if ($Value -match '["\r\n\x00]') { throw 'Arguments must not contain quotes, NUL or newlines.' }
    return '"' + [regex]::Replace($Value, '(\\+)$', '$1$1') + '"'
}

try {
    if (-not $WorkspaceRoot) { $WorkspaceRoot = Split-Path $PSScriptRoot -Parent }
    $workspace = Get-LaunchPath $WorkspaceRoot
    $local = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
    $manifestPath = $null
    if ($Binary -and $TargetDir) { throw 'Specify Binary or TargetDir, not both.' }
    if ($TargetDir) {
        $release = Join-Path (Get-LaunchPath $TargetDir) 'x86_64-pc-windows-msvc/release'
        $Binary = Join-Path $release 'ultratasks-desktop.exe'
    } elseif (-not $Binary -and (Test-Path -LiteralPath (Join-Path $workspace 'ultratasks-desktop.exe') -PathType Leaf)) {
        $Binary = Join-Path $workspace 'ultratasks-desktop.exe'
    } elseif (-not $Binary -and $local) {
        $pointerPath = Join-Path $local 'UltraTasks/artifacts/current.json'
        if (Test-Path -LiteralPath $pointerPath -PathType Leaf) {
            $pointer = [System.IO.File]::ReadAllText($pointerPath) | ConvertFrom-Json
            $Binary = $pointer.binary
            $manifestPath = $pointer.manifest
        }
    }
    if (-not $Binary -or -not (Test-Path -LiteralPath $Binary -PathType Leaf)) {
        throw 'No built executable is available. Download a successful Windows run with scripts/get-windows-artifact.ps1 -RunId <run-id> -ExpectedSha <40-character-checkout-SHA>, or specify -Binary <existing-exe>. This launcher never compiles locally.'
    }
    $binaryPath = Get-LaunchPath $Binary
    if ([IO.Path]::GetFileName($binaryPath) -ne 'ultratasks-desktop.exe') { throw 'Binary must be ultratasks-desktop.exe.' }
    if (-not $manifestPath) {
        $candidate = Join-Path (Split-Path $binaryPath -Parent) 'verified-build.json'
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { $manifestPath = $candidate }
    }
    $sourceSha = $null
    if ($manifestPath) {
        $manifest = [System.IO.File]::ReadAllText($manifestPath) | ConvertFrom-Json
        $entry = @($manifest.files | Where-Object { $_.name -eq 'ultratasks-desktop.exe' })
        if ($manifest.schema_version -ne 1 -or $manifest.verification -ne 'full-passed' -or $manifest.profile -ne 'release' -or
            $manifest.target_triple -ne 'x86_64-pc-windows-msvc' -or $manifest.toolchain -ne '1.99.1-x86_64-pc-windows-msvc' -or
            $manifest.source_sha -notmatch '^[0-9a-f]{40}$' -or $entry.Count -ne 1 -or
            (& {
                $stream = [IO.File]::OpenRead($binaryPath)
                try { Get-FileHash -InputStream $stream -Algorithm SHA256 }
                finally { $stream.Dispose() }
            }).Hash -ne $entry[0].sha256) {
            throw 'The binary does not match its verified build manifest. Download it again from the selected run.'
        }
        $sourceSha = $manifest.source_sha
    }
    if (-not $DbPath -or -not $EvidenceDir) {
        if (-not $local) { throw 'LocalApplicationData is unavailable; specify DbPath and EvidenceDir.' }
        $demoRoot = Join-Path $local 'UltraTasks/demo'
        if (-not $DbPath) { $DbPath = Join-Path $demoRoot 'demo.sqlite' }
        if (-not $EvidenceDir) { $EvidenceDir = Join-Path $demoRoot 'evidence' }
    }
    $db = Get-LaunchPath $DbPath
    $evidence = Get-LaunchPath $EvidenceDir
    $exists = Test-Path -LiteralPath $db
    if ($exists -and -not (Test-Path -LiteralPath $db -PathType Leaf)) { throw "DbPath must be a file: $db" }
    if (-not $exists) {
        $sidecars = @('-wal', '-shm', '-journal', '.ui.json') | ForEach-Object { $db + $_ }
        $sidecars += [IO.Path]::ChangeExtension($db, 'ui.json')
        foreach ($sidecar in $sidecars) {
            if (Test-Path -LiteralPath $sidecar) { throw "Database is missing but its sidecar exists: $sidecar. Choose a fresh DbPath." }
        }
    }
    $run = Join-Path $evidence ((Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssfffZ') + '-launch-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
    $arguments = @('--db', $db, '--demo', 'none', '--zone', $Zone, '--diagnostics-dir', (Join-Path $run 'diagnostics'))
    if ($Date) { $arguments += @('--date', $Date) }
    $argumentLine = ($arguments | ForEach-Object { Quote-LaunchArgument $_ }) -join ' '
    Write-Host "Binary: $binaryPath"
    Write-Host "Source SHA: $(if ($sourceSha) { $sourceSha } else { 'NOT_VERIFIED (explicit existing binary)' })"
    Write-Host "Database: $db (Demo none; no fixture generation)"
    Write-Host "Time zone: $Zone"
    Write-Host "Date: $(if ($Date) { $Date } else { 'today in the selected time zone' })"
    Write-Host "Logs: $run"
    if ($PSCmdlet.ShouldProcess($binaryPath, 'Launch the existing executable without building')) {
        [void][System.IO.Directory]::CreateDirectory((Split-Path $db -Parent))
        [void][System.IO.Directory]::CreateDirectory($run)
        $utf8 = New-Object System.Text.UTF8Encoding($false)
        [IO.File]::WriteAllText((Join-Path $run 'launch.json'), (([ordered]@{ binary = $binaryPath; source_sha = $sourceSha; arguments = $arguments; fixture = 'none' }) | ConvertTo-Json -Depth 5), $utf8)
        $process = Start-Process -FilePath $binaryPath -ArgumentList $argumentLine -WorkingDirectory $workspace -RedirectStandardOutput (Join-Path $run 'desktop.stdout.log') -RedirectStandardError (Join-Path $run 'desktop.stderr.log') -PassThru -Wait
        $code = $process.ExitCode
        $process.Dispose()
        exit $code
    }
} catch {
    Write-Error $_
    exit 1
}
