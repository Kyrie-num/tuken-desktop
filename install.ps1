# 兔啃一键安装：下载最新的安装包并打开。在 PowerShell 里运行：
#   irm https://fastly.jsdelivr.net/gh/Kyrie-num/tuken-desktop@main/install.ps1 | iex

$ErrorActionPreference = "Stop"
# Windows PowerShell 5.1 downloads far slower while it draws a progress bar.
$ProgressPreference = "SilentlyContinue"
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

# jsDelivr serves this file without a charset, so Windows PowerShell reads its
# UTF-8 as Latin-1. Such text is turned back before it is shown.
function Say([string]$text) {
  if ($text -notmatch '[^\x00-\xFF]') {
    $text = [Text.Encoding]::UTF8.GetString([Text.Encoding]::GetEncoding(28591).GetBytes($text))
  }
  Write-Host $text
}

# GitHub is often unreliable without a proxy in mainland China: a few tries.
function Get-File([string]$url, [string]$path) {
  for ($i = 1; ; $i++) {
    try { Invoke-WebRequest $url -OutFile $path -UseBasicParsing; return }
    catch { if ($i -ge 4) { throw } Start-Sleep -Seconds 2 }
  }
}

# The updater's manifest names the newest installer; no GitHub API, so no
# rate limit for a campus behind one address.
$base = "https://github.com/Kyrie-num/tuken-desktop/releases/latest/download"
$manifest = Join-Path $env:TEMP "tuken-latest.json"
Say "正在查找最新版本..."
Get-File "$base/latest.json" $manifest
$latest = Get-Content $manifest -Raw -Encoding UTF8 | ConvertFrom-Json
$url = $latest.platforms."windows-x86_64".url

$setup = Join-Path $env:TEMP ([IO.Path]::GetFileName($url))
Say "正在下载兔啃 $($latest.version)..."
Get-File $url $setup
Say "下载好了，正在打开安装程序"
Start-Process $setup
