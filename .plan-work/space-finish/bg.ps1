# Background job runner for the space-finish workstream.
# Usage: bg.ps1 -Name <id> -Dir <workdir> -Cmd <command string>
# Writes .plan-work/space-finish/logs/<id>.log and <id>.exit (exit code), plus <id>.start/.end timestamps.
param(
  [Parameter(Mandatory)] [string] $Name,
  [Parameter(Mandatory)] [string] $Dir,
  [Parameter(Mandatory)] [string] $Cmd
)
$logDir = 'C:\dev\space-finish\.plan-work\space-finish\logs'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$log = Join-Path $logDir "$Name.log"
$exit = Join-Path $logDir "$Name.exit"
Remove-Item -LiteralPath $exit -ErrorAction SilentlyContinue
$inner = @"
`$ErrorActionPreference = 'Continue'
`$env:CARGO_TERM_COLOR = 'never'
`$env:NO_COLOR = '1'
Set-Location -LiteralPath '$Dir'
(Get-Date -Format o) | Out-File -Encoding utf8 '$logDir\$Name.start'
& { $Cmd } *>&1 | Out-File -Encoding utf8 -Width 400 '$log'
`$code = `$LASTEXITCODE
(Get-Date -Format o) | Out-File -Encoding utf8 '$logDir\$Name.end'
"`$code" | Out-File -Encoding ascii '$exit'
"@
$script = Join-Path $logDir "$Name.ps1"
$inner | Out-File -Encoding utf8 $script
Start-Process -WindowStyle Hidden -FilePath powershell.exe -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',$script | Out-Null
"started $Name"
