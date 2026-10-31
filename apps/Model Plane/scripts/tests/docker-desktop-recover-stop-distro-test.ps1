#Requires -Version 7.0
# Mock-only check of Stop-Distro in scripts/dev/docker-desktop-recover.ps1.
#
# The script is parsed, never executed: the only function taken from it is Stop-Distro, and its
# two external dependencies are replaced by stubs, so no wsl.exe, docker.exe or process is
# started. It pins the behaviour the recovery runbook describes:
#   - the terminate is skipped only for a Stopped distro or a missing wsl.exe;
#   - a failed or timed-out terminate is fatal only for a distro seen Running;
#   - for an unknown state (the wedged-WSL case) it is a warning, so the folder rename that
#     follows still runs.
#
# Usage: pwsh -NoProfile -File "apps/Model Plane/scripts/tests/docker-desktop-recover-stop-distro-test.ps1"
param([string]$ScriptPath = (Join-Path $PSScriptRoot '../dev/docker-desktop-recover.ps1'))

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$tokens = $null
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile(
    (Resolve-Path -LiteralPath $ScriptPath).Path, [ref]$tokens, [ref]$parseErrors)
"parse errors in ${ScriptPath}: $($parseErrors.Count)"
if ($parseErrors.Count -gt 0) { exit 1 }

$fnAst = $ast.FindAll({
        param($node)
        $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Stop-Distro'
    }, $true) | Select-Object -First 1
if (-not $fnAst) { 'Stop-Distro not found'; exit 1 }

$Distro = 'docker-desktop'
$wsl = 'C:\mock\wsl.exe'
$script:state = 'Running'
$script:reply = $null
$script:calls = 0

function Get-DistroState { return $script:state }
function Invoke-Bounded {
    param([string]$FilePath, [string[]]$Arguments, [int]$TimeoutSeconds)
    $script:calls++
    return $script:reply
}

. ([scriptblock]::Create($fnAst.Extent.Text))

function New-Reply([bool]$TimedOut, $ExitCode, [string]$Stderr) {
    [pscustomobject]@{ TimedOut = $TimedOut; ExitCode = $ExitCode; Stdout = ''; Stderr = $Stderr }
}

$ok = New-Reply $false 0 ''
$failed = New-Reply $false 1 'boom'
$noDistro = New-Reply $false 1 'There is no distribution with the supplied name.'
$timedOut = New-Reply $true $null ''

# name, state, reply, expected terminate calls, expected outcome ('none' or a message fragment)
$cases = @(
    @('running, terminate ok', 'Running', $ok, 1, 'none'),
    @('running, terminate fails', 'Running', $failed, 1, 'failed: boom'),
    @('running, terminate times out', 'Running', $timedOut, 1, 'timed out'),
    @('stopped, nothing to do', 'Stopped', $null, 0, 'none'),
    @('wsl.exe absent, nothing to do', 'wsl.exe not found', $null, 0, 'none'),
    @('unknown, terminate ok', 'unknown (wsl -l -v failed)', $ok, 1, 'none'),
    @('unknown, terminate fails: warn, carry on to the rename', 'unknown (wsl -l -v failed)', $failed, 1, 'none'),
    @('unknown, terminate times out: warn, carry on to the rename', 'unknown (wsl -l -v failed)', $timedOut, 1, 'none'),
    @('absent, terminate ok', 'absent', $ok, 1, 'none'),
    @('absent, no such distro is expected', 'absent', $noDistro, 1, 'none')
)

$passed = 0
$failedCases = 0
foreach ($case in $cases) {
    $name, $state, $reply, $wantCalls, $wantOutcome = $case
    $script:state = $state
    $script:reply = $reply
    $script:calls = 0
    $outcome = 'none'
    try {
        Stop-Distro *>$null
    }
    catch {
        $outcome = $_.Exception.Message
    }
    $callsOk = ($script:calls -eq $wantCalls)
    $outcomeOk = if ($wantOutcome -eq 'none') { $outcome -eq 'none' } else { $outcome -like "*$wantOutcome*" }
    if ($callsOk -and $outcomeOk) {
        $passed++
        "PASS  $name (calls=$($script:calls))"
    }
    else {
        $failedCases++
        "FAIL  $name (calls=$($script:calls), want $wantCalls; outcome='$outcome', want '$wantOutcome')"
    }
}
"cases passed: $passed, failed: $failedCases"
if ($failedCases -gt 0) { exit 1 }
