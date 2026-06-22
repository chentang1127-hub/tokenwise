# TokenWise Core — Install script for Windows
# Usage (PowerShell as Administrator):
#   irm https://raw.githubusercontent.com/chentang1127-hub/tokenwise/main/install.ps1 | iex

param(
    [string]$Version = "latest",
    [string]$InstallDir = "$env:LOCALAPPDATA\tokenwise"
)

$ErrorActionPreference = "Stop"

Write-Host "⚡ TokenWise Core — Self-hosted LLM execution layer installer" -ForegroundColor Green
Write-Host ""

# ── Detect arch ────────────────────────────────────────
$Arch = if ([Environment]::Is64BitOperatingSystem) { "x86_64" } else { "i686" }
if ($Arch -eq "i686") {
    Write-Host "Warning: 32-bit Windows is not officially supported. Try x86_64." -ForegroundColor Yellow
}

# ── Build URL ───────────────────────────────────────────
$Repo = "chentang1127-hub/tokenwise"
if ($Version -eq "latest") {
    $Url = "https://github.com/$Repo/releases/latest/download/tokenwise-windows-$Arch.zip"
} else {
    $Url = "https://github.com/$Repo/releases/download/$Version/tokenwise-windows-$Arch.zip"
}

Write-Host "Downloading TokenWise Core $Version for Windows/$Arch..."
$TmpDir = Join-Path $env:TEMP "tokenwise_$(Get-Random)"
New-Item -ItemType Directory -Force -Path $TmpDir | Out-Null
$ZipFile = Join-Path $TmpDir "tokenwise.zip"

try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -Uri $Url -OutFile $ZipFile
    Expand-Archive -Path $ZipFile -DestinationPath $TmpDir -Force
} catch {
    Write-Host "Download failed: $_" -ForegroundColor Red
    Write-Host "URL: $Url" -ForegroundColor Red
    exit 1
}

# ── Install ──────────────────────────────────────────────
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$exe = Get-ChildItem -Path $TmpDir -Recurse -Name "tokenwise.exe" | Select-Object -First 1
if (-not $exe) {
    Write-Host "Error: tokenwise.exe not found in the downloaded archive." -ForegroundColor Red
    exit 1
}
$src = Join-Path $TmpDir $exe
$dst = Join-Path $InstallDir "tokenwise.exe"
Copy-Item -Path $src -Destination $dst -Force
Write-Host "  ✓ Installed to $dst" -ForegroundColor Green

# ── PATH check ──────────────────────────────────────────
$currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($currentPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$currentPath;$InstallDir", "User")
    Write-Host "  ✓ Added to user PATH (restart terminal to use from anywhere)" -ForegroundColor Green
}

# ── Config ──────────────────────────────────────────────
$ConfigDir = Join-Path $InstallDir "config"
New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null
$ConfigFile = Join-Path $ConfigDir "config.yaml"
if (-not (Test-Path $ConfigFile)) {
    @'
# TokenWise Core configuration
locale: "en"

proxy:
  listen: "127.0.0.1:9401"
  admin: "127.0.0.1:9400"
  timeout_secs: 120

providers:
  - name: "deepseek"
    base_url: "https://api.deepseek.com/v1"
    api_key_env: "DEEPSEEK_API_KEY"
    models:
      - id: "deepseek-chat"
        tier: "cheap"
        cost_per_1k_prompt: 0.00027
        cost_per_1k_completion: 0.0011
      - id: "deepseek-reasoner"
        tier: "premium"
        cost_per_1k_prompt: 0.00055
        cost_per_1k_completion: 0.00219

  - name: "openrouter"
    base_url: "https://openrouter.ai/api/v1"
    api_key_env: "OPENROUTER_API_KEY"
    models:
      - id: "openai/gpt-4.1-mini"
        tier: "mid"
        cost_per_1k_prompt: 0.0004
        cost_per_1k_completion: 0.0016
      - id: "google/gemini-2.5-flash"
        tier: "cheap"
        cost_per_1k_prompt: 0.00015
        cost_per_1k_completion: 0.0006

routing:
  simple_max_tokens: 300
  complex_min_tokens: 1500
  simple_keywords: ["summarize", "translate", "extract", "classify", "what is", "define", "list", "convert"]
  complex_keywords: ["step by step", "think carefully", "reason about", "debug", "implement", "write code", "refactor", "design"]
  tier_simple: "cheap"
  tier_complex: "premium"
  tier_default: "mid"

safety_net:
  enabled: true
  max_fallback_retries: 1
  fallback_map:
    cheap: "mid"
    mid: "premium"

license:
  key: "tw_free"

cache:
  ttl_hours: 24
  max_entries: 10000

storage:
  db_path: "./tokenwise.db"
  retention_days: 90

budget:
  daily_limit_usd: 0
  monthly_limit_usd: 0
'@ | Out-File -FilePath $ConfigFile -Encoding utf8
    Write-Host "  ✓ Created default config at $ConfigFile" -ForegroundColor Green
}

# ── Cleanup ──────────────────────────────────────────────
Remove-Item -Recurse -Force $TmpDir

Write-Host ""
Write-Host "Done!" -ForegroundColor Green
Write-Host ""
Write-Host "  1. Edit config:  $ConfigFile" -ForegroundColor White
Write-Host "  2. Start:        tokenwise start --config $ConfigFile" -ForegroundColor White
Write-Host "  3. Dashboard:    http://127.0.0.1:9400" -ForegroundColor White
Write-Host ""
Write-Host "  Set your app's API base URL to: http://127.0.0.1:9401/v1" -ForegroundColor Yellow
Write-Host ""
