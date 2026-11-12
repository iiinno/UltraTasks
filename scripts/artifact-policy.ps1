function Test-TrustedWindowsArtifactRun {
    [CmdletBinding()]
    [OutputType([bool])]
    param(
        [AllowNull()][object]$Run,
        [AllowNull()][AllowEmptyString()][string]$ExpectedSha
    )

    if ($null -eq $Run -or $Run -isnot [pscustomobject] -or $ExpectedSha -cnotmatch '^[0-9a-fA-F]{40}$') { return $false }
    foreach ($name in @('event', 'head_branch', 'head_sha', 'path', 'status', 'conclusion')) {
        $property = $Run.PSObject.Properties[$name]
        if ($null -eq $property -or $property.Value -isnot [string]) { return $false }
    }
    foreach ($name in @('repository', 'head_repository')) {
        $property = $Run.PSObject.Properties[$name]
        if ($null -eq $property -or $property.Value -isnot [pscustomobject]) { return $false }
        $fullName = $property.Value.PSObject.Properties['full_name']
        if ($null -eq $fullName -or $fullName.Value -isnot [string] -or $fullName.Value -cne 'iiinno/UltraTasks') { return $false }
    }
    return (
        $Run.event -cin @('push', 'workflow_dispatch') -and
        $Run.head_branch -cin @('main', 'prototype/pc-first') -and
        $Run.head_sha -cmatch '^[0-9a-f]{40}$' -and
        $Run.head_sha -ceq $ExpectedSha.ToLowerInvariant() -and
        $Run.path -ceq '.github/workflows/windows.yml' -and
        $Run.status -ceq 'completed' -and
        $Run.conclusion -ceq 'success'
    )
}
