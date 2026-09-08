# SPDX-License-Identifier: AGPL-3.0-only
<#
.SYNOPSIS
    Interactive PostgreSQL-style Component Installer & Setup Wizard for Temnion.

.DESCRIPTION
    Installs Temnion components with checkbox selection:
      [1] Temnion Core Database Engine & CLI (tem)
      [2] Temnion Background Server Daemon (temniond)
      [3] Temnion Studio Workbench (IDE GUI & Web)
      [4] Tzeentch Autonomous Client (Side Program)
      [5] Windows System Service (temniond auto-start)
    Configures database name, network port, admin credentials, and provisions directories.

.PARAMETER Silent
    Runs unattended installation using parameters without interactive prompts.

.PARAMETER Components
    Comma-separated list of components to install: tem, temniond, studio, tzeentch, service.

.PARAMETER DbName
    Logical database name (default: temnion_default).

.PARAMETER Port
    TNP server network port (default: 9180).

.PARAMETER FlightPort
    Arrow Flight analytical port (default: 9181).

.PARAMETER Username
    Database superuser administrator username (default: temnion_admin).

.PARAMETER Password
    Authentication token or password for superuser.

.PARAMETER InstallDir
    Target directory for Temnion binaries and assets.

.PARAMETER DataDir
    Target directory for database store files.
#>

[CmdletBinding()]
param(
    [switch]$Silent,
    [string]$Components = "tem,temniond,studio,tzeentch,service",
    [string]$DbName = "temnion_default",
    [int]$Port = 9180,
    [int]$FlightPort = 9181,
    [string]$BindHost = "127.0.0.1",
    [string]$Username = "temnion_admin",
    [string]$Password = "",
    [string]$InstallDir = "",
    [string]$DataDir = ""
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path "$PSScriptRoot\..").Path

function Write-Banner {
    Write-Host "================================================================================" -ForegroundColor Cyan
    Write-Host "        TEMNION ENTERPRISE TEMPORAL DATABASE - SETUP WIZARD                    " -ForegroundColor White
    Write-Host "        Deterministic Bounded Architecture | Rust 2024 | forbid(unsafe_code)   " -ForegroundColor DarkGray
    Write-Host "================================================================================" -ForegroundColor Cyan
    Write-Host ""
}

# Determine default paths based on privileges
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

if (-not $InstallDir) {
    $InstallDir = if ($isAdmin) { "C:\Program Files\Temnion" } else { "$env:LOCALAPPDATA\Temnion" }
}
if (-not $DataDir) {
    $DataDir = if ($isAdmin) { "C:\ProgramData\Temnion\data" } else { "$env:LOCALAPPDATA\Temnion\data" }
}

# Component flags
$selectedComponents = @{
    "tem"       = $true
    "temniond"  = $true
    "studio"    = $true
    "tzeentch"  = $true
    "service"   = $isAdmin
}

