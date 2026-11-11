[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidatePattern('^[1-9][0-9]*$')][string]$RunId,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-fA-F]{40}$')][string]$ExpectedSha
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'artifact-policy.ps1')
$repository = 'github.com/iiinno/UltraTasks'
$previousHost = $env:GH_HOST
$previousRepo = $env:GH_REPO
$previousPrompt = $env:GH_PROMPT_DISABLED
$exitCode = 0
try {
    $env:GH_HOST = 'github.com'
    $env:GH_REPO = $repository
    $env:GH_PROMPT_DISABLED = '1'
    $gh = (Get-Command gh.exe -ErrorAction Stop).Source
    & $gh --version
    if ($LASTEXITCODE -ne 0) { throw 'Install the official GitHub CLI from https://cli.github.com/.' }
    & $gh auth status --hostname github.com
    if ($LASTEXITCODE -ne 0) { throw 'GitHub CLI is not authenticated for github.com. Complete gh auth login --hostname github.com --git-protocol https --web, then retry. Never paste credentials into chat.' }
    $account = & $gh api --hostname github.com user --jq .login
    if ($LASTEXITCODE -ne 0 -or -not $account) { throw 'Cannot verify the active GitHub identity.' }
    Write-Host "GitHub: github.com / $account / iiinno/UltraTasks (read-only download)"
    if ($env:GH_TOKEN -or $env:GITHUB_TOKEN) { Write-Host 'An environment token may override stored GitHub CLI credentials; its value is not displayed.' }
    $runText = & $gh api --hostname github.com "repos/iiinno/UltraTasks/actions/runs/$RunId"
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read the selected run. Private repository access and Actions read permission are required.' }
    $run = ($runText -join "`n") | ConvertFrom-Json
    if (-not (Test-TrustedWindowsArtifactRun -Run $run -ExpectedSha $ExpectedSha)) {
        throw 'Untrusted run: API repository and head_repository must both be iiinno/UltraTasks, event must be push or workflow_dispatch, branch must be main or prototype/pc-first, and API head_sha must match ExpectedSha. The Windows workflow must be completed successfully. PR artifacts are never accepted.'
    }
    $ExpectedSha = $run.head_sha
    Write-Host "Verified API source: $($run.head_repository.full_name) / $($run.head_branch) / $($run.event) / $ExpectedSha"
    $artifact = "windows-release-$ExpectedSha-$($run.run_attempt)"
    $local = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
    if (-not $local) { throw 'LocalApplicationData is unavailable.' }
    $root = Join-Path $local 'UltraTasks/artifacts'
    $destination = Join-Path $root ("run-$RunId-$($run.run_attempt)-$ExpectedSha-" + [guid]::NewGuid().ToString('N'))
    if (Test-Path -LiteralPath $destination) { throw 'Download destination must be new and empty.' }
    [void][System.IO.Directory]::CreateDirectory($destination)
    & $gh run download $RunId --repo $repository --name $artifact --dir $destination
    if ($LASTEXITCODE -ne 0) { throw "Artifact download failed or expired: $artifact. Existing selection was not changed." }
    $allowed = @('ultratasks-desktop.exe', 'measure.exe', 'verified-build.json', 'THIRD_PARTY_NOTICES.md', 'package-manifest.json', 'Start-UltraTasks.cmd', 'scripts/start-demo.ps1', 'USAGE.txt', 'third_party/radix-colors/LICENSE', 'third_party/lucide/LICENSE', 'design/upstream-lock.json')
    $licenseNames = @('LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE', 'LICENSE.txt', 'UNLICENSE', 'COPYING', 'README.md', 'license-mit', 'license-apache-2.0', 'GPUI-KIT-APACHE-2.0.txt', 'LUCIDE-ISC-MIT.txt', 'GPUI-APACHE-2.0.txt', 'SQLITE-BLESSING.txt')
    $items = @(Get-ChildItem -LiteralPath $destination -Force -Recurse)
    if (@($items | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }).Count -ne 0) { throw 'Artifact must not contain links.' }
    foreach ($relative in $allowed) {
        if (-not (Test-Path -LiteralPath (Join-Path $destination $relative) -PathType Leaf)) { throw "Missing packaged file: $relative" }
    }
    $package = [IO.File]::ReadAllText((Join-Path $destination 'package-manifest.json')) | ConvertFrom-Json
    $files = @($items | Where-Object { -not $_.PSIsContainer })
    if ($package.source_sha -ne $ExpectedSha -or $package.run_id -ne $RunId -or $package.run_attempt -ne $run.run_attempt -or @($package.files).Count -ne ($files.Count - 1)) {
        throw 'Package manifest does not match the selected run or file count.'
    }
    foreach ($file in $files) {
        $relative = $file.FullName.Substring($destination.Length + 1).Replace('\', '/')
        if ($relative -notin $allowed -and -not ($relative.StartsWith('licenses/', [StringComparison]::Ordinal) -and $file.Name -in $licenseNames)) { throw "Unexpected packaged file: $relative" }
        if ($relative -eq 'package-manifest.json') { continue }
        $entry = @($package.files | Where-Object { $_.path -ceq $relative })
        if ($entry.Count -ne 1 -or $file.Length -ne $entry[0].bytes -or (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash -ne $entry[0].sha256) { throw "Package hash mismatch: $relative" }
    }
    $manifestPath = Join-Path $destination 'verified-build.json'
    $manifest = [System.IO.File]::ReadAllText($manifestPath) | ConvertFrom-Json
    if ($manifest.schema_version -ne 1 -or $manifest.source_sha -ne $ExpectedSha -or $manifest.repository -ne 'iiinno/UltraTasks' -or
        $manifest.run_id -ne $RunId -or $manifest.run_attempt -ne $run.run_attempt -or $manifest.verification -ne 'full-passed' -or
        $manifest.toolchain -ne '1.99.1-x86_64-pc-windows-msvc' -or $manifest.target_triple -ne 'x86_64-pc-windows-msvc' -or $manifest.profile -ne 'release') {
        throw 'Artifact provenance does not match the requested verified run and checkout SHA.'
    }
    foreach ($name in @('ultratasks-desktop.exe', 'measure.exe')) {
        $entry = @($manifest.files | Where-Object { $_.name -eq $name })
        $file = Get-Item -LiteralPath (Join-Path $destination $name)
        if ($entry.Count -ne 1 -or $file.Length -ne $entry[0].bytes -or (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash -ne $entry[0].sha256) {
            throw "Downloaded executable failed its hash/size check: $name"
        }
    }
    $binary = Join-Path $destination 'ultratasks-desktop.exe'
    $selection = [ordered]@{ binary = $binary; manifest = $manifestPath; source_sha = $ExpectedSha; run_id = $RunId; run_attempt = $run.run_attempt }
    $temporaryPointer = Join-Path $root ('selection-' + [guid]::NewGuid().ToString('N') + '.json')
    [IO.File]::WriteAllText($temporaryPointer, ($selection | ConvertTo-Json -Depth 5), (New-Object System.Text.UTF8Encoding($false)))
    Move-Item -LiteralPath $temporaryPointer -Destination (Join-Path $root 'current.json') -Force
    Write-Host "Downloaded: $binary"
    Write-Host "Checkout SHA: $ExpectedSha; native smoke: $($manifest.native_smoke)"
    Write-Host 'No executable was launched. Use Start-UltraTasks.cmd when ready. Performance measurements belong to the remote runner, not this PC.'
} catch {
    Write-Error $_ -ErrorAction Continue
    $exitCode = 1
} finally {
    $env:GH_HOST = $previousHost
    $env:GH_REPO = $previousRepo
    $env:GH_PROMPT_DISABLED = $previousPrompt
}
exit $exitCode
