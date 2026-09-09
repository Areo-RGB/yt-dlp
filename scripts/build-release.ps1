<#
.SYNOPSIS
    Builds the production release bundle for yt-dlp-gui on Windows.

.DESCRIPTION
    Automates the release build process for yt-dlp-gui:
    1. Validates prerequisites (Node.js, pnpm, Rust/Cargo).
    2. (Optional) Cleans previous build artifacts.
    3. Type-checks the frontend (vue-tsc).
    4. Builds the frontend assets (Vite).
    5. Builds the Tauri desktop application and packages installer bundles (NSIS / MSI).
    6. Displays the paths and file sizes of all generated artifacts.

.PARAMETER Bundle
    Installer bundle type to build. Options: 'nsis' (default), 'msi', 'all', 'none'.
    - 'nsis': Generates a standalone installer executable (*-setup.exe) [Default]
    - 'msi': Generates a Windows Installer package (*.msi)
    - 'all': Generates both NSIS and MSI installers
    - 'none': Skips installer bundling, only compiles the release binary

.PARAMETER DebugBuild
    Builds in debug mode instead of release mode. (Can also pass standard PowerShell -Debug switch).

.PARAMETER NoBundle
    Skips installer bundling (equivalent to -Bundle none).

.PARAMETER SkipTypecheck
    Skips the TypeScript / vue-tsc type-checking step.

.PARAMETER SkipFrontend
    Skips explicit pre-building of frontend assets.

.PARAMETER OpenOutput
    Opens the bundle output folder in Windows File Explorer upon completion.

.PARAMETER Clean
    Cleans previous dist/ and cargo build artifacts before building.

.EXAMPLE
    .\scripts\build-release.ps1
    Builds the standard production release with NSIS installer.

.EXAMPLE
    .\scripts\build-release.ps1 -Bundle all -OpenOutput
    Builds both NSIS and MSI installers, then reveals them in File Explorer.

.EXAMPLE
    .\scripts\build-release.ps1 -NoBundle
    Compiles the release .exe without creating an installer package.
#>

[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('nsis', 'msi', 'all', 'none')]
    [string]$Bundle = 'nsis',

    [Alias('d')]
    [switch]$DebugBuild,

    [switch]$NoBundle,

    [switch]$SkipTypecheck,

    [switch]$SkipFrontend,

    [switch]$OpenOutput,

    [switch]$Clean,

    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$ExtraArgs
)

$ErrorActionPreference = 'Stop'
$stopwatch = [System.Diagnostics.Stopwatch]::StartNew()

# Support both -DebugBuild and PowerShell's built-in -Debug switch
$isDebugging = $DebugBuild.IsPresent -or ($PSBoundParameters.ContainsKey('Debug') -and $PSBoundParameters['Debug'] -eq $true)

# Locate project root directory relative to this script
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir
Set-Location -LiteralPath $ProjectRoot

function Write-Section {
    param([string]$Message)
    Write-Host "`n==> $Message" -ForegroundColor Cyan
}

function Write-Success {
    param([string]$Message)
    Write-Host "  [OK] $Message" -ForegroundColor Green
}

function Write-WarnMsg {
    param([string]$Message)
    Write-Host "  [!] $Message" -ForegroundColor Yellow
}

function Test-Prerequisites {
    Write-Section "Checking build prerequisites..."

    $missing = @()

    if (-not (Get-Command 'node' -ErrorAction SilentlyContinue)) {
        $missing += "Node.js (https://nodejs.org/)"
    }
    if (-not (Get-Command 'pnpm' -ErrorAction SilentlyContinue)) {
        $missing += "pnpm (https://pnpm.io/installation)"
    }
    if (-not (Get-Command 'cargo' -ErrorAction SilentlyContinue)) {
        $missing += "Rust / Cargo (https://rustup.rs/)"
    }

    if ($missing.Count -gt 0) {
        Write-Host "`n[ERROR] Missing required build tools:" -ForegroundColor Red
        foreach ($tool in $missing) {
            Write-Host "  - $tool" -ForegroundColor Red
        }
        exit 1
    }

    $nodeVersion = node -v
    $pnpmVersion = pnpm -v
    $rustVersion = rustc --version
    Write-Host "  - Node.js: $nodeVersion" -ForegroundColor Gray
    Write-Host "  - pnpm:    $pnpmVersion" -ForegroundColor Gray
    Write-Host "  - Rust:    $rustVersion" -ForegroundColor Gray
    Write-Success "All prerequisites found"
}

function Invoke-CommandOrExit {
    param(
        [string]$Description,
        [scriptblock]$Action
    )
    Write-Section $Description
    & $Action
    if ($LASTEXITCODE -ne 0) {
        Write-Host "`n[ERROR] Step failed: $Description (Exit code: $LASTEXITCODE)" -ForegroundColor Red
        exit $LASTEXITCODE
    }
}

# --- Main Execution ---
Write-Host "==========================================" -ForegroundColor Magenta
Write-Host "   yt-dlp-gui Windows Release Builder     " -ForegroundColor Magenta
Write-Host "==========================================" -ForegroundColor Magenta
Write-Host "Target mode: $(if ($isDebugging) { 'Debug' } else { 'Release' })" -ForegroundColor Gray
Write-Host "Bundle type: $(if ($NoBundle -or $Bundle -eq 'none') { 'None (binary only)' } else { $Bundle })" -ForegroundColor Gray

