<#
.SYNOPSIS
    Builds the debug bundle (no installer bundle) for yt-dlp-gui on Windows.
    PowerShell counterpart to build-debug.sh.
#>

[CmdletBinding()]
param(
    [switch]$SkipTypecheck,
    [switch]$SkipFrontend,
    [switch]$OpenOutput,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$ExtraArgs
)

$params = @{
    DebugBuild    = $true
    NoBundle      = $true
    SkipTypecheck = $SkipTypecheck
    SkipFrontend  = $SkipFrontend
    OpenOutput    = $OpenOutput
}

if ($ExtraArgs -and $ExtraArgs.Count -gt 0) {
    & "$PSScriptRoot\build-release.ps1" @params @ExtraArgs
} else {
    & "$PSScriptRoot\build-release.ps1" @params
}
