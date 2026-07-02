<#
.SYNOPSIS
  Build the Windows-on-ARM (ARM64) setup installer (MSI) for Flight Tracker.

.DESCRIPTION
  1. Compiles a native aarch64-pc-windows-msvc release build of flight-tracker.exe.
  2. Ensures the standalone WiX v3.14 toolset is available (downloads it into a
     cache folder the first time; no admin / no install required).
  3. Runs candle + light to produce a per-machine ARM64 MSI in dist\windows-arm64.

  Run from anywhere; paths are resolved relative to this script's repo.

    powershell -ExecutionPolicy Bypass -File installer\build-installer.ps1
#>
[CmdletBinding()]
param(
    # Product version baked into the MSI file name (should match Cargo.toml).
    [string]$Version = '0.1.2'
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot

# 1) Build the native ARM64 release binary.
$Target = 'aarch64-pc-windows-msvc'
Write-Host "Building $Target release binary..." -ForegroundColor Cyan
Push-Location $RepoRoot
try {
    cargo build --release --target $Target
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
} finally { Pop-Location }

# Locate the produced exe (honour CARGO_TARGET_DIR if set, else repo 'target').
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $RepoRoot 'target' }
$Exe = Join-Path $TargetDir "$Target\release\flight-tracker.exe"
if (-not (Test-Path $Exe)) { throw "built exe not found at $Exe" }
Write-Host "Binary: $Exe" -ForegroundColor Green

# 2) Ensure WiX v3.14 binaries (candle/light) are available in a cache folder.
$WixCache = Join-Path $env:LOCALAPPDATA 'flight-tracker-build\wix314'
$Candle = Join-Path $WixCache 'candle.exe'
if (-not (Test-Path $Candle)) {
    Write-Host "Fetching WiX v3.14 toolset..." -ForegroundColor Cyan
    New-Item -ItemType Directory -Force -Path $WixCache | Out-Null
    $zip = Join-Path $WixCache 'wix314-binaries.zip'
    $url = 'https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip'
    Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
    Expand-Archive -Path $zip -DestinationPath $WixCache -Force
}
$Light = Join-Path $WixCache 'light.exe'
Write-Host "WiX: $WixCache" -ForegroundColor Green

# 3) Compile + link the MSI.
$OutDir = Join-Path $RepoRoot 'dist\windows-arm64'
$ObjDir = Join-Path $env:TEMP ('ft-wix-obj-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
New-Item -ItemType Directory -Force -Path $ObjDir | Out-Null
$Wxs = Join-Path $PSScriptRoot 'flight-tracker.wxs'
$Msi = Join-Path $OutDir "FlightTracker-$Version-arm64.msi"

Write-Host "Compiling (candle)..." -ForegroundColor Cyan
& $Candle -nologo -arch arm64 "-dExeSource=$Exe" -out "$ObjDir\" $Wxs
if ($LASTEXITCODE -ne 0) { throw "candle failed ($LASTEXITCODE)" }

Write-Host "Linking (light)..." -ForegroundColor Cyan
& $Light -nologo -ext WixUIExtension -cultures:en-us (Join-Path $ObjDir 'flight-tracker.wixobj') -out $Msi
if ($LASTEXITCODE -ne 0) { throw "light failed ($LASTEXITCODE)" }

Remove-Item $ObjDir -Recurse -Force -ErrorAction SilentlyContinue
Write-Host ""
Write-Host "Installer built: $Msi" -ForegroundColor Green
Get-Item $Msi | Select-Object FullName, @{n='SizeMB';e={[math]::Round($_.Length/1MB,2)}}
