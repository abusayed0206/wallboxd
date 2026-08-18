<#
.SYNOPSIS
    Builds Wallboxd and packages it as an MSIX for local sideload testing.

.DESCRIPTION
    This does NOT replace Store submission - Partner Center re-signs whatever
    you upload with the Store's own certificate, so a manually-built .msix is
    only useful for local testing before you submit. Steps:
      1. cargo build --release (unless -SkipBuild)
      2. Stage AppxManifest.xml + Assets\ + Wallboxd.exe into packaging\build\
      3. makeappx pack -> packaging\build\Wallboxd.msix
      4. With -TestSign: generate (or reuse) a self-signed cert whose Subject
         matches the manifest's Publisher exactly, sign the package, and
         import the cert into CurrentUser\TrustedPeople.

    Trusting the cert is necessary but not sufficient to sideload locally -
    Windows also needs sideloading/Developer Mode enabled, which requires an
    elevated (Administrator) PowerShell and isn't done by this script. See
    packaging/README.md for the exact command.

.PARAMETER TestSign
    Generate/reuse a local test certificate and sign the package with it.

.PARAMETER SkipBuild
    Skip `cargo build --release` and package whatever is already at
    target\release\Wallboxd.exe.
#>
param(
    [switch]$TestSign,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$packagingDir = Join-Path $repoRoot "packaging"
$buildDir = Join-Path $packagingDir "build"
$stageDir = Join-Path $buildDir "stage"
$msixPath = Join-Path $buildDir "Wallboxd.msix"

function Find-SdkTool {
    param([string]$Name)
    $roots = @(
        "${env:ProgramFiles(x86)}\Windows Kits\10\bin",
        "${env:ProgramFiles}\Windows Kits\10\bin"
    ) | Where-Object { Test-Path $_ }
    foreach ($root in $roots) {
        $found = Get-ChildItem -Path $root -Recurse -Filter $Name -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match '\\x64\\' } |
            Sort-Object FullName -Descending |
            Select-Object -First 1
        if ($found) { return $found.FullName }
    }
    throw "$Name not found under Windows Kits - install the Windows SDK."
}

$makeappx = Find-SdkTool "makeappx.exe"
$signtool = Find-SdkTool "signtool.exe"

if (-not $SkipBuild) {
    Write-Host "==> cargo build --release"
    Push-Location $repoRoot
    try {
        cargo build --release --locked
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    } finally {
        Pop-Location
    }
}

$exePath = Join-Path $repoRoot "target\release\Wallboxd.exe"
if (-not (Test-Path $exePath)) {
    throw "$exePath not found - build it first (omit -SkipBuild)."
}

Write-Host "==> Staging package contents"
if (Test-Path $stageDir) { Remove-Item $stageDir -Recurse -Force }
New-Item -ItemType Directory -Path $stageDir -Force | Out-Null
Copy-Item $exePath -Destination $stageDir
Copy-Item (Join-Path $packagingDir "AppxManifest.xml") -Destination $stageDir
Copy-Item (Join-Path $packagingDir "Assets") -Destination $stageDir -Recurse

Write-Host "==> makeappx pack"
if (Test-Path $msixPath) { Remove-Item $msixPath -Force }
& $makeappx pack /d $stageDir /p $msixPath /o
if ($LASTEXITCODE -ne 0) { throw "makeappx failed" }
Write-Host "Built $msixPath"

if ($TestSign) {
    $publisher = "CN=8EA73F0A-0DC1-449F-A896-493A34649C49"
    $pfxPath = Join-Path $buildDir "WallboxdTestCert.pfx"
    $cerPath = Join-Path $buildDir "WallboxdTestCert.cer"

    $existing = Get-ChildItem Cert:\CurrentUser\My |
        Where-Object { $_.Subject -eq $publisher -and $_.NotAfter -gt (Get-Date) } |
        Select-Object -First 1

    if ($existing) {
        Write-Host "==> Reusing existing test certificate ($($existing.Thumbprint))"
        $cert = $existing
    } else {
        Write-Host "==> Generating self-signed test certificate for $publisher"
        $cert = New-SelfSignedCertificate `
            -Type Custom `
            -Subject $publisher `
            -KeyUsage DigitalSignature `
            -FriendlyName "Wallboxd local test cert (not for Store submission)" `
            -CertStoreLocation "Cert:\CurrentUser\My" `
            -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
        $pw = ConvertTo-SecureString -String ([Guid]::NewGuid().ToString()) -Force -AsPlainText
        Export-PfxCertificate -Cert $cert -FilePath $pfxPath -Password $pw | Out-Null
    }
    # Re-exported every run (cheap, no private key involved) so $cerPath is
    # always present for the elevated Root-store import step below, whether
    # or not the cert itself was freshly generated this run.
    Export-Certificate -Cert $cert -FilePath $cerPath | Out-Null

    Write-Host "==> signtool sign"
    & $signtool sign /fd SHA256 /sha1 $cert.Thumbprint /s My $msixPath
    if ($LASTEXITCODE -ne 0) { throw "signtool failed" }

    Write-Host "==> Trusting the cert for this user (Cert:\CurrentUser\TrustedPeople)"
    $trustedPeople = Get-Item Cert:\CurrentUser\TrustedPeople
    $store = New-Object System.Security.Cryptography.X509Certificates.X509Store($trustedPeople.Name, "CurrentUser")
    $store.Open("ReadWrite")
    $store.Add($cert)
    $store.Close()

    Write-Host ""
    Write-Host "Signed, and trusted for CurrentUser\TrustedPeople - but that alone is NOT enough"
    Write-Host "for Add-AppxPackage to accept a self-signed cert. Two more things, both needing an"
    Write-Host "elevated (Administrator) PowerShell, are required - see packaging/README.md:"
    Write-Host "  1. Enable sideloading (AllowDevelopmentWithoutDevLicense)"
    Write-Host "  2. Import $cerPath into LocalMachine\Root (Trusted Root CAs)"
    Write-Host "Then: Add-AppxPackage -Path `"$msixPath`""
}
