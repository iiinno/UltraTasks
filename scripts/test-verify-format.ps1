[CmdletBinding()]
param([string]$EvidenceDir)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:RUNNER_OS -ne 'Windows') {
    throw 'Run this regression only on a GitHub-hosted Windows runner; do not set runner variables on the development PC.'
}
. (Join-Path $PSScriptRoot 'windows-env.ps1') -EvidenceDir $EvidenceDir
Assert-RemoteExecution

function Assert-FormatResult {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}

# Only the Cargo boundary is synthetic. The remote guard, process logging, Git
# status/diff, UTF-8 patch copy, provenance and first-error handling are real.
function Invoke-WorkspaceCargo {
    param($Context, [string]$Name, [string[]]$Arguments, [string[]]$Tail = @())
    Assert-RemoteExecution
    if ($Name -eq 'fmt' -and $Tail -contains '--check') {
        $code = $script:Scenario.Check
    } elseif ($Name -eq 'fmt-generate') {
        if ($script:Scenario.Edit) {
            Write-Utf8 $script:SourceFile ([IO.File]::ReadAllText($script:SourceFile) + "`n// $script:Japanese fmt regression`n")
        }
        if ($script:Scenario.Name -eq 'invalid-utf8') {
            [IO.File]::WriteAllBytes($script:SourceFile, ([IO.File]::ReadAllBytes($script:SourceFile) + [byte[]]@(0xff, 0x0a)))
        }
        if ($script:Scenario.Name -eq 'diff-failure') {
            & $git -C $Context.Root config --local diff.algorithm invalid-for-fmt-regression
            if ($LASTEXITCODE -ne 0) { throw 'Cannot configure the isolated Git diff failure.' }
        }
        $code = $script:Scenario.Format
    } else {
        throw "Unexpected Cargo operation in format regression: $Name"
    }
    return Invoke-LoggedStep $Context $Name $script:PowerShellExe @('-NoProfile', '-NonInteractive', '-Command', "exit $code")
}