Test-Prerequisites

# Clean previous build artifacts if requested
if ($Clean) {
    Write-Section "Cleaning previous build artifacts..."
    $distPath = Join-Path $ProjectRoot "dist"
    if (Test-Path -LiteralPath $distPath) {
        Remove-Item -LiteralPath $distPath -Recurse -Force
        Write-Host "  Removed $distPath" -ForegroundColor Gray
    }
    $targetDir = Join-Path $ProjectRoot "src-tauri\target"
    if (Test-Path -LiteralPath $targetDir) {
        Write-Host "  Cleaning Cargo target..." -ForegroundColor Gray
        Push-Location "src-tauri"
        try {
            cargo clean
        } finally {
            Pop-Location
        }
    }
    Write-Success "Clean completed"
}

# Step 1: Typecheck frontend
if (-not $SkipTypecheck) {
    Invoke-CommandOrExit "Type-checking frontend (vue-tsc)..." {
        pnpm typecheck
    }
} else {
    Write-WarnMsg "Skipping frontend type-checking (-SkipTypecheck)"
}

# Step 2: Build frontend assets
if (-not $SkipFrontend) {
    Invoke-CommandOrExit "Building frontend assets (Vite)..." {
        pnpm build
    }
} else {
    Write-WarnMsg "Skipping frontend build (-SkipFrontend)"
}

# Step 3: Build Tauri application
$tauriArgs = @('tauri', 'build')

if ($isDebugging) {
    $tauriArgs += '--debug'
}

if ($NoBundle -or $Bundle -eq 'none') {
    $tauriArgs += '--no-bundle'
} else {
    $bundleArg = switch ($Bundle) {
        'all'  { 'nsis,msi' }
        'nsis' { 'nsis' }
        'msi'  { 'msi' }
        default { $Bundle }
    }
    $tauriArgs += @('-b', $bundleArg)
}

if ($ExtraArgs) {
    $tauriArgs += $ExtraArgs
}

$bundleName = if ($NoBundle -or $Bundle -eq 'none') { "binary only (no installer)" } else { "$Bundle installer(s)" }
$profileName = if ($isDebugging) { "debug" } else { "production release" }

Invoke-CommandOrExit "Building Tauri $profileName bundle ($bundleName)..." {
    pnpm @tauriArgs
}

# Step 4: Report generated artifacts
$buildProfile = if ($isDebugging) { 'debug' } else { 'release' }
$targetBase = Join-Path $ProjectRoot "src-tauri\target\$buildProfile"
$bundleDir = Join-Path $targetBase "bundle"

Write-Section "Build Artifacts Summary"

$foundArtifacts = $false

# Search for installer bundles
if (Test-Path -LiteralPath $bundleDir) {
    $bundleFiles = Get-ChildItem -Path $bundleDir -Recurse -File | Where-Object {
        $_.Extension -in '.exe', '.msi', '.sig', '.zip'
    }

    if ($bundleFiles.Count -gt 0) {
        $foundArtifacts = $true
        Write-Host "`nInstallers and Update Artifacts:" -ForegroundColor Green
        foreach ($file in $bundleFiles) {
            $sizeMB = [math]::Round($file.Length / 1MB, 2)
            $relPath = Resolve-Path -LiteralPath $file.FullName -Relative
            Write-Host "  [OK] $($file.Name) " -NoNewline -ForegroundColor White
            Write-Host "($sizeMB MB)" -ForegroundColor Yellow
            Write-Host "       $relPath" -ForegroundColor DarkGray
        }
    }
}

# Check for the raw standalone executable
$rawExe = Join-Path $targetBase "yt-dlp-gui.exe"
if (-not (Test-Path -LiteralPath $rawExe)) {
    $rawExe = Join-Path $targetBase "YDL GUI.exe"
}
if (Test-Path -LiteralPath $rawExe) {
    $exeItem = Get-Item -LiteralPath $rawExe
    $sizeMB = [math]::Round($exeItem.Length / 1MB, 2)
    $relPath = Resolve-Path -LiteralPath $rawExe -Relative
    Write-Host "`nStandalone Executable:" -ForegroundColor Green
    Write-Host "  [OK] $($exeItem.Name) " -NoNewline -ForegroundColor White
    Write-Host "($sizeMB MB)" -ForegroundColor Yellow
    Write-Host "       $relPath" -ForegroundColor DarkGray
    $foundArtifacts = $true
}

if (-not $foundArtifacts) {
    Write-WarnMsg "No build artifacts detected in $targetBase"
}

$stopwatch.Stop()
$elapsed = $stopwatch.Elapsed.ToString("mm\:ss")
Write-Host "`n==========================================" -ForegroundColor Magenta
Write-Host "  Build completed successfully in $elapsed! " -ForegroundColor Green
Write-Host "==========================================" -ForegroundColor Magenta

# Step 5: Open output folder if requested
if ($OpenOutput) {
    $destination = if (Test-Path -LiteralPath $bundleDir) { $bundleDir } else { $targetBase }
    Write-Host "Opening output directory: $destination" -ForegroundColor Cyan
    Invoke-Item -LiteralPath $destination
}
