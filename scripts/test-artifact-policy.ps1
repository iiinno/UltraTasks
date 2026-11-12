[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'artifact-policy.ps1')

$trustedJson = @'
{
  "repository": {"full_name": "iiinno/UltraTasks"},
  "head_repository": {"full_name": "iiinno/UltraTasks"},
  "event": "push",
  "head_branch": "main",
  "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "path": ".github/workflows/windows.yml",
  "status": "completed",
  "conclusion": "success"
}
'@
$sha = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
$otherSha = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
$script:Checked = 0

function Assert-ArtifactPolicy {
    param([string]$Name, [AllowNull()][object]$Run, [AllowNull()][AllowEmptyString()][string]$ExpectedSha, [bool]$Allowed)
    $actual = Test-TrustedWindowsArtifactRun -Run $Run -ExpectedSha $ExpectedSha
    if ($actual -isnot [bool] -or $actual -ne $Allowed) { throw "Artifact policy failed: $Name (expected $Allowed, got $actual)" }
    $script:Checked++
}

foreach ($event in @('push', 'workflow_dispatch')) {
    foreach ($branch in @('main', 'prototype/pc-first')) {
        $run = $trustedJson | ConvertFrom-Json
        $run.event = $event
        $run.head_branch = $branch
        Assert-ArtifactPolicy "$event on $branch" $run $sha $true
    }
}
Assert-ArtifactPolicy 'uppercase expected SHA' ($trustedJson | ConvertFrom-Json) $sha.ToUpperInvariant() $true

$rejected = @(
    @{ Name = 'other repository'; Field = 'repository'; Value = [pscustomobject]@{ full_name = 'attacker/UltraTasks' } },
    @{ Name = 'fork head repository'; Field = 'head_repository'; Value = [pscustomobject]@{ full_name = 'attacker/UltraTasks' } },
    @{ Name = 'same-repository PR'; Field = 'event'; Value = 'pull_request' },
    @{ Name = 'PR target'; Field = 'event'; Value = 'pull_request_target' },
    @{ Name = 'other event'; Field = 'event'; Value = 'workflow_run' },
    @{ Name = 'other branch'; Field = 'head_branch'; Value = 'feature/untrusted' },
    @{ Name = 'case-distinct branch'; Field = 'head_branch'; Value = 'MAIN' },
    @{ Name = 'API SHA mismatch'; Field = 'head_sha'; Value = $otherSha },
    @{ Name = 'malformed API SHA'; Field = 'head_sha'; Value = 'not-a-sha' },
    @{ Name = 'array API SHA'; Field = 'head_sha'; Value = @($sha, $otherSha) },
    @{ Name = 'array event'; Field = 'event'; Value = @('push', 'pull_request') },
    @{ Name = 'unsuccessful run'; Field = 'conclusion'; Value = 'failure' },
    @{ Name = 'cancelled run'; Field = 'conclusion'; Value = 'cancelled' },
    @{ Name = 'incomplete run'; Field = 'status'; Value = 'in_progress' },
    @{ Name = 'other workflow'; Field = 'path'; Value = '.github/workflows/ci.yml' },
    @{ Name = 'missing conclusion'; Field = 'conclusion'; Value = $null },
    @{ Name = 'missing head repository name'; Field = 'head_repository'; Value = [pscustomobject]@{} }
)
foreach ($case in $rejected) {
    $run = $trustedJson | ConvertFrom-Json
    $run.($case.Field) = $case.Value
    Assert-ArtifactPolicy $case.Name $run $sha $false
}

$forged = $trustedJson | ConvertFrom-Json
$forged.event = 'pull_request'
$forged.head_repository.full_name = 'attacker/UltraTasks'
$forged.head_sha = $otherSha
$forged | Add-Member -NotePropertyName artifact_name -NotePropertyValue "windows-release-$sha-1"
$forged | Add-Member -NotePropertyName manifest -NotePropertyValue ([pscustomobject]@{ source_sha = $sha; verification = 'full-passed' })
Assert-ArtifactPolicy 'fork PR claiming a trusted artifact and manifest SHA' $forged $sha $false
$forged.event = 'push'
$forged.head_repository.full_name = 'iiinno/UltraTasks'
Assert-ArtifactPolicy 'artifact SHA cannot override API SHA' $forged $sha $false

foreach ($field in @('repository', 'head_repository', 'event', 'head_branch', 'head_sha', 'path', 'status', 'conclusion')) {
    $run = $trustedJson | ConvertFrom-Json
    $run.PSObject.Properties.Remove($field)
    Assert-ArtifactPolicy "missing API field $field" $run $sha $false
}
Assert-ArtifactPolicy 'missing run' $null $sha $false
Assert-ArtifactPolicy 'different expected SHA' ($trustedJson | ConvertFrom-Json) $otherSha $false
Assert-ArtifactPolicy 'empty expected SHA' ($trustedJson | ConvertFrom-Json) '' $false
Assert-ArtifactPolicy 'malformed expected SHA' ($trustedJson | ConvertFrom-Json) 'claimed-by-artifact' $false

Write-Output "Artifact policy: $script:Checked synthetic JSON cases passed. No network, downloads, builds, fixtures or applications were run."
