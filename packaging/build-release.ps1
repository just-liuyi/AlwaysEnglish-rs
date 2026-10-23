# Builds the release artifacts into .\dist:
#   AlwaysEnglish-<version>-portable-x64.exe   standalone executable
#   AlwaysEnglish-<version>-setup-x64.exe      Inno Setup installer
#   SHA256SUMS.txt
#
# Requires Rust (MSVC) and Inno Setup 6 (ISCC.exe on PATH, in Program Files,
# or in %LOCALAPPDATA%\Programs\InnoSetup6).

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
Write-Host "Building AlwaysEnglish $version"

cargo build --release
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

$iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
if (-not $iscc) {
    $iscc = @(
        "$env:LOCALAPPDATA\Programs\InnoSetup6\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $iscc) { throw 'ISCC.exe (Inno Setup 6) not found' }

$dist = Join-Path $root 'dist'
New-Item -ItemType Directory $dist -Force | Out-Null
Get-ChildItem $dist -File | Remove-Item

Copy-Item target\release\AlwaysEnglish.exe (Join-Path $dist "AlwaysEnglish-$version-portable-x64.exe")

& $iscc /Q "/DAppVersion=$version" packaging\AlwaysEnglish.iss
if ($LASTEXITCODE -ne 0) { throw 'ISCC failed' }

Get-ChildItem $dist -Filter *.exe | Sort-Object Name | ForEach-Object {
    "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
} | Set-Content (Join-Path $dist 'SHA256SUMS.txt') -Encoding ascii

Get-ChildItem $dist | Format-Table Name, @{ n = 'Size (MB)'; e = { '{0:N1}' -f ($_.Length / 1MB) } }
