# 兔啃一键安装：下载 GitHub 上最新的安装包并打开。在 PowerShell 里运行：
#   irm https://raw.githubusercontent.com/Kyrie-num/tuken-desktop/main/install.ps1 | iex

$ErrorActionPreference = "Stop"
# Windows PowerShell 5.1 downloads far slower while it draws a progress bar.
$ProgressPreference = "SilentlyContinue"
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

# The updater's manifest names the newest installer; no GitHub API, so no
# rate limit for a campus behind one address.
$base = "https://github.com/Kyrie-num/tuken-desktop/releases/latest/download"
$manifest = Join-Path $env:TEMP "tuken-latest.json"
Write-Host "正在查找最新版本..."
Invoke-WebRequest "$base/latest.json" -OutFile $manifest -UseBasicParsing
$latest = Get-Content $manifest -Raw -Encoding UTF8 | ConvertFrom-Json
$url = $latest.platforms."windows-x86_64".url

$setup = Join-Path $env:TEMP ([IO.Path]::GetFileName($url))
Write-Host "正在下载兔啃 $($latest.version)..."
Invoke-WebRequest $url -OutFile $setup -UseBasicParsing
Write-Host "下载好了，正在打开安装程序"
Start-Process $setup
