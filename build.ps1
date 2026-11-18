# Builds the Encounter Designer in both profiles: debug, then release.
#
#   .\build.ps1            both builds
#   .\build.ps1 -Test      run the tests first, and build only if they pass
#
# Cargo builds one profile per command, so this runs it twice. Extra arguments
# go to both builds (for example .\build.ps1 --locked).

param(
    [switch]$Test,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CargoArgs = @()
)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

function Invoke-Cargo([string[]]$Arguments) {
    Write-Host "> cargo $($Arguments -join ' ')" -ForegroundColor Cyan
    & cargo @Arguments
    if ($LASTEXITCODE -ne 0) {
        Write-Host "cargo $($Arguments[0]) failed (exit $LASTEXITCODE)" -ForegroundColor Red
        exit $LASTEXITCODE
    }
}

if ($Test) {
    Invoke-Cargo (@('test') + $CargoArgs)
}
Invoke-Cargo (@('build') + $CargoArgs)
Invoke-Cargo (@('build', '--release') + $CargoArgs)

Write-Host ''
foreach ($profile in 'debug', 'release') {
    $exe = Join-Path $PSScriptRoot "target\$profile\encounter-designer.exe"
    if (Test-Path $exe) {
        $item = Get-Item $exe
        Write-Host ("{0,-8} {1}  ({2:N1} MB, {3:g})" -f $profile, $item.FullName, ($item.Length / 1MB), $item.LastWriteTime)
    }
}
