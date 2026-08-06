# AI Brain Companion — Installation für Windows
#
#   irm https://raw.githubusercontent.com/peppermint-digital/ai-brain-companion/main/install.ps1 | iex
#
# Holt das gebaute Programm aus dem letzten Release, legt es nach
# %LOCALAPPDATA%\Programs\ai-brain-companion und trägt den Ordner in den PATH
# des Benutzers ein.
#
# Kein Rust nötig — im Gegensatz zur Unix-Variante, die aus dem Quelltext baut,
# solange es noch keine Releases gibt.

$ErrorActionPreference = 'Stop'

$Repo      = 'peppermint-digital/ai-brain-companion'
$ZielOrdner = Join-Path $env:LOCALAPPDATA 'Programs\ai-brain-companion'
$ZielDatei = Join-Path $ZielOrdner 'ai-brain-companion.exe'

Write-Host 'AI Brain Companion — Installation'
Write-Host ''

# ------------------------------------------------------------------ holen ----

Write-Host 'Suche das neueste Release …'

try {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ 'User-Agent' = 'ai-brain-companion-installer' }
} catch {
    Write-Error @"
Kein Release gefunden.

Solange es keins gibt, baust du aus dem Quelltext — dafuer brauchst du Rust
(https://rustup.rs) und Git:

  git clone https://github.com/$Repo.git
  cd ai-brain-companion
  cargo build --release
"@
    exit 1
}

$asset = $release.assets | Where-Object { $_.name -like '*windows*' -and $_.name -like '*.exe' } | Select-Object -First 1

if (-not $asset) {
    Write-Error "Das Release $($release.tag_name) enthaelt kein Windows-Programm."
    exit 1
}

Write-Host "Lade $($asset.name) aus $($release.tag_name) …"

New-Item -ItemType Directory -Force -Path $ZielOrdner | Out-Null
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $ZielDatei

# Windows markiert heruntergeladene Dateien; ohne das Entfernen fragt der
# SmartScreen beim ersten Start nach.
Unblock-File -Path $ZielDatei -ErrorAction SilentlyContinue

# ------------------------------------------------------------------- PATH ----

$benutzerPfad = [Environment]::GetEnvironmentVariable('Path', 'User')

if ($benutzerPfad -split ';' -contains $ZielOrdner) {
    Write-Host "PATH enthaelt $ZielOrdner bereits."
} else {
    # Nur den Benutzer-PATH anfassen, nie den des Systems — der gilt fuer alle
    # Konten und braucht Administratorrechte.
    [Environment]::SetEnvironmentVariable('Path', "$benutzerPfad;$ZielOrdner", 'User')
    Write-Host "PATH ergaenzt: $ZielOrdner"
    Write-Host 'Oeffne ein neues Terminal, damit das greift.'
}

Write-Host ''
Write-Host "Installiert: $ZielDatei"
Write-Host ''
Write-Host 'Dann:'
Write-Host ''
Write-Host '  ai-brain-companion connect deine-brain-domain.de'
Write-Host ''
