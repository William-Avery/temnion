# SPDX-License-Identifier: AGPL-3.0-only
<#
.SYNOPSIS
    Uninstalls the Temnion Windows Service.

.DESCRIPTION
    Stops and removes the Temnion database daemon Windows Service.
    Requires administrative privileges.

.PARAMETER ServiceName
    Internal service name identifier to remove. Defaults to "Temnion".
#>

[CmdletBinding()]
param(
    [string]$ServiceName = "Temnion"
)

$ErrorActionPreference = "Stop"

# Ensure running with administrative elevation
$currentPrincipal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "This script must be executed in an elevated PowerShell session (Run as Administrator)."
    exit 1
}

$svc = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if (-not $svc) {
    Write-Host "Service '$ServiceName' is not installed." -ForegroundColor Yellow
    exit 0
}

Write-Host "Removing service '$ServiceName'..." -ForegroundColor Cyan

if ($svc.Status -eq 'Running') {
    Write-Host "Stopping service..."
    Stop-Service -Name $ServiceName -Force
    $svc.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
}

sc.exe delete $ServiceName

Write-Host "Service '$ServiceName' successfully removed." -ForegroundColor Green
