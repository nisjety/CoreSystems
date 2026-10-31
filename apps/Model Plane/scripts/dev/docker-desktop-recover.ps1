#Requires -Version 7.0
<#
.SYNOPSIS
    Recovers a wedged Docker Desktop on Windows. A dry run unless -Apply is given.

.DESCRIPTION
    The dry run only reads. It lists the Docker Desktop processes, the
    docker-desktop WSL distro state, load and free memory inside the VM, how many
    rustc / rust-lld / cargo processes run inside it, the two stale socket
    folders, and recent log lines that match known wedge signatures. Its one
    command inside the VM is that VM probe, which runs only when the distro is
    Running, and it is bounded to 30 seconds.

    -Apply runs four steps in order:
      1. Closes, then force-stops, the Docker Desktop processes (never docker.exe).
      2. Terminates the docker-desktop WSL distro (wsl --terminate only). It is
         skipped only when the distro is reported Stopped (or wsl.exe is absent).
         When the state could not be read, a failed terminate is a warning and the
         run goes on to the rename; for a distro seen Running it stops the run.
      3. RENAMES, never deletes, %LOCALAPPDATA%\docker-secrets-engine and
         %LOCALAPPDATA%\Docker\run to *.stale-<yyyyMMdd-HHmmss>.
      4. Starts Docker Desktop and waits up to five minutes for docker info.

    It never resets Docker Desktop to factory defaults, never unregisters or
    shuts down a WSL distro, never removes a container, image, volume or file,
    and never reads or prints a credential.

    For the human operator or the lead only. Automated agents never run -Apply.

    Runbook: apps/Model Plane/docs/runbooks/docker-desktop-recovery.md

.PARAMETER Apply
    Perform the four steps. Without it the script changes nothing.

.PARAMETER Force
    Allow -Apply while docker info already answers. Step 2 then stops every
    running container. Use it only when the engine answers but cannot be used.

.PARAMETER DockerDesktopExe
    Full path to Docker Desktop.exe when it is not in a default location.

.OUTPUTS
    0  dry run complete, or -Apply finished and the engine answers.
    1  a step failed, or the engine did not answer within five minutes.
    2  refused by a guard (the engine answers and -Force was not given).
    3  Docker Desktop.exe was not found.
#>
[CmdletBinding()]
param(
    [switch]$Apply,
    [switch]$Force,
    [string]$DockerDesktopExe
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# wsl.exe writes UTF-16 unless this is set. It affects only child processes of this script.
$env:WSL_UTF8 = '1'

$Distro       = 'docker-desktop'
$ProcessRegex = '^(Docker Desktop|com\.docker\..+|vpnkit.*)$'
$StaleFolders = @(
    (Join-Path $env:LOCALAPPDATA 'docker-secrets-engine'),
    (Join-Path $env:LOCALAPPDATA 'Docker\run')
)
$LogRoot      = Join-Path $env:LOCALAPPDATA 'Docker\log'
$WedgeRegex   = 'no route to host|initializing Secrets Engine|initializing Ingest server|error 1920|cannot be accessed by the system'
$Stamp        = Get-Date -Format 'yyyyMMdd-HHmmss'

function Show-Section([string]$Title) {
    Write-Host ''
    Write-Host ('== {0}' -f $Title)
}

function Resolve-Tool([string]$Name) {
    $command = Get-Command -Name $Name -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($command) { return $command.Source }
    return $null
}

# Runs a native program with a time limit. It writes no temporary files.
function Invoke-Bounded {
    param(
        [Parameter(Mandatory)][string]$FilePath,
        [string[]]$Arguments = @(),
        [int]$TimeoutSeconds = 20
    )
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new($FilePath)
    foreach ($argument in $Arguments) { [void]$startInfo.ArgumentList.Add($argument) }
    $startInfo.UseShellExecute        = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError  = $true
    $startInfo.CreateNoWindow         = $true
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    [void]$process.Start()
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
        try { $process.Kill($true) } catch { }
        return [pscustomobject]@{ TimedOut = $true; ExitCode = $null; Stdout = ''; Stderr = '' }
    }
    $process.WaitForExit()
    [pscustomobject]@{
        TimedOut = $false
        ExitCode = $process.ExitCode
        Stdout   = $stdout.Result
        Stderr   = $stderr.Result
    }
}

$docker = Resolve-Tool 'docker.exe'
$wsl    = Resolve-Tool 'wsl.exe'

function Get-DockerProcesses {
    Get-Process -ErrorAction SilentlyContinue |
        Where-Object { $_.ProcessName -match $ProcessRegex } |
        Sort-Object ProcessName
}

function Test-Engine {
    if (-not $docker) { return $false }
    $result = Invoke-Bounded -FilePath $docker -Arguments @('info', '--format', '{{.ServerVersion}}') -TimeoutSeconds 20
    return ((-not $result.TimedOut) -and ($result.ExitCode -eq 0))
}

