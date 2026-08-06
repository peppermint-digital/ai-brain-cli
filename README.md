# AI Brain Companion

Verbindet einen Rechner mit einer AI-Brain-Instanz — und stellt deren
MCP-Server danach für Claude Code bereit.

Du gibst eine Domain an, bestätigst einmal einen kurzen Code im Browser, fertig.
Zugangsdaten tippst du nirgends ab; sie wandern nie durch die Zwischenablage.

## Was dabei passiert

```
Companion                         AI Brain
   |                                 |
   |-- Steckbrief lesen ------------>|   /.well-known/ai-brain
   |<-- Endpunkte + Client-ID -------|
   |                                 |
   |-- Kopplung starten ------------>|   /oauth/device/code
   |<-- Code "WXYZ-1234" ------------|
   |                                 |
   |   Du bestätigst im Browser      |   /oauth/device
   |                                 |
   |-- wartet ---------------------->|
   |<-- Zugangstoken ----------------|
   |                                 |
   |-- Gerät einrichten ------------>|   /api/v1/companion/provision
   |<== eigener Gerätezugang ========|
```

Am Ende hat **dieser Rechner** einen eigenen Zugang, der **dir** gehört. Er
taucht unter „Meine Geräte" auf und lässt sich dort jederzeit entziehen.

Warum ein eigener Zugang pro Gerät und nicht ein geteilter: So bleibt
nachvollziehbar, was auf welchem Rechner passiert ist. In der Zeiterfassung
steht der Gerätename, zugerechnet wird die Arbeit dir.

## Installieren

Aktuell wird aus dem Quelltext gebaut. Vorgefertigte Binaries und der
`curl`-Einzeiler kommen mit dem ersten Release.

Voraussetzung ist eine Rust-Installation (`rustup`).

```bash
git clone https://github.com/peppermint-digital/ai-brain-companion.git
cd ai-brain-companion
cargo build --release
install -m 755 target/release/ai-brain-companion ~/.local/bin/
```

Liegt `~/.local/bin` nicht in deinem `PATH`, ergänze es in `~/.bashrc` oder
`~/.zshrc`:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

**Plattformen:** Linux läuft. macOS ist der nächste Schritt. Windows reichen wir
nach. Für macOS und Windows braucht es signierte Binaries, sonst blocken
Gatekeeper und SmartScreen — das hängt an CI und Zertifikaten, nicht am Code.

## Verbinden

```bash
ai-brain-companion connect brain.example.com
```

Der Companion zeigt einen Code und eine Adresse. Öffne sie, melde dich an,
bestätige. Danach steht:

```
Fertig.
  Gerät          : mein-laptop (linux x86_64)
  Zugerechnet an : Bastian
  Hinterlegt in  : ~/.config/ai-brain-companion/connection.json
  Claude Code    : Eintrag „ai-brain" in ~/.claude.json
```

Claude Code neu starten, dann stehen die Werkzeuge bereit.

### Optionen

| Option | Wirkung |
|---|---|
| `--name <Name>` | Abweichender Gerätename (Standard: Hostname) |
| `--no-claude-config` | Claude-Code-Konfiguration nicht anfassen |

## Weitere Befehle

```bash
ai-brain-companion status       # Zeigt die aktuelle Verbindung
ai-brain-companion disconnect   # Entfernt die lokale Verbindung
ai-brain-companion mcp          # Läuft als MCP-Server (startet Claude Code selbst)
```

`mcp` rufst du nicht von Hand auf — das macht Claude Code. Der Befehl hält die
Gerätezugangsdaten, besorgt sich damit Zugangstoken, erneuert sie rechtzeitig
und reicht die Aufrufe an die Instanz weiter.

Genau dafür gibt es den Companion überhaupt als dauerhaft laufendes Programm:
Ein Zugangstoken lebt eine Stunde, Claude Codes Konfiguration kann aber nur
einen festen Wert halten. Ohne etwas, das erneuert, wäre die Verbindung nach
einer Stunde tot.

## Wo was liegt

| Datei | Inhalt |
|---|---|
| `~/.config/ai-brain-companion/connection.json` | Gerätezugangsdaten, Rechte `0600` |
| `~/.claude.json` | Eintrag `mcpServers.ai-brain` |
| `~/.claude.json.vor-companion` | Sicherung, angelegt vor dem ersten Eintrag |

`disconnect` entfernt nur die lokale Ablage. Der Gerätezugang besteht auf der
Instanz weiter — endgültig entziehst du ihn unter „Meine Geräte".

## Voraussetzungen auf der Instanz

Die Instanz muss den Kopplungs-Client kennen. Einmalig dort ausführen:

```bash
php artisan mcp:companion-client
```

Fehlt er, sagt der Companion genau das — statt in einen OAuth-Fehler zu laufen.

## Sicherheit

Der Kopplungs-Client ist öffentlich und hat **kein** Secret. Das ist Absicht:
Ein Programm, das auf fremden Rechnern läuft, kann kein Geheimnis hüten. Die
Berechtigung entsteht nicht durch den Besitz des Programms, sondern durch deine
Bestätigung im Browser.

Der Token, mit dem der Companion seinen Gerätezugang abholt, wird dabei
**verbraucht**. Ein zweiter Versuch mit demselben Token läuft ins Leere.

Bekannte Grenze: Die Gerätezugangsdaten liegen als Datei mit `0600` statt im
Schlüsselbund des Betriebssystems. Das steht auf der Liste.

## Lizenz

MIT