$workspace = Split-Path $PSScriptRoot -Parent
$sourceSha = Get-WorkspaceRevision ([pscustomobject]@{ Root = $workspace })
$git = (Get-Command git.exe -ErrorAction Stop).Source
$script:PowerShellExe = (Get-Process -Id $PID).Path
$script:Japanese = -join ([char[]]@(0x65e5, 0x672c, 0x8a9e))
$testRoot = Join-Path $env:RUNNER_TEMP ('ultratasks-fmt-test-' + [guid]::NewGuid().ToString('N'))
$root = Join-Path $testRoot 'checkout'
if (-not $EvidenceDir) { $EvidenceDir = Join-Path $env:RUNNER_TEMP ('ultratasks-fmt-evidence-' + [guid]::NewGuid().ToString('N')) }
[void][IO.Directory]::CreateDirectory($testRoot)
$relativeSource = 'crates/ultratasks-core/src/lib.rs'
$script:SourceFile = Join-Path $root $relativeSource
$cases = @(
    @{ Name = 'formatted-full'; Mode = 'full'; Check = 0; Format = 0; Edit = $false; Dirty = $false; Expected = 'PASSED' },
    @{ Name = 'formatted-fast'; Mode = 'fast'; Check = 0; Format = 0; Edit = $false; Dirty = $false; Expected = 'PASSED' },
    @{ Name = 'patch-full'; Mode = 'full'; Check = 7; Format = 0; Edit = $true; Dirty = $false; Expected = 'GENERATED_UNVERIFIED' },
    @{ Name = 'patch-fast'; Mode = 'fast'; Check = 7; Format = 0; Edit = $true; Dirty = $false; Expected = 'GENERATED_UNVERIFIED' },
    @{ Name = 'formatter-failure'; Mode = 'full'; Check = 7; Format = 9; Edit = $true; Dirty = $false; Expected = 'FAILED' },
    @{ Name = 'empty-diff'; Mode = 'full'; Check = 7; Format = 0; Edit = $false; Dirty = $false; Expected = 'FAILED' },
    @{ Name = 'dirty-source'; Mode = 'full'; Check = 7; Format = 0; Edit = $false; Dirty = $true; Expected = 'FAILED' },
    @{ Name = 'diff-failure'; Mode = 'full'; Check = 7; Format = 0; Edit = $true; Dirty = $false; Expected = 'FAILED' },
    @{ Name = 'invalid-utf8'; Mode = 'full'; Check = 7; Format = 0; Edit = $true; Dirty = $false; Expected = 'FAILED' }
)
try {
    # A disposable clone keeps the actual checkout SHA without changing any CI
    # identity variables, committing fixtures or modifying the source checkout.
    & $git clone --shared --no-checkout --quiet -- $workspace $root
    if ($LASTEXITCODE -ne 0) { throw 'Cannot clone the regression checkout.' }
    & $git -C $root -c core.autocrlf=false checkout --quiet --detach $sourceSha
    if ($LASTEXITCODE -ne 0) { throw 'Cannot check out the source SHA.' }

    foreach ($case in $cases) {
        $script:Scenario = $case
        & $git -C $root -c core.autocrlf=false restore --source=$sourceSha --worktree -- $relativeSource
        if ($LASTEXITCODE -ne 0) { throw 'Cannot restore the isolated source file.' }
        if ($case.Dirty) { Write-Utf8 $script:SourceFile ([IO.File]::ReadAllText($script:SourceFile) + "`n// pre-existing edit`n") }
        $context = New-WorkspaceContext $root (Join-Path $testRoot 'target') $EvidenceDir $case.Name
        $allowed = Invoke-VerificationFormat $context $case.Mode
        Save-RunResult $context
        if ($case.Name -eq 'diff-failure') {
            & $git -C $root config --local --unset diff.algorithm
            if ($LASTEXITCODE -ne 0) { throw 'Cannot reset the isolated Git diff configuration.' }
        }
        Assert-FormatResult ($allowed -is [bool] -and $allowed -eq ($case.Check -eq 0)) "$($case.Name): format gate decision"
        Assert-FormatResult ($context.ExitCode -eq $case.Check) "$($case.Name): first exit code was not preserved"
        $summary = [IO.File]::ReadAllText((Join-Path $context.Evidence 'summary.json')) | ConvertFrom-Json
        Assert-FormatResult ($summary.exit_code -eq $case.Check) "$($case.Name): saved exit code"
        $feedbackPath = Join-Path $context.Evidence 'fmt-feedback.json'
        $patchPath = Join-Path $context.Evidence 'fmt.patch'
        if ($allowed) {
            Assert-FormatResult ($context.Steps.Count -eq 1 -and $context.Steps[0].status -eq 'PASSED') "$($case.Name): successful check ran feedback steps"
            Assert-FormatResult (-not (Test-Path -LiteralPath $feedbackPath)) "$($case.Name): unexpected feedback"
            Assert-FormatResult (-not (Test-Path -LiteralPath $patchPath)) "$($case.Name): unexpected patch"
            continue
        }

        Assert-FormatResult ($context.Steps[0].name -eq 'fmt' -and $context.Steps[0].status -eq 'FAILED') "$($case.Name): original check must stay failed"
        $remaining = if ($case.Mode -eq 'fast') { @('core-tests', 'native-smoke') } else {
            @('clippy', 'all-tests', 'product-build', 'measure-build', 'dependency-metadata', 'dependency-features', 'dependency-check', 'native-smoke', 'verified-build')
        }
        foreach ($name in $remaining) {
            $steps = @($summary.steps | Where-Object { $_.name -eq $name })
            Assert-FormatResult ($steps.Count -eq 1 -and $steps[0].status -eq 'NOT_RUN' -and $steps[0].reason -match 'fmt --check failed') "$($case.Name): $name must be NOT_RUN with a reason"
        }
        $feedback = [IO.File]::ReadAllText($feedbackPath) | ConvertFrom-Json
        Assert-FormatResult ($feedback.status -eq $case.Expected -and $feedback.source_sha -ceq $sourceSha) "$($case.Name): feedback status/source SHA"
        Assert-FormatResult ($feedback.review_only -eq $true -and $feedback.release_eligible -eq $false -and $feedback.verification -eq 'FAILED') "$($case.Name): patch was certified as a build"
        Assert-FormatResult ($feedback.check_exit_code -eq $case.Check) "$($case.Name): feedback lost the original check failure"
        if ($case.Dirty) {
            Assert-FormatResult (-not $feedback.source_clean -and $null -eq $feedback.format_exit_code) "$($case.Name): dirty checkout was formatted"
        } else {
            Assert-FormatResult ($feedback.source_clean -and $feedback.format_exit_code -eq $case.Format) "$($case.Name): formatter result was lost"
        }
        if ($case.Expected -eq 'GENERATED_UNVERIFIED') {
            $bytes = [IO.File]::ReadAllBytes($patchPath)
            $text = (New-Object System.Text.UTF8Encoding($false, $true)).GetString($bytes)
            Assert-FormatResult ($text.Contains($script:Japanese)) "$($case.Name): Japanese UTF-8 was corrupted"
            Assert-FormatResult ($feedback.patch.name -eq 'fmt.patch' -and $feedback.patch.bytes -eq $bytes.Length) "$($case.Name): patch identity/size"
            Assert-FormatResult ($feedback.patch.sha256 -ceq (Get-FileHash -LiteralPath $patchPath -Algorithm SHA256).Hash) "$($case.Name): patch hash mismatch"
            $diff = @($context.Steps | Where-Object { $_.name -eq 'fmt-diff' })[0]
            Assert-FormatResult ([Convert]::ToBase64String($bytes) -ceq [Convert]::ToBase64String([IO.File]::ReadAllBytes($diff.stdout))) "$($case.Name): diff bytes changed during copy"
            & $git -C $root -c core.autocrlf=false restore --source=$sourceSha --worktree -- $relativeSource
            if ($LASTEXITCODE -ne 0) { throw 'Cannot restore the patch base.' }
            & $git -C $root apply --check -- $patchPath
            Assert-FormatResult ($LASTEXITCODE -eq 0) "$($case.Name): patch does not apply to its source SHA"
        } else {
            Assert-FormatResult ($null -eq $feedback.patch -and -not [string]::IsNullOrWhiteSpace($feedback.error)) "$($case.Name): feedback generation failure was not recorded"
            Assert-FormatResult (-not (Test-Path -LiteralPath $patchPath)) "$($case.Name): published a failed patch"
            if ($case.Name -in @('diff-failure', 'invalid-utf8')) {
                $diff = @($context.Steps | Where-Object { $_.name -eq 'fmt-diff' })[0]
                Assert-FormatResult ($feedback.diff_exit_code -eq $diff.exit_code) "$($case.Name): diff exit code was lost"
                if ($case.Name -eq 'diff-failure') {
                    Assert-FormatResult ($diff.status -eq 'FAILED' -and $diff.exit_code -ne 0 -and $feedback.error -match 'Cannot collect the formatting patch') "$($case.Name): nonzero Git diff was not rejected"
                } else {
                    $bytes = [IO.File]::ReadAllBytes($diff.stdout)
                    Assert-FormatResult ($diff.status -eq 'PASSED' -and $diff.exit_code -eq 0 -and $bytes -contains [byte]0xff) "$($case.Name): invalid UTF-8 did not reach the decoder"
                }
            }
        }
    }
    Write-Host "Format gate: $($cases.Count) synthetic cases passed. No Rust/Cargo, builds, fixtures, applications, commits or pushes were run."
} finally {
    Remove-Item -LiteralPath $testRoot -Recurse -Force
}
exit 0