function Get-DistroState {
    if (-not $wsl) { return 'wsl.exe not found' }
    $result = Invoke-Bounded -FilePath $wsl -Arguments @('-l', '-v') -TimeoutSeconds 20
    if ($result.TimedOut -or $result.ExitCode -ne 0) { return 'unknown (wsl -l -v failed)' }
    $pattern = '^\s*\*?\s*' + [regex]::Escape($Distro) + '\s+(\S+)\s+\d'
    foreach ($line in ($result.Stdout -split '\r?\n')) {
        if ($line -match $pattern) { return $Matches[1] }
    }
    return 'absent'
}

function Get-DockerDesktopExe {
    $candidates = if ($DockerDesktopExe) {
        @($DockerDesktopExe)
    }
    else {
        @(
            (Join-Path $env:LOCALAPPDATA 'Programs\DockerDesktop\Docker Desktop.exe'),
            (Join-Path $env:ProgramFiles 'Docker\Docker\Docker Desktop.exe')
        )
    }
    return ($candidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1)
}

function Get-WedgeLogLines {
    if (-not (Test-Path -LiteralPath $LogRoot)) { return @() }
    Get-ChildItem -LiteralPath $LogRoot -Recurse -File -Filter '*.log' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 8 |
        ForEach-Object {
            Select-String -LiteralPath $_.FullName -Pattern $WedgeRegex -ErrorAction SilentlyContinue |
                Select-Object -Last 3 |
                ForEach-Object {
                    $text = $_.Line.Trim()
                    '{0}:{1}: {2}' -f $_.Filename, $_.LineNumber, $text.Substring(0, [Math]::Min(300, $text.Length))
                }
        }
}

function Stop-DockerProcesses {
    $targets = @(Get-DockerProcesses)
    foreach ($process in $targets) {
        try { [void]$process.CloseMainWindow() } catch { }
    }
    if ($targets.Count -gt 0) { Start-Sleep -Seconds 10 }
    foreach ($process in @(Get-DockerProcesses)) {
        try {
            Stop-Process -Id $process.Id -Force
            Write-Host ('stopped: {0} [{1}]' -f $process.ProcessName, $process.Id)
        }
        catch {
            Write-Warning ('not stopped: {0} [{1}]: {2}' -f $process.ProcessName, $process.Id, $_.Exception.Message)
        }
    }
}

function Stop-Distro {
    $state = Get-DistroState
    # Nothing is terminated only when the distro is reported Stopped, or when wsl.exe is absent
    # (then no distro can be running). Running is the normal case. An unknown state (the probe
    # failed or timed out) and an absent one (the probe read the distro list and found no such
    # distro) are not proof that nothing runs, and the probe can misread a running distro, so the
    # terminate is attempted for them too. It is harmless on a stopped distro.
    if ($state -eq 'Stopped' -or $state -eq 'wsl.exe not found') {
        Write-Host ('distro ''{0}'' is ''{1}''; nothing to terminate' -f $Distro, $state)
        return
    }
    # A failed or timed-out terminate is fatal only when the distro was positively seen Running.
    # For an unknown state (the wedged-WSL case this script exists for) the probe already could
    # not talk to WSL, so the terminate failing too is expected, and the folder rename that
    # follows needs only the Docker processes stopped: carry on to it with a warning.
    $mustSucceed = ($state -eq 'Running')
    $result = Invoke-Bounded -FilePath $wsl -Arguments @('--terminate', $Distro) -TimeoutSeconds 90
    if ($result.TimedOut) {
        $message = ('wsl --terminate {0} timed out after 90 seconds' -f $Distro)
        if ($mustSucceed) { throw $message }
        Write-Warning ('{0} (state was ''{1}''); continuing to the rename' -f $message, $state)
        return
    }
    if ($result.ExitCode -ne 0) {
        if ($state -eq 'absent') {
            # The probe read the distro list and found no docker-desktop distro, so WSL saying the same is expected.
            Write-Host ('no ''{0}'' distro to terminate: {1}' -f $Distro, $result.Stderr.Trim())
            return
        }
        $message = ('wsl --terminate {0} failed: {1}' -f $Distro, $result.Stderr.Trim())
        if ($mustSucceed) { throw $message }
        Write-Warning ('{0} (state was ''{1}''); continuing to the rename' -f $message, $state)
        return
    }
    Write-Host ('terminated: {0}' -f $Distro)
}

