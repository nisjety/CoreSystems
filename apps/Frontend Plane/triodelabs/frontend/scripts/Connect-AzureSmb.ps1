<#
.SYNOPSIS
Connects a Windows computer to an Azure Files SMB share after Azure VPN is connected.

.DESCRIPTION
The Azure VPN Client connection itself remains a user action because the Windows client
does not provide a stable command-line interface for connecting arbitrary imported
profiles. This script checks the connection to SMB, maps the share, creates a desktop
shortcut, and opens the mapped drive.

The storage key is deliberately not stored in this file. Windows prompts for it as a
password without echoing it. The mapping is persistent for the current Windows user.

.EXAMPLE
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
.\Connect-AzureSmb.ps1

.EXAMPLE
# Retain Azure Files' FQDN by default. Only specify this for an approved custom SMB host.
.\Connect-AzureSmb.ps1 -SmbHost 'files.example.internal'
#>

[CmdletBinding()]
param(
    [string]$StorageAccountName = 'stcadprodnwe001',
    [string]$ShareName = 'aquatiq-azure',
    [ValidatePattern('^[A-Z]$')]
    [string]$DriveLetter = 'Z',
    [string]$SmbHost
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Status {
    param([string]$Message)

    Write-Host "[+] $Message" -ForegroundColor Green
}

function Write-WarningStatus {
    param([string]$Message)

    Write-Host "[!] $Message" -ForegroundColor Yellow
}

$smbHostName = if ([string]::IsNullOrWhiteSpace($SmbHost)) {
    "$StorageAccountName.file.core.windows.net"
} else {
    $SmbHost
}
$smbPath = "\\$smbHostName\$ShareName"
$credentialUserName = "Azure\$StorageAccountName"

Write-Host '=== Azure VPN + SMB Setup for Windows ==='
Write-Host "SMB share: $smbPath"
Write-Host "Drive letter: $DriveLetter`:"
Write-Host ''

Write-Host '=== System Check ==='
if (Get-Command -Name Test-NetConnection -ErrorAction SilentlyContinue) {
    Write-Status 'Test-NetConnection is available.'
} else {
    throw 'Test-NetConnection is unavailable. Run this script on Windows 8.1/Server 2012 R2 or newer.'
}

$azureVpnInstalled = @(Get-AppxPackage -Name '*AzureVPN*' -ErrorAction SilentlyContinue).Count -gt 0
if ($azureVpnInstalled) {
    Write-Status 'Azure VPN Client appears to be installed.'
} else {
    Write-WarningStatus 'Azure VPN Client was not detected. Install it from the Microsoft Store if needed.'
}

Write-Host ''
Write-Host '=== VPN Check ==='
Write-Host "Connect to the 'Aquatiq-Azure-VPN' profile in Azure VPN Client before continuing."
$vpnStatus = Read-Host 'Are you connected to Azure VPN? (y/n/skip)'
if ($vpnStatus -notin @('y', 'Y', 'skip')) {
    Write-Host ''
    Write-Host 'To connect:'
    Write-Host '1. Open Azure VPN Client.'
    Write-Host '2. Import the Aquatiq-Azure-VPN profile if it is not already present.'
    Write-Host '3. Sign in with your Azure AD account and connect.'
    [void](Read-Host 'Press Enter after connecting, or press Ctrl+C to cancel')
}

Write-Host ''
Write-Host '=== Testing SMB Connection ==='
Write-Host "Testing $smbHostName on TCP port 445..."
$connectionOk = Test-NetConnection -ComputerName $smbHostName -Port 445 -InformationLevel Quiet -WarningAction SilentlyContinue

if ($connectionOk) {
    Write-Status 'The SMB endpoint is reachable.'
} else {
    Write-WarningStatus "Cannot reach $smbHostName on TCP port 445."
    Write-Host 'Confirm that Azure VPN is connected and that your network allows SMB traffic.'
    $tryAnyway = Read-Host 'Try mapping the share anyway? (y/n)'
    if ($tryAnyway -notin @('y', 'Y')) {
        exit 1
    }
}

Write-Host ''
Write-Host '=== SMB Connection ==='
$credential = Get-Credential -UserName $credentialUserName -Message "Enter the Azure Files access key for $StorageAccountName as the password."
$existingDrive = Get-PSDrive -Name $DriveLetter -ErrorAction SilentlyContinue
$driveReady = $false

if ($existingDrive) {
    $existingRoot = if ($existingDrive.DisplayRoot) { $existingDrive.DisplayRoot } else { $existingDrive.Root }
    if ($existingRoot -ieq $smbPath) {
        if (Test-Path -LiteralPath "$DriveLetter`:\") {
            Write-Status "Drive $DriveLetter`: is already mapped to this share."
            $driveReady = $true
        } else {
            Write-WarningStatus "Drive $DriveLetter`: is mapped but not currently reachable."
            $replaceStaleMapping = Read-Host 'Remove the stale mapping and reconnect? (y/n)'
            if ($replaceStaleMapping -in @('y', 'Y')) {
                Remove-PSDrive -Name $DriveLetter -Force
                Write-Status "Removed the stale $DriveLetter`: mapping."
            } else {
                throw "Cannot continue while the stale $DriveLetter`: mapping remains."
            }
        }
    } else {
        throw "Drive $DriveLetter`: is already mapped to $existingRoot. Choose an unused drive letter with -DriveLetter."
    }
}

if (-not $driveReady) {
    New-PSDrive -Name $DriveLetter -PSProvider FileSystem -Root $smbPath -Credential $credential -Persist -Scope Global | Out-Null
    if (-not (Test-Path -LiteralPath "$DriveLetter`:")) {
        throw "Windows created the $DriveLetter`: mapping, but the share cannot be opened. Check Azure VPN, private DNS, and access permissions."
    }
    Write-Status "Mapped $smbPath to $DriveLetter`:"
}

try {
    $desktopPath = [Environment]::GetFolderPath('Desktop')
    $shortcutPath = Join-Path -Path $desktopPath -ChildPath 'Azure Share.lnk'
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($shortcutPath)
    $shortcut.TargetPath = "$DriveLetter`:\"
    $shortcut.Description = "Azure SMB share ($ShareName)"
    $shortcut.Save()
    Write-Status "Created desktop shortcut: $shortcutPath"
} catch {
    Write-WarningStatus "The share is mapped, but a desktop shortcut could not be created: $($_.Exception.Message)"
}

Start-Process -FilePath explorer.exe -ArgumentList "$DriveLetter`:\"

Write-Host ''
Write-Host 'Setup complete.' -ForegroundColor Cyan
Write-Host "Access the share in File Explorer at $DriveLetter`: or through the Azure Share desktop shortcut."
Write-Host 'Keep Azure VPN connected while you use the share.'
