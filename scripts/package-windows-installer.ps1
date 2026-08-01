# Stage files and compile Inno Setup installer.
# Usage: package-windows-installer.ps1 [-Binary path] [-Version 0.2.3]
param(
    [string]$Binary = "",
    [string]$Version = "0.0.0"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$Version = $Version.TrimStart("v")
if (-not $Binary) {
    $candidates = @(
        "target\x86_64-pc-windows-msvc\release\mouse-suite.exe",
        "target\release\mouse-suite.exe"
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) { $Binary = (Resolve-Path $c).Path; break }
    }
}
if (-not $Binary -or -not (Test-Path $Binary)) {
    throw "mouse-suite.exe not found; build release first"
}

$stage = Join-Path $root "dist\installer-staging"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $stage | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $stage "data") | Out-Null

Copy-Item $Binary (Join-Path $stage "mouse-suite.exe")
Copy-Item (Join-Path $root "config.toml") $stage -ErrorAction SilentlyContinue
Copy-Item (Join-Path $root "README.md") $stage -ErrorAction SilentlyContinue
Copy-Item (Join-Path $root "AGENT_BRIDGE.md") $stage -ErrorAction SilentlyContinue
Copy-Item (Join-Path $root "LICENSE") $stage -ErrorAction SilentlyContinue
if (Test-Path (Join-Path $root "workflows")) {
    Copy-Item (Join-Path $root "workflows") (Join-Path $stage "workflows") -Recurse -Force
}

@"
Mouse Suite — 安装后

1. 从开始菜单或桌面快捷方式启动
2. 数据目录：安装目录下的 data\
3. 卸载：设置 → 应用，或开始菜单卸载项
"@ | Set-Content -Encoding UTF8 (Join-Path $stage "首次运行.txt")

$iscc = @(
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "${env:LocalAppData}\Programs\Inno Setup 6\ISCC.exe",
    "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $iscc) {
    throw "Inno Setup 6 (ISCC.exe) not found. Install from https://jrsoftware.org/isinfo.php"
}

$iss = Join-Path $root "packaging\windows\mouse-suite.iss"
& $iscc "/DMyAppVersion=$Version" $iss
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$setup = Join-Path $root "dist\Mouse-Suite-$Version-windows-x86_64-setup.exe"
if (-not (Test-Path $setup)) {
    throw "expected installer missing: $setup"
}
Write-Host "==> installer: $setup"
