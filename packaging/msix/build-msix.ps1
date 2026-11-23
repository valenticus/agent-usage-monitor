<#
.SYNOPSIS
  Builds the Microsoft Store (MSIX) package of AgentUsageMonitor, x64.

.DESCRIPTION
  1. Reads the version from rust/agent-monitor-ui/Cargo.toml (x.y.z -> x.y.z.0).
  2. Builds the release binaries (unless -SkipBuild), the same command as the
     Inno Setup script: cargo build --release --target x86_64-pc-windows-msvc.
  3. Stages the two EXEs, LICENSE, the manifest and the visual assets, which
     are resized from the official app icon (assets/icon.png).
  4. Indexes resources with makepri and packs with makeappx (Windows SDK).

  The package is NOT signed: Partner Center signs Store submissions. Nothing
  is installed, uninstalled or published, and no certificate is created.

.EXAMPLE
  pwsh -File packaging\msix\build-msix.ps1
  pwsh -File packaging\msix\build-msix.ps1 -SkipBuild -Jobs 2
  pwsh -File packaging\msix\build-msix.ps1 -ValidateManifestOnly

.NOTES
  -ValidateManifestOnly stages zero-byte placeholder EXEs in a temporary folder,
  then runs makepri and makeappx to validate the manifest, identity, assets and
  package layout without compiling Rust. The temporary package is deleted.
#>
[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [switch]$ValidateManifestOnly,
    [string]$OutputDirectory,
    [string]$TargetDirectory,
    [int]$Jobs = 0
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$rust = Join-Path $repo 'rust'
$target = if ($TargetDirectory) { [IO.Path]::GetFullPath($TargetDirectory) } else { Join-Path $rust 'target' }

# --- Version ---------------------------------------------------------------
$versionLine = Select-String -Path (Join-Path $rust 'agent-monitor-ui\Cargo.toml') -Pattern '^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"' | Select-Object -First 1
if (-not $versionLine) { throw 'Could not read a x.y.z version from agent-monitor-ui/Cargo.toml' }
$g = $versionLine.Matches[0].Groups
$version = "$($g[1].Value).$($g[2].Value).$($g[3].Value).0"   # Store requires revision 0
Write-Host "AgentUsageMonitor MSIX $version (x64)"

# --- Windows SDK tools -----------------------------------------------------
function Find-SdkTool([string]$name) {
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $tool = Get-ChildItem $kits -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
        Sort-Object { [version]$_.Name } -Descending |
        ForEach-Object { Join-Path $_.FullName "x64\$name" } |
        Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $tool) { throw "$name not found; install the Windows 10/11 SDK (winget install Microsoft.WindowsSDK.10.0.26100)" }
    $tool
}
$makeappx = Find-SdkTool 'makeappx.exe'
$makepri = Find-SdkTool 'makepri.exe'

# --- Output ----------------------------------------------------------------
if ($ValidateManifestOnly) {
    $out = Join-Path ([IO.Path]::GetTempPath()) ("aum-msix-validate-" + [guid]::NewGuid().ToString('N'))
} elseif ($OutputDirectory) {
    $out = [IO.Path]::GetFullPath($OutputDirectory)
} else {
    $out = Join-Path $target "msix-$version"
}
if (Test-Path $out) {
    if (Get-ChildItem -LiteralPath $out -Force | Select-Object -First 1) {
        throw "Output directory is not empty; choose a new directory: $out"
    }
} else {
    New-Item -ItemType Directory -Force -Path $out | Out-Null
}
$stage = Join-Path $out 'stage'
$assetsOut = Join-Path $stage 'Assets'
New-Item -ItemType Directory -Force -Path $assetsOut | Out-Null
$package = Join-Path $out "AgentUsageMonitor_${version}_x64.msix"

