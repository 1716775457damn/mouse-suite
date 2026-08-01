# Build portable release zip for Mouse Suite
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path "$root\Cargo.toml")) { $root = $PSScriptRoot }
Set-Location $root

Write-Host "==> cargo build --release"
cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$exe = Join-Path $root "target\release\mouse-suite.exe"
if (-not (Test-Path $exe)) {
    Write-Error "missing $exe"
}

$stamp = Get-Date -Format "yyyyMMdd"
$outDir = Join-Path $root "dist\mouse-suite-$stamp"
$zipPath = Join-Path $root "dist\mouse-suite-$stamp-win64.zip"

Remove-Item -Recurse -Force $outDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $outDir "data") | Out-Null

Copy-Item $exe (Join-Path $outDir "mouse-suite.exe")
if (Test-Path (Join-Path $root "config.toml")) {
    Copy-Item (Join-Path $root "config.toml") (Join-Path $outDir "config.toml")
}
Copy-Item (Join-Path $root "README.md") (Join-Path $outDir "README.md") -ErrorAction SilentlyContinue
Copy-Item (Join-Path $root "AGENT_BRIDGE.md") (Join-Path $outDir "AGENT_BRIDGE.md") -ErrorAction SilentlyContinue

@"
Mouse Suite — 首次运行

1. 双击 mouse-suite.exe
2. 数据保存在本目录 data\（元素库、文档会话、Agent 桥接文件）
3. 文档页：F8 开始/停止录制；Ctrl+Alt+F10 也可停止
4. AI 说明：文档页「AI 设置」配置智谱 / 自定义 / 本地 7999 代理

"@ | Set-Content -Encoding UTF8 (Join-Path $outDir "首次运行.txt")

Remove-Item $zipPath -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $outDir "*") -DestinationPath $zipPath -Force
Write-Host "==> packed: $zipPath"
Write-Host "==> folder: $outDir"
