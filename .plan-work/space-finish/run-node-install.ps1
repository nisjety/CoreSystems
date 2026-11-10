Set-Location -LiteralPath 'C:\dev\space-finish\apps\Application Plane\convex-core'
pnpm install --frozen-lockfile 2>&1
Write-Output "convex-install exit=$LASTEXITCODE"
Set-Location -LiteralPath 'C:\dev\space-finish\apps\Control Plane\auth-core'
pnpm install --frozen-lockfile 2>&1
Write-Output "auth-install exit=$LASTEXITCODE"
