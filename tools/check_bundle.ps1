<#
.SYNOPSIS
Checks a Sonder release bundle for files that must never ship (Roon filter tables).

.DESCRIPTION
Builds or inspects the Tauri bundle and fails when any file inside matches a Roon table:
`polyphase_*`, a filters folder's `*.csv`, `filters_manifest.json` or `*.decrypted`. The same
patterns are checked in git by scripts/check-forbidden-files.sh.

.EXAMPLE
pwsh tools/check_bundle.ps1
pwsh tools/check_bundle.ps1 -Build
#>
param(
    [switch]$Build,
    [string]$BundlePath = ""
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

if ($Build) {
    Write-Host "Building the release bundle (pnpm tauri build)..."
    Push-Location app
    try {
        pnpm tauri build
        if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }
    } finally {
        Pop-Location
    }
}

$candidates = @(
    $BundlePath,
    (Join-Path $repo "target/release/bundle"),
    (Join-Path $repo "app/src-tauri/target/release/bundle")
) | Where-Object { $_ -and (Test-Path $_) }

if (-not $candidates) {
    Write-Error "No bundle found. Run with -Build, or pass -BundlePath."
}

$forbidden = {
    param([System.IO.FileInfo]$file)
    $name = $file.Name.ToLowerInvariant()
    if ($name.StartsWith("polyphase_")) { return $true }
    if ($name -eq "filters_manifest.json") { return $true }
    if ($name.EndsWith(".decrypted")) { return $true }
    if ($name.EndsWith(".csv") -and $file.FullName -match "filter") { return $true }
    return $false
}

$failed = $false
foreach ($root in $candidates) {
    Write-Host "Inspecting $root"
    $files = Get-ChildItem -Path $root -Recurse -File
    Write-Host ("  {0} files, {1:N1} MB" -f $files.Count, (($files | Measure-Object Length -Sum).Sum / 1MB))
    foreach ($file in $files) {
        if (& $forbidden $file) {
            Write-Host "  FORBIDDEN: $($file.FullName)" -ForegroundColor Red
            $failed = $true
        }
    }
}

# Resources are staged beside the executable before the installer is packed; the UI assets and
# any `bundle.resources` files must be clean too.
$staged = @(
    (Join-Path $repo "target/release"),
    (Join-Path $repo "app/src-tauri/target/release"),
    (Join-Path $repo "app/dist")
) | Where-Object { Test-Path $_ }

foreach ($root in $staged) {
    foreach ($file in Get-ChildItem -Path $root -File -ErrorAction SilentlyContinue) {
        if (& $forbidden $file) {
            Write-Host "  FORBIDDEN: $($file.FullName)" -ForegroundColor Red
            $failed = $true
        }
    }
}
$dist = Join-Path $repo "app/dist"
if (Test-Path $dist) {
    foreach ($file in Get-ChildItem -Path $dist -Recurse -File) {
        if (& $forbidden $file) {
            Write-Host "  FORBIDDEN: $($file.FullName)" -ForegroundColor Red
            $failed = $true
        }
    }
}

if ($failed) {
    Write-Error "The bundle contains Roon data; do not publish it."
}

Write-Host "Bundle check passed: no Roon filter files." -ForegroundColor Green
