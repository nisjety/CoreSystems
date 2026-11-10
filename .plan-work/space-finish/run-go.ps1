param([string]$Extra = '')
Set-Location -LiteralPath 'C:\dev\space-finish\apps\Control Plane'
foreach ($s in 'user-core','org-core','audit-core') {
  Write-Output "===== $s"
  Push-Location $s
  go vet ./... 2>&1 | Select-Object -Last 20
  Write-Output "===== $s vet exit=$LASTEXITCODE"
  go test -count=1 ./... 2>&1
  Write-Output "===== $s test exit=$LASTEXITCODE"
  Pop-Location
}