# Interactive Wizard
if (-not $Silent) {
    Write-Banner
    Write-Host "Welcome to the Temnion Database Setup Wizard." -ForegroundColor Green
    Write-Host "This installer will guide you through component selection, port allocation,"
    Write-Host "and superuser credential generation, just like PostgreSQL Setup."
    Write-Host ""
    Write-Host "Press ENTER to continue..." -NoNewline
    [void][Console]::ReadLine()

    # Step 1: Component Selection Checkboxes
    $selecting = $true
    while ($selecting) {
        Write-Banner
        Write-Host "--- STEP 1 of 4: Select Components to Install ---" -ForegroundColor Yellow
        Write-Host "Toggle components using numbers (1-5), or press ENTER to accept selection:"
        Write-Host ""

        $c1 = if ($selectedComponents["tem"])      { "[X]" } else { "[ ]" }
        $c2 = if ($selectedComponents["temniond"])  { "[X]" } else { "[ ]" }
        $c3 = if ($selectedComponents["studio"])    { "[X]" } else { "[ ]" }
        $c4 = if ($selectedComponents["tzeentch"])  { "[X]" } else { "[ ]" }
        $c5 = if ($selectedComponents["service"])   { "[X]" } else { "[ ]" }

        Write-Host "  1. $c1 Temnion Core Database Engine & CLI (tem.exe)" -ForegroundColor $(if ($selectedComponents["tem"]) { "Green" } else { "DarkGray" })
        Write-Host "  2. $c2 Temnion Server Daemon (temniond.exe - TNP/Flight/MCP)" -ForegroundColor $(if ($selectedComponents["temniond"]) { "Green" } else { "DarkGray" })
        Write-Host "  3. $c3 Temnion Studio Workbench (IDE GUI - MySQL Workbench style)" -ForegroundColor $(if ($selectedComponents["studio"]) { "Green" } else { "DarkGray" })
        Write-Host "  4. $c4 Tzeentch Autonomous Client (Standalone side-program & tracer)" -ForegroundColor $(if ($selectedComponents["tzeentch"]) { "Green" } else { "DarkGray" })
        Write-Host "  5. $c5 Register Windows Service (Automatic background daemon)" -ForegroundColor $(if ($selectedComponents["service"]) { "Green" } else { "DarkGray" })
        Write-Host ""
        Write-Host "Enter item numbers to toggle (e.g. '4', '3,5'), or press ENTER to confirm: " -NoNewline -ForegroundColor Cyan
        $choice = [Console]::ReadLine()

        if ([string]::IsNullOrWhiteSpace($choice)) {
            $selecting = $false
        } else {
            $tokens = $choice -split '[, ]+'
            foreach ($tok in $tokens) {
                switch ($tok.Trim()) {
                    "1" { $selectedComponents["tem"] = -not $selectedComponents["tem"] }
                    "2" { $selectedComponents["temniond"] = -not $selectedComponents["temniond"] }
                    "3" { $selectedComponents["studio"] = -not $selectedComponents["studio"] }
                    "4" { $selectedComponents["tzeentch"] = -not $selectedComponents["tzeentch"] }
                    "5" { $selectedComponents["service"] = -not $selectedComponents["service"] }
                }
            }
        }
    }

    # Step 2: Database Configuration
    Write-Banner
    Write-Host "--- STEP 2 of 4: Database & Network Configuration ---" -ForegroundColor Yellow
    Write-Host "Specify database name and listening ports for network communication."
    Write-Host ""

    Write-Host "Database Name [$DbName]: " -NoNewline -ForegroundColor Cyan
    $inDb = [Console]::ReadLine()
    if ($inDb) { $DbName = $inDb.Trim() }

    Write-Host "TNP Server Port [$Port]: " -NoNewline -ForegroundColor Cyan
    $inPort = [Console]::ReadLine()
    if ($inPort -and ($inPort -as [int])) { $Port = [int]$inPort }

    Write-Host "Arrow Flight Port [$FlightPort]: " -NoNewline -ForegroundColor Cyan
    $inFlight = [Console]::ReadLine()
    if ($inFlight -and ($inFlight -as [int])) { $FlightPort = [int]$inFlight }

    Write-Host "Listen Address [$BindHost]: " -NoNewline -ForegroundColor Cyan
    $inHost = [Console]::ReadLine()
    if ($inHost) { $BindHost = $inHost.Trim() }

    # Step 3: Superuser Credentials
    Write-Banner
    Write-Host "--- STEP 3 of 4: Superuser Credentials ---" -ForegroundColor Yellow
    Write-Host "Create administrative credentials for authentication and connector authorization."
    Write-Host ""

    Write-Host "Superuser Username [$Username]: " -NoNewline -ForegroundColor Cyan
    $inUser = [Console]::ReadLine()
    if ($inUser) { $Username = $inUser.Trim() }

    $passPrompting = $true
    while ($passPrompting) {
        Write-Host "Enter Password / Auth Token (leave blank to generate secure token): " -NoNewline -ForegroundColor Cyan
        $securePass = Read-Host -AsSecureString
        $bstr = [System.Runtime.InteropServices.Marshal]::SecureStringToBSTR($securePass)
        $plainPass = [System.Runtime.InteropServices.Marshal]::PtrToStringAuto($bstr)
        [System.Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr)

        if (-not $plainPass) {
            # Auto-generate random secure hex token
            $bytes = New-Object byte[] 16
            (New-Object Security.Cryptography.RNGCryptoServiceProvider).GetBytes($bytes)
            $plainPass = ($bytes | ForEach-Object { $_.ToString("x2") }) -join ""
            Write-Host "Generated secure token: $plainPass" -ForegroundColor Green
            $Password = $plainPass
            $passPrompting = $false
        } else {
            Write-Host "Confirm Password / Auth Token: " -NoNewline -ForegroundColor Cyan
            $secureConfirm = Read-Host -AsSecureString
            $bstrConf = [System.Runtime.InteropServices.Marshal]::SecureStringToBSTR($secureConfirm)
            $plainConfirm = [System.Runtime.InteropServices.Marshal]::PtrToStringAuto($bstrConf)
            [System.Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstrConf)

            if ($plainPass -eq $plainConfirm) {
                $Password = $plainPass
                $passPrompting = $false
            } else {
                Write-Host "Passwords do not match. Please re-enter." -ForegroundColor Red
            }
        }
    }

    # Step 4: Installation & Data Directories
    Write-Banner
    Write-Host "--- STEP 4 of 4: Installation Paths ---" -ForegroundColor Yellow
    Write-Host ""

    Write-Host "Installation Directory [$InstallDir]: " -NoNewline -ForegroundColor Cyan
    $inInst = [Console]::ReadLine()
    if ($inInst) { $InstallDir = $inInst.Trim() }

    Write-Host "Database Store Directory [$DataDir]: " -NoNewline -ForegroundColor Cyan
    $inData = [Console]::ReadLine()
    if ($inData) { $DataDir = $inData.Trim() }

} else {
    # Parse silent components
    $compList = $Components -split '[, ]+'
    $selectedComponents["tem"] = $compList -contains "tem"
    $selectedComponents["temniond"] = $compList -contains "temniond"
    $selectedComponents["studio"] = $compList -contains "studio"
    $selectedComponents["tzeentch"] = $compList -contains "tzeentch"
    $selectedComponents["service"] = $compList -contains "service"

    if (-not $Password) {
        $Password = "temnion_secret_token"
    }
}

