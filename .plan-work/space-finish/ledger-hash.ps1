# Compare the auth migration ledger checksums captured from the 2026-10-27 proof with LF and CRLF hashes
# of the files in this worktree. Read-only; touches no database.
Set-Location -LiteralPath 'C:\dev\space-finish'
$sha = [System.Security.Cryptography.SHA256]::Create()
function Get-Sha([byte[]]$b) { ($sha.ComputeHash($b) | ForEach-Object { $_.ToString('x2') }) -join '' }
$ledger = @{}
Get-Content .plan-work/space-proof/auth_ledger.data.sql | Where-Object { $_ -match "^\S+\.sql`t" } | ForEach-Object {
  $p = $_ -split "`t"; $ledger[$p[0]] = $p[1]
}
foreach ($name in $ledger.Keys | Sort-Object) {
  $path = "apps/Control Plane/auth-core/migrations/$name"
  if (-not (Test-Path -LiteralPath $path)) { "{0,-45} MISSING in worktree" -f $name; continue }
  $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $path))
  $text = [System.Text.Encoding]::UTF8.GetString($bytes)
  $lfText = $text -replace "`r`n", "`n"
  $lf = Get-Sha ([System.Text.Encoding]::UTF8.GetBytes($lfText))
  $crlf = Get-Sha ([System.Text.Encoding]::UTF8.GetBytes(($lfText -replace "`n", "`r`n")))
  $m = if ($ledger[$name] -eq $lf) { 'ledger=LF' } elseif ($ledger[$name] -eq $crlf) { 'ledger=CRLF' } else { 'ledger=OTHER' }
  "{0,-45} {1}" -f $name, $m
}
