[CmdletBinding()]
param(
    [ValidateSet('fast', 'full')][string]$Mode = 'fast',
    [ValidateSet('Native', 'Skip')][string]$GuiMode = 'Native',
    [string]$WorkspaceRoot,
    [string]$TargetDir,
    [string]$EvidenceDir
)

. (Join-Path $PSScriptRoot 'windows-env.ps1') -WorkspaceRoot $WorkspaceRoot -TargetDir $TargetDir -EvidenceDir $EvidenceDir
$context = $null
try {
    $context = New-WorkspaceContext $WorkspaceRoot $TargetDir $EvidenceDir "verify-$Mode"
    $release = Join-Path $context.Target "$($context.Triple)/release"
    $buildManifest = Join-Path $release 'verified-build.json'
    if ($Mode -eq 'full' -and (Test-Path -LiteralPath $buildManifest -PathType Leaf)) { Remove-Item -LiteralPath $buildManifest }
    if (Initialize-WindowsEnvironment $context) {
        [void](Invoke-WorkspaceCargo $context 'fmt' @('fmt', '--all') @('--', '--check'))
        if ($Mode -eq 'fast') {
            [void](Invoke-WorkspaceCargo $context 'core-tests' @('test', '-p', 'ultratasks-core', '--release', '--locked', '--features', 'test-support,fixtures', '--target', $context.Triple))
            Add-SkippedStep $context 'native-smoke' 'fast mode does not run GUI checks'
        } else {
            [void](Invoke-WorkspaceCargo $context 'clippy' @('clippy', '--workspace', '--all-targets', '--release', '--locked', '--features', 'ultratasks-core/test-support,ultratasks-core/fixtures', '--target', $context.Triple) @('--', '-D', 'warnings'))
            [void](Invoke-WorkspaceCargo $context 'all-tests' @('test', '--workspace', '--all-targets', '--release', '--locked', '--features', 'ultratasks-core/test-support,ultratasks-core/fixtures', '--target', $context.Triple))
            $build = Invoke-WorkspaceCargo $context 'product-build' @('build', '-p', 'ultratasks-desktop', '--release', '--locked', '--target', $context.Triple)
            [void](Invoke-WorkspaceCargo $context 'measure-build' @('build', '-p', 'ultratasks-core', '--example', 'measure', '--features', 'fixtures', '--release', '--locked', '--target', $context.Triple))
            $metadata = Invoke-WorkspaceCargo $context 'dependency-metadata' @('metadata', '--locked', '--format-version', '1', '--filter-platform', $context.Triple)
            [void](Invoke-WorkspaceCargo $context 'dependency-features' @('tree', '--locked', '-e', 'features', '--target', $context.Triple))
            if ($metadata.exit_code -eq 0) {
                [void](Invoke-LoggedStep $context 'dependency-check' (Get-Command python.exe -ErrorAction Stop).Source @('-I', (Join-Path $context.Root 'scripts/check-dependencies.py'), $metadata.stdout))
            } else {
                Add-SkippedStep $context 'dependency-check' 'cargo metadata failed'
            }
            if ($GuiMode -eq 'Skip') {
                Add-SkippedStep $context 'native-smoke' 'GuiMode Skip: native window and frame callback NOT VERIFIED'
            } elseif ($build.exit_code -ne 0) {
                Add-SkippedStep $context 'native-smoke' 'product build failed; stale binary is not used'
            } else {
                $binary = Join-Path $context.Target "$($context.Triple)/release/ultratasks-desktop.exe"
                $smokeDir = Join-Path $context.Evidence 'smoke'
                [void][System.IO.Directory]::CreateDirectory($smokeDir)
                $smoke = Invoke-LoggedStep $context 'native-smoke' $binary @('--db', (Join-Path $smokeDir 'smoke.sqlite'), '--demo', 'small', '--seed', '42', '--date', '2026-11-11', '--zone', 'Asia/Tokyo', '--diagnostics-dir', $smokeDir, '--smoke-test') -TimeoutSeconds 30
                if ($smoke.exit_code -eq 0) {
                    $output = [System.IO.File]::ReadAllText($smoke.stdout) + [System.IO.File]::ReadAllText($smoke.stderr)
                    if ($output -notmatch '(?m)^SMOKE_OK:') {
                        $smoke.status = 'FAILED'
                        $smoke.exit_code = 1
                        if ($context.ExitCode -eq 0) { $context.ExitCode = 1 }
                        Write-Utf8 (Join-Path $smokeDir 'missing-marker.log') 'Process exited 0 but did not report SMOKE_OK.'
                    }
                }
            }
        }
        if ($Mode -eq 'full' -and $context.ExitCode -eq 0) {
            $revision = Get-WorkspaceRevision $context
            $changes = & (Get-Command git.exe -ErrorAction Stop).Source -C $context.Root status --porcelain --untracked-files=no
            if ($LASTEXITCODE -ne 0 -or $changes) { throw 'Verified artifacts require a clean tracked checkout.' }
            $files = foreach ($relative in @('ultratasks-desktop.exe', 'examples/measure.exe')) {
                $file = Get-Item -LiteralPath (Join-Path $release $relative) -ErrorAction Stop
                [ordered]@{ name = $file.Name; bytes = $file.Length; sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash }
            }
            $manifest = [ordered]@{
                schema_version = 1; source_sha = $revision; repository = $env:GITHUB_REPOSITORY
                run_id = $env:GITHUB_RUN_ID; run_attempt = $env:GITHUB_RUN_ATTEMPT
                toolchain = $context.Toolchain; target_triple = $context.Triple; profile = 'release'
                lock_sha256 = (Get-FileHash -LiteralPath (Join-Path $context.Root 'Cargo.lock') -Algorithm SHA256).Hash
                verification = 'full-passed'; native_smoke = $(if ($GuiMode -eq 'Skip') { 'NOT_RUN' } else { 'PASSED' })
                files = @($files); verified_utc = [DateTime]::UtcNow.ToString('o')
            }
            Write-Utf8 $buildManifest ($manifest | ConvertTo-Json -Depth 8)
            Write-Utf8 (Join-Path $context.Evidence 'verified-build.json') ($manifest | ConvertTo-Json -Depth 8)
        }
    }
} catch {
    if ($null -eq $context) { Write-Error $_; exit 1 }
    Save-RunError $context $_
} finally {
    if ($null -ne $context) { Save-RunResult $context }
}
exit $context.ExitCode
