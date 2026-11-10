# Commit the staged changes with a message file, letting the pre-commit secret scan finish.
# Usage: commit.ps1 -Msg <path to message file>
param([Parameter(Mandatory)][string]$Msg)
Set-Location -LiteralPath 'C:\dev\space-finish'
# The repo's pre-commit hook prefers a local gitleaks binary over the Docker image;
# both scan the same piped staged diff with the same .gitleaks.toml. The local binary
# is used because the Docker engine on this host is down (see phase1-notes.md).
$env:PATH = 'C:\dev\space-finish-tools;' + $env:PATH
git commit -F $Msg 2>&1
Write-Output "commit exit=$LASTEXITCODE"
git log --oneline -1
