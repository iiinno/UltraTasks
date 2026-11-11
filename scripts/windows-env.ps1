[CmdletBinding()]
param(
    [string]$WorkspaceRoot,
    [string]$TargetDir,
    [string]$EvidenceDir
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:WorkspaceScripts = $PSScriptRoot
$script:Utf8 = New-Object System.Text.UTF8Encoding($false)

function Write-Utf8 {
    param([string]$Path, [string]$Text)
    [System.IO.File]::WriteAllText($Path, $Text, $script:Utf8)
}

function Get-AbsolutePath {
    param([string]$Path)
    if (-not [System.IO.Path]::IsPathRooted($Path)) {
        $Path = Join-Path (Get-Location).Path $Path
    }
    return [System.IO.Path]::GetFullPath($Path)
}

function Quote-BatchArgument {
    param([string]$Value)
    if ($Value -match '["\r\n\x00]') {
        throw 'Arguments must not contain quotes, NUL or newlines.'
    }
    $escaped = $Value.Replace('%', '%%')
    $escaped = [regex]::Replace($escaped, '(\\+)$', '$1$1')
    return '"' + $escaped + '"'
}

function Assert-RemoteExecution {
    $hosted = $env:GITHUB_ACTIONS -eq 'true' -and $env:RUNNER_ENVIRONMENT -eq 'github-hosted' -and $env:RUNNER_OS -eq 'Windows'
    $remote = $env:GITHUB_ACTIONS -ne 'true' -and $env:ULTRATASKS_REMOTE_HOST -eq '1'
    if (-not ($hosted -or $remote)) {
        throw 'Rust builds, fixtures and measurements are remote-only. Use the GitHub-hosted Windows workflow. For an existing binary use scripts/start-demo.ps1. Remote Windows host setup is documented in docs/development.md; the development PC must not bypass this guard.'
    }
}

function Get-WorkspaceRevision {
    param($Context)
    $revision = & (Get-Command git.exe -ErrorAction Stop).Source -C $Context.Root rev-parse HEAD
    if ($LASTEXITCODE -ne 0 -or "$revision" -notmatch '^[0-9a-f]{40}$') { throw 'Cannot identify the workspace commit.' }
    if ($env:GITHUB_ACTIONS -eq 'true' -and $revision -ne $env:GITHUB_SHA) { throw 'Checkout SHA differs from GITHUB_SHA.' }
    return [string]$revision
}

function New-WorkspaceContext {
    param([string]$WorkspaceRoot, [string]$TargetDir, [string]$EvidenceDir, [string]$Purpose)
    Assert-RemoteExecution
    if (-not $WorkspaceRoot) { $WorkspaceRoot = Split-Path $script:WorkspaceScripts -Parent }
    $root = Get-AbsolutePath $WorkspaceRoot
    $manifest = Join-Path $root 'Cargo.toml'
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) { throw "Workspace manifest not found: $manifest" }
    $parent = Split-Path $root -Parent
    $work = Split-Path $parent -Parent
    if ((Split-Path $root -Leaf) -eq 'repository' -and (Split-Path $work -Leaf) -eq 'work') {
        $lane = Split-Path $parent -Leaf
        $base = Split-Path $work -Parent
        if (-not $TargetDir) { $TargetDir = Join-Path $base "build/$lane" }
        if (-not $EvidenceDir) { $EvidenceDir = Join-Path $base "evidence/$lane" }
    } elseif ((Split-Path $root -Leaf) -eq 'prototype') {
        if (-not $TargetDir) { $TargetDir = Join-Path $parent 'build/prototype' }
        if (-not $EvidenceDir) { $EvidenceDir = Join-Path $parent 'evidence/prototype' }
    } else {
        if (-not $TargetDir) { $TargetDir = Join-Path $root 'target' }
        if (-not $EvidenceDir) { $EvidenceDir = Join-Path $root 'evidence' }
    }
    $target = Get-AbsolutePath $TargetDir
    $evidence = Get-AbsolutePath $EvidenceDir
    $run = Join-Path $evidence ((Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssfffZ') + '-' + $Purpose + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
    [void][System.IO.Directory]::CreateDirectory($target)
    [void][System.IO.Directory]::CreateDirectory($run)
    $context = [pscustomobject]@{
        Root = $root; Manifest = $manifest; Target = $target; Evidence = $run
        Triple = 'x86_64-pc-windows-msvc'; Toolchain = '1.99.1-x86_64-pc-windows-msvc'; VsDevCmd = $null
        Steps = New-Object 'System.Collections.Generic.List[object]'
        ExitCode = 0; Purpose = $Purpose
    }
    $lock = Join-Path $root 'Cargo.lock'
    $lockHash = if (Test-Path -LiteralPath $lock) { (Get-FileHash -LiteralPath $lock -Algorithm SHA256).Hash } else { $null }
    Write-Utf8 (Join-Path $run 'environment.json') (([ordered]@{
        workspace = $root; target = $target; evidence = $run; purpose = $Purpose
        toolchain = $context.Toolchain; target_triple = $context.Triple; profile = 'release'
        lock_sha256 = $lockHash; powershell = $PSVersionTable.PSVersion.ToString()
        os = [Environment]::OSVersion.VersionString; rustflags = $env:RUSTFLAGS
        cargo_encoded_rustflags = $env:CARGO_ENCODED_RUSTFLAGS
        execution_host = $(if ($env:RUNNER_ENVIRONMENT -eq 'github-hosted') { 'github-hosted' } else { 'explicit-remote-host' })
        source_sha = $env:GITHUB_SHA; repository = $env:GITHUB_REPOSITORY
        run_id = $env:GITHUB_RUN_ID; run_attempt = $env:GITHUB_RUN_ATTEMPT
        runner_image = $env:ImageOS; runner_image_version = $env:ImageVersion
        performance_scope = 'Remote host only; these results do not describe the user PC.'
    }) | ConvertTo-Json -Depth 5)
    return $context
}

function Add-SkippedStep {
    param($Context, [string]$Name, [string]$Reason)
    $Context.Steps.Add([pscustomobject]@{ name = $Name; status = 'NOT_RUN'; exit_code = $null; reason = $Reason })
    Write-Host "NOT_RUN $Name : $Reason"
}

function Invoke-LoggedStep {
    param(
        $Context, [string]$Name, [string]$FilePath,
        [string[]]$Arguments = @(), [int]$TimeoutSeconds = 0
    )
    $prefix = Join-Path $Context.Evidence ('{0:D2}-{1}' -f ($Context.Steps.Count + 1), $Name)
    $stdout = $prefix + '.stdout.log'
    $stderr = $prefix + '.stderr.log'
    $batch = $prefix + '.cmd'
    $command = (Quote-BatchArgument $FilePath) + ' ' + (($Arguments | ForEach-Object { Quote-BatchArgument $_ }) -join ' ')
    $lines = @('@echo off', 'chcp 65001 >nul', 'setlocal DisableDelayedExpansion')
    if ($Context.VsDevCmd) {
        $lines += ('call ' + (Quote-BatchArgument $Context.VsDevCmd) + ' -no_logo -arch=x64 -host_arch=x64 1>' + (Quote-BatchArgument ($prefix + '.msvc.stdout.log')) + ' 2>' + (Quote-BatchArgument ($prefix + '.msvc.stderr.log')))
        $lines += 'if errorlevel 1 exit /b %errorlevel%'
    }
    $lines += ('cd /d ' + (Quote-BatchArgument $Context.Root))
    $lines += 'if errorlevel 1 exit /b %errorlevel%'
    $lines += ('set "CARGO_TARGET_DIR=' + $Context.Target.Replace('%', '%%') + '"')
    $lines += ('set "CARGO_BUILD_TARGET=' + $Context.Triple + '"')
    $lines += $command
    $lines += 'exit /b %errorlevel%'
    Write-Utf8 $batch (($lines -join "`r`n") + "`r`n")
    Write-Utf8 ($prefix + '.command.json') (([ordered]@{
        executable = $FilePath; arguments = @($Arguments); working_directory = $Context.Root
        vsdevcmd = $Context.VsDevCmd; target_directory = $Context.Target
        timeout_seconds = $TimeoutSeconds
    }) | ConvertTo-Json -Depth 5)
    Write-Host "RUN $Name : $command"
    $started = [DateTime]::UtcNow
    $code = 1
    $outStream = [System.IO.File]::Create($stdout)
    $errStream = [System.IO.File]::Create($stderr)
    $process = New-Object System.Diagnostics.Process
    try {
        $process.StartInfo.FileName = $env:ComSpec
        $process.StartInfo.Arguments = '/d /s /c ""' + $batch + '""'
        $process.StartInfo.WorkingDirectory = $Context.Root
        $process.StartInfo.UseShellExecute = $false
        $process.StartInfo.CreateNoWindow = $true
        $process.StartInfo.RedirectStandardOutput = $true
        $process.StartInfo.RedirectStandardError = $true
        [void]$process.Start()
        $copyOut = $process.StandardOutput.BaseStream.CopyToAsync($outStream)
        $copyErr = $process.StandardError.BaseStream.CopyToAsync($errStream)
        if ($TimeoutSeconds -gt 0 -and -not $process.WaitForExit($TimeoutSeconds * 1000)) {
            & "$env:SystemRoot/System32/taskkill.exe" /PID $process.Id /T /F 1> ($prefix + '.timeout.log') 2>&1
            $process.WaitForExit()
            $code = 124
        } else {
            $process.WaitForExit()
            $code = $process.ExitCode
        }
        [void]$copyOut.GetAwaiter().GetResult()
        [void]$copyErr.GetAwaiter().GetResult()
    } catch {
        Write-Utf8 ($prefix + '.launcher-error.log') $_.Exception.ToString()
        $code = 1
    } finally {
        $outStream.Dispose()
        $errStream.Dispose()
        $process.Dispose()
    }
    $status = if ($code -eq 0) { 'PASSED' } else { 'FAILED' }
    $result = [pscustomobject]@{
        name = $Name; status = $status; exit_code = [int]$code
        started_utc = $started.ToString('o'); duration_ms = ([DateTime]::UtcNow - $started).TotalMilliseconds
        stdout = $stdout; stderr = $stderr; command = $prefix + '.command.json'
    }
    $Context.Steps.Add($result)
    if ($code -ne 0 -and $Context.ExitCode -eq 0) { $Context.ExitCode = [int]$code }
    Write-Host "$status $Name exit=$code"
    return $result
}

function Invoke-WorkspaceCargo {
    param($Context, [string]$Name, [string[]]$Arguments, [string[]]$Tail = @())
    Assert-RemoteExecution
    $all = @('+' + $Context.Toolchain) + $Arguments + @('--manifest-path', $Context.Manifest) + $Tail
    return Invoke-LoggedStep $Context $Name (Get-Command cargo.exe -ErrorAction Stop).Source $all
}

function Initialize-WorkspaceArtifacts {
    param($Context)
    Assert-RemoteExecution
    $profileDir = Join-Path $Context.Target "$($Context.Triple)/release"
    $stamp = Join-Path $profileDir '.ultratasks-workspace'
    if ((Test-Path -LiteralPath $stamp -PathType Leaf) -and
        [System.IO.File]::ReadAllText($stamp).Equals($Context.Root, [StringComparison]::OrdinalIgnoreCase)) {
        return $true
    }
    $metadata = Invoke-WorkspaceCargo $Context 'workspace-cache-metadata' @('metadata', '--no-deps', '--locked', '--format-version', '1')
    if ($metadata.exit_code -ne 0) { return $false }
    $workspace = [System.IO.File]::ReadAllText($metadata.stdout) | ConvertFrom-Json
    $packages = @($workspace.packages | Where-Object { $_.id -in $workspace.workspace_members })
    if ($packages.Count -eq 0) { throw 'No workspace packages found for cache initialization.' }
    $arguments = @('clean', '--release', '--target', $Context.Triple)
    foreach ($package in $packages) { $arguments += @('--package', $package.name) }
    $clean = Invoke-WorkspaceCargo $Context 'workspace-cache-reset' $arguments
    if ($clean.exit_code -ne 0) { return $false }
    [void][System.IO.Directory]::CreateDirectory($profileDir)
    Write-Utf8 $stamp $Context.Root
    return $true
}

function Initialize-WindowsEnvironment {
    param($Context)
    Assert-RemoteExecution
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) { throw 'vswhere.exe not found; install Visual Studio C++ Build Tools and a Windows SDK.' }
    $result = Invoke-LoggedStep $Context 'vswhere' $vswhere @('-latest', '-products', '*', '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath')
    if ($result.exit_code -ne 0) { return $false }
    $installation = [System.IO.File]::ReadAllText($result.stdout).Trim()
    if (-not $installation) { throw 'No Visual Studio installation has the VC x64 tools.' }
    $Context.VsDevCmd = Join-Path $installation 'Common7/Tools/VsDevCmd.bat'
    if (-not (Test-Path -LiteralPath $Context.VsDevCmd -PathType Leaf)) { throw 'VsDevCmd.bat not found.' }
    foreach ($tool in @('cl', 'link', 'rc', 'fxc')) {
        $result = Invoke-LoggedStep $Context "find-$tool" "$env:SystemRoot/System32/where.exe" @("$tool.exe")
        if ($result.exit_code -ne 0) { return $false }
        $first = ([System.IO.File]::ReadAllLines($result.stdout) | Select-Object -First 1)
        if ($tool -in @('cl', 'link') -and -not $first.StartsWith($installation + '\', [StringComparison]::OrdinalIgnoreCase)) {
            throw "The first $tool.exe is not the detected MSVC tool: $first"
        }
    }
    $rust = Invoke-LoggedStep $Context 'rustc-version' (Get-Command rustc.exe -ErrorAction Stop).Source @(('+' + $Context.Toolchain), '--version', '--verbose')
    if ($rust.exit_code -ne 0) { return $false }
    $version = [System.IO.File]::ReadAllText($rust.stdout)
    if ($version -notmatch '(?m)^host: x86_64-pc-windows-msvc\r?$' -or $version -notmatch '(?m)^release: 1\.99\.1\r?$') {
        throw 'Rust must be version 1.99.1 with the x86_64-pc-windows-msvc host.'
    }
    # cargo --version has no manifest option, so it still runs from this workspace.
    $cargo = Invoke-LoggedStep $Context 'cargo-version' (Get-Command cargo.exe -ErrorAction Stop).Source @(('+' + $Context.Toolchain), '--version')
    if ($cargo.exit_code -ne 0) { return $false }
    return Initialize-WorkspaceArtifacts $Context
}

function Save-RunResult {
    param($Context)
    Write-Utf8 (Join-Path $Context.Evidence 'summary.json') (([ordered]@{
        purpose = $Context.Purpose; workspace = $Context.Root; target = $Context.Target
        exit_code = $Context.ExitCode; steps = @($Context.Steps.ToArray())
    }) | ConvertTo-Json -Depth 8)
    Write-Host "Evidence: $($Context.Evidence)"
}

function Save-RunError {
    param($Context, $ErrorRecord)
    if ($Context.ExitCode -eq 0) { $Context.ExitCode = 1 }
    Write-Utf8 (Join-Path $Context.Evidence 'script-error.log') $ErrorRecord.ToString()
    Write-Host "FAILED: $ErrorRecord"
}

if ($MyInvocation.InvocationName -ne '.') {
    $context = $null
    try {
        $context = New-WorkspaceContext $WorkspaceRoot $TargetDir $EvidenceDir 'environment'
        [void](Initialize-WindowsEnvironment $context)
    } catch {
        if ($null -eq $context) { Write-Error $_; exit 1 }
        Save-RunError $context $_
    } finally {
        if ($null -ne $context) { Save-RunResult $context }
    }
    exit $context.ExitCode
}
