[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$DbPath,
    [ValidateSet('small', '1000', '10000')][string]$Profile = 'small',
    [ValidateSet('spread', 'dense')][string]$Distribution = 'spread',
    [UInt64]$Seed = 42,
    [string]$Date = '2026-11-11',
    [string]$Zone = 'Asia/Tokyo',
    [string]$WorkspaceRoot,
    [string]$TargetDir,
    [string]$EvidenceDir
)

. (Join-Path $PSScriptRoot 'windows-env.ps1') -WorkspaceRoot $WorkspaceRoot -TargetDir $TargetDir -EvidenceDir $EvidenceDir
$context = $null
try {
    $context = New-WorkspaceContext $WorkspaceRoot $TargetDir $EvidenceDir 'fixture'
    if (-not [System.IO.Path]::IsPathRooted($DbPath)) { throw 'DbPath must be absolute.' }
    if (Test-Path -LiteralPath $DbPath) {
        $item = Get-Item -LiteralPath $DbPath
        if ($item.PSIsContainer -or $item.Length -ne 0 -or ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
            throw 'Refusing to seed an existing nonempty database or non-file. Choose a fresh fixture path.'
        }
    }
    if (Initialize-WindowsEnvironment $context) {
        [void](Invoke-WorkspaceCargo $context 'seed' @('run', '-p', 'ultratasks-core', '--example', 'seed', '--features', 'fixtures', '--release', '--locked', '--target', $context.Triple) @('--', '--db', (Get-AbsolutePath $DbPath), '--profile', $Profile, '--distribution', $Distribution, '--seed', $Seed.ToString(), '--date', $Date, '--zone', $Zone))
    }
} catch {
    if ($null -eq $context) { Write-Error $_; exit 1 }
    Save-RunError $context $_
} finally {
    if ($null -ne $context) { Save-RunResult $context }
}
exit $context.ExitCode