# ---------------------------------------------------------------------------
# Installation Execution & Provisioning
# ---------------------------------------------------------------------------

Write-Banner
Write-Host "Installing Temnion components..." -ForegroundColor Yellow

# 1. Create Directories
$binDir = "$InstallDir\bin"
$confDir = "$InstallDir\config"
New-Item -ItemType Directory -Path $binDir -Force | Out-Null
New-Item -ItemType Directory -Path $confDir -Force | Out-Null
New-Item -ItemType Directory -Path $DataDir -Force | Out-Null

# 2. Locate Source Binaries
$builtReleaseDir = "$repoRoot\target\release"
$debugReleaseDir = "$repoRoot\target\debug"

function Get-BinaryPath($name) {
    if (Test-Path "$builtReleaseDir\$name.exe") { return "$builtReleaseDir\$name.exe" }
    if (Test-Path "$debugReleaseDir\$name.exe") { return "$debugReleaseDir\$name.exe" }
    if (Test-Path "$PSScriptRoot\..\bin\$name.exe") { return "$PSScriptRoot\..\bin\$name.exe" }
    return $null
}

# Copy tem
if ($selectedComponents["tem"]) {
    $src = Get-BinaryPath "tem"
    if ($src) {
        Copy-Item $src "$binDir\tem.exe" -Force
        Write-Host "  [OK] Installed tem CLI -> $binDir\tem.exe" -ForegroundColor Green
    } else {
        Write-Warning "Source binary for 'tem' not found; compile with 'cargo build --release --bin tem'."
    }
}

# Copy temniond
if ($selectedComponents["temniond"]) {
    $src = Get-BinaryPath "temniond"
    if ($src) {
        Copy-Item $src "$binDir\temniond.exe" -Force
        Write-Host "  [OK] Installed temniond daemon -> $binDir\temniond.exe" -ForegroundColor Green
    } else {
        Write-Warning "Source binary for 'temniond' not found; compile with 'cargo build --release --bin temniond'."
    }
}

# Copy tzeentch
if ($selectedComponents["tzeentch"]) {
    $src = Get-BinaryPath "tzeentch"
    if ($src) {
        Copy-Item $src "$binDir\tzeentch.exe" -Force
        Write-Host "  [OK] Installed tzeentch client -> $binDir\tzeentch.exe" -ForegroundColor Green
    } else {
        Write-Warning "Source binary for 'tzeentch' not found; compile with 'cargo build --release --bin tzeentch'."
    }
}

# Copy Studio Workbench
if ($selectedComponents["studio"]) {
    $studioDest = "$InstallDir\studio-web"
    New-Item -ItemType Directory -Path $studioDest -Force | Out-Null
    $studioDist = "$repoRoot\apps\temnion-studio\dist"
    if (Test-Path $studioDist) {
        Copy-Item -Recurse -Force "$studioDist\*" $studioDest
        Write-Host "  [OK] Installed Temnion Studio Workbench assets -> $studioDest" -ForegroundColor Green
    }
}

