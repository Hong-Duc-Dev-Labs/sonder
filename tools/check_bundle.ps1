<#
.SYNOPSIS
Checks a Sonder release bundle for files that must never ship (Roon filter tables).

.DESCRIPTION
Builds or inspects the Tauri bundle and fails when any file in it matches a Roon table: the
bundle folder, the files staged for the installer (resources and UI assets) and, on Windows,
the contents of each MSI, unpacked with an administrative install. Patterns:
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

# Resources are staged beside the executable before the installer is packed (`bundle.resources`
# lands in subfolders there); the UI assets must be clean too. Cargo's own build folders hold
# no shipped files and are skipped.
$skip = @("deps", "build", ".fingerprint", "incremental", "examples", "bundle", "wix", "nsis")
$staged = @(
    (Join-Path $repo "target/release"),
    (Join-Path $repo "app/src-tauri/target/release"),
    (Join-Path $repo "app/dist")
) | Where-Object { Test-Path $_ }

foreach ($root in $staged) {
    $files = @(Get-ChildItem -Path $root -File)
    foreach ($dir in Get-ChildItem -Path $root -Directory) {
        if ($skip -notcontains $dir.Name) {
            $files += Get-ChildItem -Path $dir.FullName -Recurse -File
        }
    }
    foreach ($file in $files) {
        if (& $forbidden $file) {
            Write-Host "  FORBIDDEN: $($file.FullName)" -ForegroundColor Red
            $failed = $true
        }
    }
}

# An installer is a compressed archive, so file names inside it are invisible to the scan
# above. On Windows an administrative install unpacks each MSI without installing it, and the
# unpacked tree is checked file by file. The NSIS setup .exe packs the same staged files.
if ($IsWindows -or $env:OS -eq "Windows_NT") {
    foreach ($root in $candidates) {
        foreach ($msi in Get-ChildItem -Path $root -Recurse -File -Filter "*.msi") {
            $unpacked = Join-Path ([System.IO.Path]::GetTempPath()) ("sonder-bundle-" + [guid]::NewGuid())
            Write-Host "Unpacking $($msi.Name)"
            $process = Start-Process msiexec.exe -Wait -PassThru -ArgumentList @(
                "/a", "`"$($msi.FullName)`"", "/qn", "TARGETDIR=`"$unpacked`""
            )
            try {
                if ($process.ExitCode -ne 0) {
                    Write-Host "  could not unpack $($msi.Name) (msiexec $($process.ExitCode))" -ForegroundColor Red
                    $failed = $true
                    continue
                }
                foreach ($file in Get-ChildItem -Path $unpacked -Recurse -File) {
                    if (& $forbidden $file) {
                        Write-Host "  FORBIDDEN in $($msi.Name): $($file.FullName)" -ForegroundColor Red
                        $failed = $true
                    }
                }
            } finally {
                Remove-Item -Recurse -Force $unpacked -ErrorAction SilentlyContinue
            }
        }
    }
}

if ($failed) {
    Write-Error "The bundle contains Roon data; do not publish it."
}

Write-Host "Bundle check passed: no Roon filter files." -ForegroundColor Green
