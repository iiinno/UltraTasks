[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$DbPath,
    [ValidateSet('none', 'small', '1000', '10000')][string]$Demo = 'none',
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
    $context = New-WorkspaceContext $WorkspaceRoot $TargetDir $EvidenceDir 'dev'
    if (-not [System.IO.Path]::IsPathRooted($DbPath)) { throw 'DbPath must be absolute.' }
    if ($Demo -ne 'none' -and (Test-Path -LiteralPath $DbPath)) {
        $item = Get-Item -LiteralPath $DbPath
        if ($item.PSIsContainer -or $item.Length -ne 0 -or ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
            throw 'Refusing to seed an existing nonempty database or non-file. Use -Demo none to reopen it.'
        }
    }
    if (Initialize-WindowsEnvironment $context) {
        $build = Invoke-WorkspaceCargo $context 'product-build' @('build', '-p', 'ultratasks-desktop', '--release', '--locked', '--target', $context.Triple)
        if ($build.exit_code -eq 0) {
            $binary = Join-Path $context.Target "$($context.Triple)/release/ultratasks-desktop.exe"
            $arguments = @('--db', (Get-AbsolutePath $DbPath), '--demo', $Demo, '--seed', $Seed.ToString(), '--zone', $Zone, '--diagnostics-dir', (Join-Path $context.Evidence 'diagnostics'))
            if ($Date) { $arguments += @('--date', $Date) }
            [void](Invoke-LoggedStep $context 'desktop' $binary $arguments)
        } else {
            Add-SkippedStep $context 'desktop' 'product build failed; stale binary is not used'
        }
    }
} catch {
    if ($null -eq $context) { Write-Error $_; exit 1 }
    Save-RunError $context $_
} finally {
    if ($null -ne $context) { Save-RunResult $context }
}
exit $context.ExitCode
