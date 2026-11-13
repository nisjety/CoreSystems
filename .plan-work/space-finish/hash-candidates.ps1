Set-Location -LiteralPath 'C:\dev\space-finish'
$target = '9d2492d1d5d3c3f76f47b537a650e0f58e36276876a3b23378d76330f40d3034'
$revs = git log --all --format='%h' -- 'apps/Control Plane/auth-core/migrations/init_better_auth.sql'
$sha = [System.Security.Cryptography.SHA256]::Create()
foreach ($r in $revs) {
  foreach ($base in "$r", "$r~1") {
    $spec = "${base}:apps/Control Plane/auth-core/migrations/init_better_auth.sql"
    $blob = git rev-parse --verify --quiet $spec 2>$null
    if (-not $blob) { continue }
    $tmp = [System.IO.Path]::GetTempFileName()
    $p = Start-Process -FilePath git -ArgumentList 'cat-file','blob',$blob -RedirectStandardOutput $tmp -NoNewWindow -Wait -PassThru
    $bytes = [System.IO.File]::ReadAllBytes($tmp)
    $lf = ($sha.ComputeHash($bytes) | ForEach-Object { $_.ToString('x2') }) -join ''
    $text = [System.Text.Encoding]::UTF8.GetString($bytes)
    $crlfText = ($text -replace "`r`n", "`n") -replace "`n", "`r`n"
    $crlf = ($sha.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($crlfText)) | ForEach-Object { $_.ToString('x2') }) -join ''
    $m = if ($lf -eq $target) { 'MATCH-LF' } elseif ($crlf -eq $target) { 'MATCH-CRLF' } else { '' }
    "$base blob=$blob lf=$($lf.Substring(0,12)) crlf=$($crlf.Substring(0,12)) $m"
    Remove-Item $tmp
  }
}
