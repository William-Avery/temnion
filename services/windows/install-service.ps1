# SPDX-License-Identifier: AGPL-3.0-only
<#
.SYNOPSIS
    Installs temniond as a Windows Service.

.DESCRIPTION
    Registers the Temnion Temporal-Epistemic Database daemon as a managed
    Windows Service using New-Service or sc.exe. Requires administrative privileges.

.PARAMETER BinaryPath
    Full path to temniond.exe executable.

.PARAMETER ConfigPath
    Full path to temnion.toml configuration file.

.PARAMETER ServiceName
    Internal service name identifier. Defaults to "Temnion".

.PARAMETER DisplayName
    User-visible service display name. Defaults to "Temnion Database Daemon".

.PARAMETER StartupType
    Startup type: Automatic, Manual, or Disabled. Defaults to "Automatic".
#>

[CmdletBinding()]
param(
    [string]$BinaryPath = "$PSScriptRoot\..\..\target\release\temniond.exe",
    [string]$ConfigPath = "C:\ProgramData\Temnion\temnion.toml",
    [string]$ServiceName = "Temnion",
    [string]$DisplayName = "Temnion Database Daemon",
    [ValidateSet("Automatic", "Manual", "Disabled")]
    [string]$StartupType = "Automatic"
)

$ErrorActionPreference = "Stop"

# Ensure running with administrative elevation
$currentPrincipal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "This script must be executed in an elevated PowerShell session (Run as Administrator)."
    exit 1
}

# Resolve paths
$resolvedBinary = (Resolve-Path $BinaryPath -ErrorAction SilentlyContinue)
if (-not $resolvedBinary -or -not (Test-Path $resolvedBinary.Path)) {
    Write-Error "Could not locate binary at '$BinaryPath'. Please specify a valid -BinaryPath."
    exit 1
}
$binaryFullPath = $resolvedBinary.Path

Write-Host "Registering Windows Service '$ServiceName'..." -ForegroundColor Cyan
Write-Host "  Binary:  $binaryFullPath"
Write-Host "  Config:  $ConfigPath"
Write-Host "  Startup: $StartupType"

$binPathWithArgs = "`"$binaryFullPath`" run --config `"$ConfigPath`""

$existing = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($existing) {
    Write-Warning "Service '$ServiceName' is already registered. Stopping and updating..."
    if ($existing.Status -eq 'Running') {
        Stop-Service -Name $ServiceName -Force
    }
    sc.exe config $ServiceName binPath= $binPathWithArgs start= auto
} else {
    New-Service `
        -Name $ServiceName `
        -DisplayName $DisplayName `
        -BinaryPathName $binPathWithArgs `
        -StartupType $StartupType `
        -Description "Temnion temporal-epistemic database background daemon providing TNP, Flight, and MCP endpoints."
}

# Set service recovery actions (restart on failure)
sc.exe failure $ServiceName reset= 86400 actions= restart/5000/restart/10000/restart/30000

Write-Host "Service '$ServiceName' successfully installed." -ForegroundColor Green
Write-Host "To start the service now, run: Start-Service -Name $ServiceName" -ForegroundColor Yellow
