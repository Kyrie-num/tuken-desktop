# Tuken installer. Downloads are served by Cloudflare R2.
# irm https://download.tuken.site/install.ps1 | iex
[CmdletBinding()]
param([switch]$DownloadOnly)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

function Get-TukenFile([string]$Url, [string]$Path) {
    for ($attempt = 1; $attempt -le 4; $attempt++) {
        try {
            Invoke-WebRequest -Uri $Url -OutFile $Path -UseBasicParsing -TimeoutSec 120
            return
        } catch {
            if ($attempt -eq 4) { throw }
            Start-Sleep -Seconds 2
        }
    }
}

if (-not [Environment]::Is64BitOperatingSystem) {
    throw 'Tuken requires 64-bit Windows 10 or later.'
}
$base = 'https://download.tuken.site'
$folder = Join-Path ([IO.Path]::GetTempPath()) ('tuken-install-' + [Guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $folder
$manifest = Join-Path $folder 'latest.json'
Write-Host 'Checking the latest Tuken version...'
Get-TukenFile "$base/latest.json" $manifest
$latest = Get-Content -LiteralPath $manifest -Raw -Encoding UTF8 | ConvertFrom-Json
$platform = $latest.platforms.'windows-x86_64'
$url = [Uri]$platform.url
if ($url.Scheme -ne 'https' -or $url.Host -ne 'download.tuken.site' -or $url.Port -ne 443 -or $url.UserInfo -or $url.Query -or $url.Fragment) {
    throw 'The update manifest contains an unexpected download address.'
}
$fileName = [IO.Path]::GetFileName($url.AbsolutePath)
if ($url.AbsolutePath -notmatch '^/releases/v[0-9]+\.[0-9]+\.[0-9]+/Tuken_[0-9]+\.[0-9]+\.[0-9]+_x64-setup\.exe$') {
    throw 'The update manifest contains an unexpected installer name.'
}
if ($platform.sha256 -notmatch '^[0-9a-fA-F]{64}$' -or [long]$platform.size -le 0) {
    throw 'The update manifest is missing the installer checksum or size.'
}
$setup = Join-Path $folder $fileName
Write-Host "Downloading Tuken $($latest.version)..."
Get-TukenFile $url.AbsoluteUri $setup
$actual = (Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash
if ($actual -ne $platform.sha256 -or (Get-Item -LiteralPath $setup).Length -ne [long]$platform.size) {
    throw 'The downloaded installer failed verification. Please retry.'
}
if ($DownloadOnly) {
    Write-Output $setup
} else {
    Write-Host 'Download verified. Opening the installer...'
    Start-Process -FilePath $setup
}
