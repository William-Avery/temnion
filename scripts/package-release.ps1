# SPDX-License-Identifier: AGPL-3.0-only
<#
.SYNOPSIS
    Builds and packages Temnion release distributions.

.DESCRIPTION
    Compiles tem and temniond in release mode, collects binaries, documentation,
    service units, and configuration templates into a distribution archive with SHA256 checksums.

.PARAMETER Version
    Release version tag (e.g., 0.1.0). If omitted, read from Cargo.toml.

.PARAMETER Target
    Cargo compilation target architecture.

.PARAMETER OutputDir
    Directory where release packages are output. Defaults to dist/.

.PARAMETER SkipBuild
    If specified, skips cargo build step (packages existing binaries).
#>

[CmdletBinding()]
param(
    [string]$Version,
    [string]$Target,
    [string]$OutputDir = "$PSScriptRoot\..\dist",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

# Auto-detect version from root Cargo.toml if omitted
$repoRoot = (Resolve-Path "$PSScriptRoot\..").Path
if (-not $Version) {
    $cargoToml = Get-Content "$repoRoot\Cargo.toml" -Raw
    if ($cargoToml -match 'version\s*=\s*"([^"]+)"') {
        $Version = $matches[1]
    } else {
        $Version = "0.1.0"
    }
}

# Determine default target if not specified
if (-not $Target) {
    $Target = rustc -Vv | Select-String "host: (.*)" | ForEach-Object { $_.Matches.Groups[1].Value }
}

Write-Host "Packaging Temnion v$Version for $Target..." -ForegroundColor Cyan

# 1. Build release binaries
if (-not $SkipBuild) {
    Write-Host "Building release binaries (tem, temniond)..." -ForegroundColor Yellow
    $buildArgs = @("build", "--release", "--bin", "tem", "--bin", "temniond", "--locked")
    if ($Target) {
        $buildArgs += @("--target", $Target)
    }
    & cargo @buildArgs
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Cargo build failed with exit code $LASTEXITCODE."
        exit $LASTEXITCODE
    }
}

# 2. Locate built binaries
$targetDir = if ($Target) { "$repoRoot\target\$Target\release" } else { "$repoRoot\target\release" }
$exeSuffix = if ($Target -like "*windows*") { ".exe" } else { "" }

$temBin = "$targetDir\tem$exeSuffix"
$temniondBin = "$targetDir\temniond$exeSuffix"

if (-not (Test-Path $temBin)) {
    Write-Error "Missing binary: $temBin"
    exit 1
}
if (-not (Test-Path $temniondBin)) {
    Write-Error "Missing binary: $temniondBin"
    exit 1
}

# 3. Create stage directory
$packageBaseName = "temnion-v$Version-$Target"
$stageDir = "$OutputDir\$packageBaseName"
if (Test-Path $stageDir) {
    Remove-Item -Recurse -Force $stageDir
}

New-Item -ItemType Directory -Path "$stageDir\bin" -Force | Out-Null
New-Item -ItemType Directory -Path "$stageDir\config" -Force | Out-Null
New-Item -ItemType Directory -Path "$stageDir\services\systemd" -Force | Out-Null
New-Item -ItemType Directory -Path "$stageDir\services\windows" -Force | Out-Null
New-Item -ItemType Directory -Path "$stageDir\studio-web" -Force | Out-Null

# Copy binaries
Copy-Item $temBin "$stageDir\bin\"
Copy-Item $temniondBin "$stageDir\bin\"

# Copy Studio Desktop binary if built
$studioExe = "$repoRoot\apps\temnion-studio\src-tauri\target\release\temnion-studio$exeSuffix"
if (Test-Path $studioExe) {
    Copy-Item $studioExe "$stageDir\bin\"
}

# Copy Studio Web assets
if (Test-Path "$repoRoot\apps\temnion-studio\dist") {
    Copy-Item -Recurse -Force "$repoRoot\apps\temnion-studio\dist\*" "$stageDir\studio-web\"
}

# Copy configs and services
Copy-Item "$repoRoot\temnion.example.toml" "$stageDir\config\"
Copy-Item "$repoRoot\services\systemd\temniond.service" "$stageDir\services\systemd\"
Copy-Item "$repoRoot\services\windows\install-service.ps1" "$stageDir\services\windows\"
Copy-Item "$repoRoot\services\windows\uninstall-service.ps1" "$stageDir\services\windows\"

# Copy documentation and license
Copy-Item "$repoRoot\README.md" "$stageDir\"
Copy-Item "$repoRoot\CHANGELOG.md" "$stageDir\"
Copy-Item "$repoRoot\LICENSE" "$stageDir\" -ErrorAction SilentlyContinue

# 4. Create ZIP archive
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
$archivePath = "$OutputDir\$packageBaseName.zip"
if (Test-Path $archivePath) {
    Remove-Item -Force $archivePath
}

Write-Host "Creating archive: $archivePath" -ForegroundColor Yellow
Compress-Archive -Path "$stageDir\*" -DestinationPath $archivePath -Force

# 5. Compute SHA256 checksum
$hash = Get-FileHash -Path $archivePath -Algorithm SHA256
$hashFile = "$OutputDir\$packageBaseName.zip.sha256"
"$($hash.Hash)  $packageBaseName.zip" | Set-Content -Path $hashFile

Write-Host "Package successfully created!" -ForegroundColor Green
Write-Host "  Archive:  $archivePath"
Write-Host "  SHA256:   $($hash.Hash)"
Write-Host "  Checksum: $hashFile"
