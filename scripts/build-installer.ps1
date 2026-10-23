# Compila o HomeUp em release e gera dist\HomeUp-<versão>-setup.exe
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
cargo build --release --target-dir target
if ($LASTEXITCODE -ne 0) { throw "cargo build falhou" }
$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw "Inno Setup 6 não encontrado (winget install JRSoftware.InnoSetup)" }
& $iscc "/DAppVersion=$version" installer\homeup.iss
if ($LASTEXITCODE -ne 0) { throw "ISCC falhou" }
Write-Host "Instalador: $root\dist\HomeUp-$version-setup.exe"