function Rename-StaleFolders {
    foreach ($folder in $StaleFolders) {
        if (-not (Test-Path -LiteralPath $folder)) {
            Write-Host ('absent, nothing to rename: {0}' -f $folder)
            continue
        }
        if (-not $folder.StartsWith($env:LOCALAPPDATA, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw ('refusing to rename outside LOCALAPPDATA: {0}' -f $folder)
        }
        $newName = '{0}.stale-{1}' -f (Split-Path -Leaf $folder), $Stamp
        $target  = Join-Path (Split-Path -Parent $folder) $newName
        if (Test-Path -LiteralPath $target) { throw ('destination already exists: {0}' -f $target) }
        Rename-Item -LiteralPath $folder -NewName $newName
        Write-Host ('renamed: {0} -> {1}' -f $folder, $newName)
    }
}

function Wait-Engine([int]$Minutes) {
    $deadline = (Get-Date).AddMinutes($Minutes)
    while ((Get-Date) -lt $deadline) {
        if (Test-Engine) { return $true }
        Start-Sleep -Seconds 10
    }
    return $false
}

# ---- Dry run: read-only report -------------------------------------------------

Show-Section 'Docker Desktop processes'
$processes = @(Get-DockerProcesses)
if ($processes.Count -eq 0) { Write-Host 'none running' }
else { $processes | Select-Object ProcessName, Id | Format-Table -AutoSize | Out-String | Write-Host }

Show-Section 'Docker engine'
$engineUp = Test-Engine
if (-not $docker) { Write-Host 'docker.exe is not on PATH' }
elseif ($engineUp) { Write-Host 'answers docker info' }
else { Write-Host 'does not answer docker info within 20 seconds' }

Show-Section ('WSL distro ''{0}''' -f $Distro)
$state = Get-DistroState
Write-Host ('state: {0}' -f $state)
if ($state -eq 'Running') {
    $vm = Invoke-Bounded -FilePath $wsl -Arguments @(
        '-d', $Distro, '--exec', 'sh', '-c',
        'cat /proc/loadavg; grep -E ''MemTotal|MemAvailable'' /proc/meminfo; printf ''build_processes ''; grep -l -x -E ''rustc|rust-lld|cargo'' /proc/[0-9]*/comm 2>/dev/null | wc -l'
    ) -TimeoutSeconds 30
    if ($vm.TimedOut) { Write-Host 'VM probe timed out after 30 seconds; the VM is probably overloaded' }
    else { Write-Host $vm.Stdout.Trim() }
}
else {
    Write-Host 'VM probe skipped: the distro is not running, and this script does not start it'
}

Show-Section 'stale socket folders'
foreach ($folder in $StaleFolders) {
    if (Test-Path -LiteralPath $folder) {
        $item    = Get-Item -LiteralPath $folder -Force
        $reparse = $item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)
        Write-Host ('present: {0} (reparse point: {1})' -f $folder, $reparse)
    }
    else {
        Write-Host ('absent: {0}' -f $folder)
    }
}

Show-Section 'Docker Desktop executable'
$exe = Get-DockerDesktopExe
if ($exe) { Write-Host ('found: {0}' -f $exe) }
else { Write-Host 'not found in the default locations; pass -DockerDesktopExe' }

Show-Section 'recent wedge signatures in the Docker logs (matching lines only)'
$lines = @(Get-WedgeLogLines)
if ($lines.Count -eq 0) { Write-Host 'none' }
else { $lines | ForEach-Object { Write-Host $_ } }

if (-not $Apply) {
    Show-Section 'DRY RUN: nothing was changed'
    Write-Host 'Read apps/Model Plane/docs/runbooks/docker-desktop-recovery.md, then re-run with -Apply.'
    if (-not $exe) { exit 3 }
    exit 0
}

# ---- Apply ----------------------------------------------------------------------

if ($engineUp -and -not $Force) {
    Write-Host 'REFUSED: the engine answers docker info. Recovery stops every running container.'
    Write-Host 'Re-run with -Apply -Force only if the engine answers but cannot be used.'
    exit 2
}
if (-not $exe) {
    Write-Host 'REFUSED: Docker Desktop.exe was not found; pass -DockerDesktopExe.'
    exit 3
}

try {
    Show-Section 'step 1: stop Docker Desktop processes'
    Stop-DockerProcesses

    Show-Section ('step 2: terminate WSL distro ''{0}'' (no unregister, no shutdown)' -f $Distro)
    Stop-Distro

    Show-Section 'step 3: rename stale socket folders (never delete)'
    Rename-StaleFolders

    Show-Section 'step 4: start Docker Desktop and wait up to five minutes for the engine'
    Start-Process -FilePath $exe
    if (Wait-Engine -Minutes 5) {
        Write-Host 'engine answers docker info'
        exit 0
    }
    Write-Host 'engine did not answer within five minutes. Do not reset. See "If the engine still does not start" in the runbook.'
    exit 1
}
catch {
    Write-Host ('STEP FAILED: {0}' -f $_.Exception.Message)
    Write-Host 'No file was deleted. Re-run without -Apply to see the current state.'
    exit 1
}
