# Builds the release exe, then dist/Leirk-Setup-<version>.exe and dist/Leirk-<version>-portable.zip.
# Usage: powershell -ExecutionPolicy Bypass -File scripts/package.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"(.+)"').Matches[0].Groups[1].Value

cargo build --release
if ($LASTEXITCODE) { throw "cargo build failed" }

$dist = Join-Path $root "dist"
New-Item -ItemType Directory -Force $dist | Out-Null

# installer
$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw "Inno Setup 6 not found (winget install JRSoftware.InnoSetup)" }
& $iscc /Q "/DAppVersion=$version" installer\leirk.iss
if ($LASTEXITCODE) { throw "ISCC failed" }

# portable: the `portable` marker keeps the profile in .\data next to the exe
$stage = Join-Path $dist "Leirk-portable"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory $stage | Out-Null
Copy-Item target\release\leirk.exe $stage
Set-Content -Path (Join-Path $stage "portable") -Value "" -Encoding ascii
Set-Content -Path (Join-Path $stage "LISEZ-MOI.txt") -Encoding utf8 -Value @"
Leirk $version - version portable

Lancez leirk.exe. Rien n'est installé : favoris, historique et réglages sont rangés dans le dossier .\data
à côté de l'exe. Copiez le dossier entier pour emporter votre profil (clé USB, autre PC).
Supprimez le fichier "portable" pour utiliser le profil normal (%LOCALAPPDATA%\Leirk).

Nécessite Windows 10/11 x64 avec le runtime Microsoft Edge WebView2 (déjà présent sur Windows 11).
"@
$zip = Join-Path $dist "Leirk-$version-portable.zip"
Remove-Item $zip -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip
Remove-Item -Recurse -Force $stage

Get-ChildItem $dist | ForEach-Object { "{0}  {1:N1} Mo" -f $_.Name, ($_.Length / 1MB) }
