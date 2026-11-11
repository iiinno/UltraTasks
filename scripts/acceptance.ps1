[CmdletBinding()]
param(
    [ValidateSet('check', 'test')][string]$Mode = 'test',
    [string]$WorkspaceRoot,
    [string]$TargetDir,
    [string]$EvidenceDir
)

. (Join-Path $PSScriptRoot 'windows-env.ps1') -WorkspaceRoot $WorkspaceRoot -TargetDir $TargetDir -EvidenceDir $EvidenceDir
$context = $null
try {
    $context = New-WorkspaceContext $WorkspaceRoot $TargetDir $EvidenceDir "acceptance-$Mode"
    if (Initialize-WindowsEnvironment $context) {
        $testRoot = Join-Path $context.Root 'crates/ultratasks-core/tests'
        $files = @('acceptance.rs', 'calendar_cases.rs', 'history_cases.rs', 'query_cases.rs', 'common/mod.rs') | ForEach-Object { Join-Path $testRoot $_ }
        [void](Invoke-LoggedStep $context 'acceptance-format' (Get-Command rustfmt.exe -ErrorAction Stop).Source (@(('+' + $context.Toolchain), '--edition', '2024', '--config', 'skip_children=true', '--check') + $files))
        $arguments = @($Mode, '-p', 'ultratasks-core', '--release', '--locked', '--features', 'test-support,fixtures', '--target', $context.Triple)
        if ($Mode -eq 'check') {
            $arguments += '--tests'
            [void](Invoke-WorkspaceCargo $context 'acceptance-typecheck' $arguments)
            Add-SkippedStep $context 'acceptance-tests' 'check mode compiles tests; test cases NOT RUN'
        } else {
            foreach ($name in @('acceptance', 'calendar_cases', 'history_cases', 'query_cases')) {
                $arguments += @('--test', $name)
            }
            [void](Invoke-WorkspaceCargo $context 'acceptance-tests' $arguments)
        }
    }
} catch {
    if ($null -eq $context) { Write-Error $_; exit 1 }
    Save-RunError $context $_
} finally {
    if ($null -ne $context) { Save-RunResult $context }
}
exit $context.ExitCode