try {
    # --- Binaries ----------------------------------------------------------
    $exeNames = @('agent-usage-monitor.exe', 'agent-usage-monitor-claude-bridge.exe')
    if ($ValidateManifestOnly) {
        foreach ($name in $exeNames) { New-Item -ItemType File -Path (Join-Path $stage $name) | Out-Null }
        Write-Host 'Validation mode: zero-byte placeholder EXEs (no Rust build).'
    } else {
        if (-not $SkipBuild) {
            Push-Location $rust
            $previousTargetDirectory = $env:CARGO_TARGET_DIR
            try {
                $env:CARGO_TARGET_DIR = $target
                $cargoArgs = @('build', '--locked', '--release', '--target', 'x86_64-pc-windows-msvc',
                    '-p', 'agent-usage-monitor-ui', '-p', 'agent-usage-monitor-core', '--bins')
                if ($Jobs -gt 0) { $cargoArgs += @('-j', "$Jobs") }
                & cargo @cargoArgs
                if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
            } finally {
                $env:CARGO_TARGET_DIR = $previousTargetDirectory
                Pop-Location
            }
        }
        $release = Join-Path $target 'x86_64-pc-windows-msvc\release'
        foreach ($name in $exeNames) {
            $file = Join-Path $release $name
            if (-not (Test-Path $file)) { throw "Missing release binary: $file" }
            Copy-Item $file $stage
        }
    }
    Copy-Item (Join-Path $repo 'LICENSE') $stage

    # --- Manifest ----------------------------------------------------------
    $manifest = (Get-Content -Raw (Join-Path $PSScriptRoot 'AppxManifest.xml')).Replace('__VERSION__', $version)
    if ($manifest -match '__[A-Z]+__') { throw 'Unreplaced placeholder left in AppxManifest.xml' }
    [xml]$null = $manifest   # well-formed XML check
    [IO.File]::WriteAllText((Join-Path $stage 'AppxManifest.xml'), $manifest, (New-Object Text.UTF8Encoding $false))

    # --- VC++ runtime ------------------------------------------------------
    # The MSVC target links the CRT dynamically. VCRUNTIME140/MSVCP140 are not
    # part of Windows, so a package whose EXEs import them must declare the
    # VCLibs framework (the Store installs it). Import names are plain ASCII.
    if (-not $ValidateManifestOnly) {
        foreach ($name in $exeNames) {
            $text = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes((Join-Path $stage $name)))
            $runtime = [regex]::Match($text, '(?i)(VCRUNTIME|MSVCP)140[_0-9a-z]*\.dll')
            $declared = $manifest.Contains('Name="Microsoft.VCLibs.140.00.UWPDesktop"')
            if ($runtime.Success -and -not $declared) {
                throw "$name imports $($runtime.Value) but AppxManifest.xml has no Microsoft.VCLibs.140.00.UWPDesktop dependency"
            }
            if ($runtime.Success) { Write-Host "$name needs $($runtime.Value): VCLibs framework dependency declared." }
        }
    }

    # --- Visual assets from the official app icon --------------------------
    Add-Type -AssemblyName System.Drawing
    $icon = Join-Path $repo 'assets\icon.png'
    $source = [System.Drawing.Image]::FromFile($icon)
    try {
        function Save-Square([int]$size, [string]$name) {
            $bitmap = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
            $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
            try {
                $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
                $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
                $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
                $graphics.Clear([System.Drawing.Color]::Transparent)
                $graphics.DrawImage($source, 0, 0, $size, $size)
                $bitmap.Save((Join-Path $assetsOut $name), [System.Drawing.Imaging.ImageFormat]::Png)
            } finally {
                $graphics.Dispose(); $bitmap.Dispose()
            }
        }
        Save-Square 50 'StoreLogo.png'
        Save-Square 150 'Square150x150Logo.png'
        Save-Square 44 'Square44x44Logo.png'
        # Unplated taskbar / Start list icons (selected through resources.pri).
        foreach ($size in 16, 24, 32, 48, 256) {
            Save-Square $size "Square44x44Logo.targetsize-${size}_altform-unplated.png"
        }
    } finally {
        $source.Dispose()
    }

    # --- Guard: nothing that must not ship ---------------------------------
    $forbidden = Get-ChildItem $stage -Recurse -File | Where-Object {
        $_.Extension -in '.pdb', '.svg', '.json', '.ilk', '.exp', '.lib' -or
        $_.Name -match '(?i)codex|openai|blossom|spark|credential|token|settings'
    }
    if ($forbidden) { throw "Unexpected files in the package staging folder: $($forbidden.Name -join ', ')" }

    # --- Resources and package ---------------------------------------------
    $priConfig = Join-Path $out 'priconfig.xml'
    & $makepri createconfig /cf $priConfig /dq en-US /pv 10.0.0 /o | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "makepri createconfig failed ($LASTEXITCODE)" }
    & $makepri new /pr $stage /cf $priConfig /mn (Join-Path $stage 'AppxManifest.xml') /of (Join-Path $stage 'resources.pri') /o | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "makepri new failed ($LASTEXITCODE)" }
    & $makeappx pack /d $stage /p $package /h SHA256 /o
    if ($LASTEXITCODE -ne 0) { throw "makeappx pack failed ($LASTEXITCODE)" }

    # Package family name as Windows derives it: Crockford base32 of the first
    # 8 bytes of SHA-256 over the UTF-16LE Publisher. Checked against the
    # identity confirmed in Partner Center > Product identity, so an edited
    # Identity Name or Publisher fails the build (and CI) instead of producing
    # a package the Store would reject. Nothing depends on it at run time.
    $expectedPackageFamilyName = 'ValentynFetisov.AgentUsageMonitor_sfbqfhxtgjc58'
    [xml]$staged = Get-Content -Raw (Join-Path $stage 'AppxManifest.xml')
    $identity = $staged.Package.Identity
    $digest = [Security.Cryptography.SHA256]::Create().ComputeHash([Text.Encoding]::Unicode.GetBytes($identity.Publisher))
    $bits = (($digest[0..7] | ForEach-Object { [Convert]::ToString($_, 2).PadLeft(8, '0') }) -join '') + '0'
    $publisherId = -join (0..12 | ForEach-Object { '0123456789abcdefghjkmnpqrstvwxyz'[[Convert]::ToInt32($bits.Substring($_ * 5, 5), 2)] })
    $packageFamilyName = "$($identity.Name)_$publisherId"
    Write-Host "Package family name: $packageFamilyName"
    if ($packageFamilyName -cne $expectedPackageFamilyName) {
        throw "Package family name $packageFamilyName does not match the Partner Center identity $expectedPackageFamilyName"
    }
    # The rest of the Partner Center identity and the product branding, each
    # compared exactly (case-sensitive). Store ID 9N20DHG8SSS1 is not part of
    # the manifest; it is assigned by Partner Center to this identity.
    $expectedIdentity = [ordered]@{
        'Identity Name'                 = @($identity.Name, 'ValentynFetisov.AgentUsageMonitor')
        'Identity Publisher'            = @($identity.Publisher, 'CN=090E947F-DD2D-4592-BD86-2187AAA0F9BA')
        'Properties DisplayName'        = @($staged.Package.Properties.DisplayName, 'AgentUsageMonitor')
        'Properties PublisherDisplayName' = @($staged.Package.Properties.PublisherDisplayName, 'Valentyn Fetisov')
        'VisualElements DisplayName'    = @($staged.Package.Applications.Application.VisualElements.DisplayName, 'AgentUsageMonitor')
    }
    foreach ($field in $expectedIdentity.Keys) {
        $actual, $expected = $expectedIdentity[$field]
        if ("$actual" -cne $expected) {
            throw "$field is '$actual'; the Partner Center identity requires '$expected'"
        }
    }
    Write-Host 'Identity, display names and publisher display name match Partner Center.'

    $hash = (Get-FileHash $package -Algorithm SHA256).Hash
    if ($ValidateManifestOnly) {
        Write-Host "Manifest, identity, assets and layout validated by makepri + makeappx (placeholder EXEs)."
    } else {
        Write-Host ("{0}  {1}  ({2:N0} bytes)" -f $hash, (Split-Path $package -Leaf), (Get-Item $package).Length)
        Write-Host 'Unsigned Store upload package. Partner Center signs it on submission.'
    }
} finally {
    if ($ValidateManifestOnly -and (Test-Path $out)) { Remove-Item -LiteralPath $out -Recurse -Force }
}
