<#
.SYNOPSIS
    Builds the production release bundle for yt-dlp-gui on Windows.
    Alias / wrapper for build-release.ps1.

.DESCRIPTION
    See Get-Help .\scripts\build-release.ps1 for full details and parameters.
#>

[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('nsis', 'msi', 'all', 'none')]
    [string]$Bundle = 'nsis',

    [Alias('d')]
    [switch]$DebugBuild,

    [switch]$NoBundle,

    [switch]$NoSign,

    [switch]$SkipTypecheck,

    [switch]$SkipFrontend,

    [switch]$OpenOutput,

    [switch]$Clean,

    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$ExtraArgs
)

& "$PSScriptRoot\build-release.ps1" @PSBoundParameters