# 3. Generate Authoritative temnion.toml
$daemonConfigFile = "$confDir\temnion.toml"
$dataDirEscaped = $DataDir -replace '\\', '/'
$tomlLines = @(
    '# Temnion Authoritative Server Configuration',
    '[database]',
    ('database_name = "' + $DbName + '"'),
    ('admin_user = "' + $Username + '"'),
    ('auth_token = "' + $Password + '"'),
    '',
    '[storage]',
    ('data_dir = "' + $dataDirEscaped + '"'),
    'source_id = 1',
    'source_epoch = 1',
    '',
    '[network]',
    'server_id = "temniond-primary"',
    ('tnp_bind = "' + $BindHost + ':' + $Port + '"'),
    ('flight_bind = "' + $BindHost + ':' + $FlightPort + '"'),
    'mcp_enabled = true',
    '',
    '[maintenance]',
    'maintenance_interval_secs = 60'
)
$tomlLines | Set-Content -Path $daemonConfigFile -Encoding utf8
Write-Host "  [OK] Generated server configuration -> $daemonConfigFile" -ForegroundColor Green

# 4. Generate Client Connection Profile (connections.toml)
$userTemnionDir = "$HOME\.temnion"
New-Item -ItemType Directory -Path $userTemnionDir -Force | Out-Null
$userConnFile = "$userTemnionDir\connections.toml"
$connLines = @(
    '# Temnion Client Connection Profile',
    '[default]',
    ('name = "Local Primary (' + $DbName + ')"'),
    ('host = "' + $BindHost + '"'),
    ('port = ' + $Port),
    ('flight_port = ' + $FlightPort),
    ('database = "' + $DbName + '"'),
    ('username = "' + $Username + '"'),
    ('auth_token = "' + $Password + '"')
)
$connLines | Set-Content -Path $userConnFile -Encoding utf8
$connLines | Set-Content -Path "$confDir\connections.toml" -Encoding utf8
Write-Host "  [OK] Configured client connection profile -> $userConnFile" -ForegroundColor Green

# 5. Initialize Store Directory
if ($selectedComponents["temniond"]) {
    $temniondBin = "$binDir\temniond.exe"
    if (Test-Path $temniondBin) {
        & $temniondBin init --config "$daemonConfigFile" --data-dir "$DataDir" | Out-Null
        Write-Host "  [OK] Initialized authoritative database store -> $DataDir" -ForegroundColor Green
    }
}

# 6. Service Registration
if ($selectedComponents["service"]) {
    if ($isAdmin) {
        $svcScript = "$PSScriptRoot\..\services\windows\install-service.ps1"
        if (Test-Path $svcScript) {
            & powershell -ExecutionPolicy Bypass -File $svcScript -BinPath "$binDir\temniond.exe" -ConfigPath "$daemonConfigFile"
            Write-Host "  [OK] Registered and started Windows Service (temniond)" -ForegroundColor Green
        }
    } else {
        Write-Warning "Skipped Windows Service registration (requires Administrator privileges)."
    }
}

# ---------------------------------------------------------------------------
# Setup Completion Summary
# ---------------------------------------------------------------------------

Write-Banner
Write-Host "================================================================================" -ForegroundColor Green
Write-Host "                    TEMNION INSTALLATION COMPLETED                              " -ForegroundColor Green
Write-Host "================================================================================" -ForegroundColor Green
Write-Host ""
Write-Host "Configuration Summary:" -ForegroundColor White
Write-Host "  Database Name:     $DbName"
Write-Host "  TNP Network Port:  $Port"
Write-Host "  Flight Port:       $FlightPort"
Write-Host "  Superuser:         $Username"
Write-Host "  Auth Token:        $Password"
Write-Host "  Connection URI:    temnion://$Username`:$Password@$BindHost`:$Port/$DbName" -ForegroundColor Cyan
Write-Host ""
Write-Host "Installed Paths:" -ForegroundColor White
Write-Host "  Binaries:          $binDir"
Write-Host "  Server Config:     $daemonConfigFile"
Write-Host "  Client Profile:    $userConnFile"
Write-Host "  Database Store:    $DataDir"
Write-Host ""
Write-Host "Quickstart Commands:" -ForegroundColor White
Write-Host "  1. Studio Workbench IDE: Open http://localhost:5173 or your desktop shortcut"
Write-Host "  2. Test Connection:      & '$binDir\tzeentch.exe' status"
Write-Host "  3. Query Database:       & '$binDir\tem.exe' query 'FROM temnion SELECT * LIMIT 10'"
Write-Host "  4. Start Daemon Manually: & '$binDir\temniond.exe' run --config '$daemonConfigFile'"
Write-Host ""
Write-Host "Thank you for installing Temnion." -ForegroundColor Green
Write-Host ""
