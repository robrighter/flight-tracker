<#
.SYNOPSIS
  Build a Microsoft Store-ready MSIX bundle for Flight Tracker (x64 + arm64).

.DESCRIPTION
  1. Compiles release binaries for x86_64-pc-windows-msvc and
     aarch64-pc-windows-msvc.
  2. Stages a per-architecture package layout (exe + Assets + manifest) under
     packaging\layout\<arch>.
  3. Packs each layout into a .msix with makeappx, then bundles both into a
     single .msixbundle so the Store serves the right native binary per device.
  4. Signs the bundle with a self-signed test certificate whose Subject
     matches the registered Publisher exactly (required for the package to be
     valid). This script creates the certificate in Cert:\CurrentUser\My only —
     it never touches the Trusted Root store or Developer Mode, so it does not
     change what your machine trusts. To locally sideload-test the signed
     bundle before submitting, you still need to either trust that certificate
     yourself or enable Developer Mode; see packaging/STORE_SUBMISSION.md.

  Run from anywhere; paths are resolved relative to this script's repo.

    powershell -File packaging\build-msix.ps1
#>
[CmdletBinding()]
param(
    # MSIX package version (must be 4 parts, last part must be 0).
    [string]$Version = '1.0.0.0',
    [string]$CertSubject = 'CN=231AD612-7448-4A51-A946-11B802E9219B'
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot
$PackagingDir = Join-Path $RepoRoot 'packaging'

# Locate the Windows SDK packaging tools (any host-arch subfolder works for
# any target package arch; x64 is the most universally present).
$SdkBinRoot = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin' -Directory |
    Where-Object { $_.Name -match '^10\.' } |
    Sort-Object Name -Descending |
    Select-Object -First 1
if (-not $SdkBinRoot) { throw "Windows 10 SDK bin directory not found under Windows Kits\10\bin" }
$MakeAppx = Join-Path $SdkBinRoot.FullName 'x64\makeappx.exe'
$SignTool = Join-Path $SdkBinRoot.FullName 'x64\signtool.exe'
if (-not (Test-Path $MakeAppx)) { throw "makeappx.exe not found at $MakeAppx" }
if (-not (Test-Path $SignTool)) { throw "signtool.exe not found at $SignTool" }
Write-Host "SDK tools: $($SdkBinRoot.FullName)\x64" -ForegroundColor Green

# 1) Build both architectures.
$Targets = @{
    'x64'   = 'x86_64-pc-windows-msvc'
    'arm64' = 'aarch64-pc-windows-msvc'
}
Push-Location $RepoRoot
try {
    foreach ($arch in $Targets.Keys) {
        $rustTarget = $Targets[$arch]
        Write-Host "Building $rustTarget release binary..." -ForegroundColor Cyan
        cargo build --release --target $rustTarget
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $rustTarget ($LASTEXITCODE)" }
    }
} finally { Pop-Location }

# 2) Stage a per-architecture layout and pack each into a .msix.
$LayoutRoot = Join-Path $PackagingDir 'layout'
$OutDir = Join-Path $RepoRoot 'dist\store'
Remove-Item $LayoutRoot -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $LayoutRoot | Out-Null
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$ManifestTemplate = Get-Content (Join-Path $PackagingDir 'AppxManifest.template.xml') -Raw
$MsixPaths = @()

foreach ($arch in $Targets.Keys) {
    $rustTarget = $Targets[$arch]
    $archLayout = Join-Path $LayoutRoot $arch
    New-Item -ItemType Directory -Force -Path $archLayout | Out-Null

    $exe = Join-Path $RepoRoot "target\$rustTarget\release\flight-tracker.exe"
    if (-not (Test-Path $exe)) { throw "built exe not found at $exe" }
    Copy-Item $exe (Join-Path $archLayout 'flight-tracker.exe')
    Copy-Item (Join-Path $PackagingDir 'Assets') (Join-Path $archLayout 'Assets') -Recurse

    $manifest = $ManifestTemplate.Replace('{ARCH}', $arch).Replace('{VERSION}', $Version)
    Set-Content -Path (Join-Path $archLayout 'AppxManifest.xml') -Value $manifest -Encoding UTF8

    $msix = Join-Path $OutDir "FlightTracker-$Version-$arch.msix"
    if (Test-Path $msix) { Remove-Item $msix -Force }
    Write-Host "Packing $arch -> $msix" -ForegroundColor Cyan
    & $MakeAppx pack /d $archLayout /p $msix /o
    if ($LASTEXITCODE -ne 0) { throw "makeappx pack failed for $arch ($LASTEXITCODE)" }
    $MsixPaths += $msix
}

# 3) Bundle both architectures into one .msixbundle.
$BundleDir = Join-Path $LayoutRoot 'bundle-input'
New-Item -ItemType Directory -Force -Path $BundleDir | Out-Null
foreach ($msix in $MsixPaths) { Copy-Item $msix $BundleDir }
$Bundle = Join-Path $OutDir "FlightTracker-$Version.msixbundle"
if (Test-Path $Bundle) { Remove-Item $Bundle -Force }
Write-Host "Bundling -> $Bundle" -ForegroundColor Cyan
& $MakeAppx bundle /d $BundleDir /p $Bundle
if ($LASTEXITCODE -ne 0) { throw "makeappx bundle failed ($LASTEXITCODE)" }

# 4) Sign with a self-signed cert whose Subject matches the Publisher exactly.
# Stored in Cert:\CurrentUser\My only; not trusted system-wide by this script.
$existingCert = Get-ChildItem Cert:\CurrentUser\My |
    Where-Object { $_.Subject -eq $CertSubject -and $_.HasPrivateKey } |
    Sort-Object NotAfter -Descending | Select-Object -First 1
if ($existingCert) {
    $cert = $existingCert
    Write-Host "Reusing existing signing certificate (thumbprint $($cert.Thumbprint))" -ForegroundColor Green
} else {
    Write-Host "Creating self-signed signing certificate for $CertSubject..." -ForegroundColor Cyan
    $cert = New-SelfSignedCertificate -Type Custom -Subject $CertSubject `
        -KeyUsage DigitalSignature -FriendlyName 'Flight Tracker MSIX test signing' `
        -CertStoreLocation 'Cert:\CurrentUser\My' `
        -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}false') `
        -NotAfter (Get-Date).AddYears(3)
}

Write-Host "Signing bundle..." -ForegroundColor Cyan
& $SignTool sign /fd SHA256 /s My /sha1 $cert.Thumbprint /v $Bundle
if ($LASTEXITCODE -ne 0) { throw "signtool sign failed ($LASTEXITCODE)" }

Write-Host ""
Write-Host "Store package ready: $Bundle" -ForegroundColor Green
Get-Item $Bundle | Select-Object FullName, @{n = 'SizeMB'; e = { [math]::Round($_.Length / 1MB, 2) } }
Write-Host "Signing certificate thumbprint: $($cert.Thumbprint)" -ForegroundColor Yellow
Write-Host "Not trusted locally yet - see packaging/STORE_SUBMISSION.md for local sideload testing." -ForegroundColor Yellow
