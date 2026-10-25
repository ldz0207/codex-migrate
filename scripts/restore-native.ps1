param(
    [Parameter(Mandatory=$true)][string]$ThreadId,
    [string]$CodexHome = $(if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }),
    [string]$ExpectedName = '',
    [string]$Cwd = '',
    [string]$CodexExecutable = '',
    [switch]$AllowUnarchive
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
Import-Module (Join-Path $PSScriptRoot 'CodexNativeRestore.psm1') -Force
Invoke-CodexNativeRestore -ThreadId $ThreadId -CodexHome $CodexHome -ExpectedName $ExpectedName -Cwd $Cwd -CodexExecutable $CodexExecutable -AllowUnarchive:$AllowUnarchive | ConvertTo-Json -Depth 5
