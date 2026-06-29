$ErrorActionPreference = "Stop"

$Repo = "ShiinaSaku/Hayate"

# ── Header ────────────────────────────────────────────────────────────────
Write-Host "`n    __  _______  _____  ____________" -ForegroundColor Cyan
Write-Host "   / / / /   \ \/ /   |/_  __/ ____/" -ForegroundColor Cyan
Write-Host "  / /_/ / /| |\  / /| | / / / __/   " -ForegroundColor Cyan
Write-Host " / __  / ___ |/ / ___ |/ / / /___   " -ForegroundColor Cyan
Write-Host "/_/ /_/_/  |_/_/_/  |_/_/ /_____/   `n" -ForegroundColor Cyan
Write-Host "  Swift, Secure, Encrypted & Compressed Local File Transfers`n" -ForegroundColor Magenta

# ── Delegate to cargo-dist PowerShell installer ───────────────────────────
$InstallerUrl = "https://github.com/$Repo/releases/latest/download/hayate-cli-installer.ps1"

Write-Host "[*] Downloading cargo-dist installer..." -ForegroundColor DarkGray
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-Expression (Invoke-WebRequest -Uri $InstallerUrl -UseBasicParsing).Content
} catch {
    Write-Host "[-] Failed to run installer. Try manual install:" -ForegroundColor Red
    Write-Host "    irm $InstallerUrl | iex" -ForegroundColor Yellow
    exit 1
}
