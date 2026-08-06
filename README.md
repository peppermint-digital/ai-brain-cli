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

### Von deiner AI-Brain-Instanz (empfohlen)

```bash
curl -fsSL https://deine-brain-domain.de/install.sh | sh
```

Jede Instanz liefert den Companion selbst aus. Der Vorteil: **Die Domain steckt
schon im Befehl** — das Skript merkt sie sich, und danach genügt
`ai-brain-companion connect` ohne Argument.

Ohne AI Brain ist der Companion ohnehin nutzlos; die Instanz ist also sein
natürlicher Ort. Wer AI Brain selbst betreibt, muss seine Leute damit nicht auf
ein fremdes Repository schicken.

Was eine Instanz bereithält, steht unter `https://deine-domain.de/install.json`.

> **Für Betreiber:** Die Wege kommen mit AI Brain mit, die Programme nicht — 8 MB
> Binärdateien gehören nicht ins Repository. Einmal
> `php artisan companion:fetch` auf der Instanz, danach versorgt sie ihre eigenen
> Rechner. Ohne Internetzugang: `companion:fetch --von=/pfad/zum/ordner`. Liegt
> für eine Plattform nichts bereit, sagt das Skript das und verweist auf den
> Quelltext, statt eine kaputte Datei zu installieren.

### Von GitHub

```bash
curl -fsSL https://raw.githubusercontent.com/peppermint-digital/ai-brain-companion/main/install.sh | sh
```

Holt das gebaute Programm aus dem letzten Release, legt es nach `~/.local/bin`
und trägt das Verzeichnis in `~/.zshrc` und `~/.bashrc` ein, falls es dort noch
fehlt. Mehrfaches Ausführen ändert nichts doppelt; einen vorhandenen eigenen
Eintrag lässt es in Ruhe. `~/.bash_profile` und `~/.profile` werden nur
angefasst, wenn sie schon existieren.

Danach eine neue Shell öffnen (oder `. ~/.zshrc`).

Gibt es kein passendes Release, **baut das Skript aus dem Quelltext** — dann
braucht es Rust (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
und etwa eine halbe Minute.

### Windows

```powershell
irm https://raw.githubusercontent.com/peppermint-digital/ai-brain-companion/main/install.ps1 | iex
```

Legt das Programm nach `%LOCALAPPDATA%\Programs\ai-brain-companion` und
ergänzt den PATH deines Benutzers — nicht den des Systems, das bräuchte
Administratorrechte. Danach ein neues Terminal öffnen.

Alternativ funktioniert unter WSL die Linux-Anleitung unverändert.

### Von Hand

```bash
git clone https://github.com/peppermint-digital/ai-brain-companion.git
cd ai-brain-companion
cargo build --release
install -m 755 target/release/ai-brain-companion ~/.local/bin/
```

### Plattformen

Linux ist Ende zu Ende getestet. macOS und Windows werden gebaut und
veröffentlicht, sind aber noch nicht im Feld erprobt.

**Signaturen brauchst du dafür nicht.** Ein Programm, das per `curl` im Terminal
ankommt, bekommt unter macOS kein Quarantäne-Merkmal — Gatekeeper greift bei
`.dmg`, `.pkg` und allem, was im Finder doppelgeklickt wird. Unter Windows
entfernt das Installationsskript die Herkunftsmarkierung.

## Verbinden

```bash
ai-brain-companion connect brain.example.com
```

Der Companion zeigt einen Code und eine Adresse. Öffne sie, melde dich an,
bestätige. Der Browser meldet „Gerät verbunden", das Terminal zeigt:

```
Fertig.
  Gerät          : bastian@mein-laptop (linux x86_64)
  Zugerechnet an : Bastian
  Hinterlegt in  : ~/.config/ai-brain-companion/connection.json
  Claude Code    : Eintrag „ai-brain" in ~/.claude.json
```

Claude Code neu starten, dann stehen die Werkzeuge bereit.

**Codex wird mitbedient**, falls es installiert ist. Der Companion nutzt dafür
`codex mcp add` — Codex kennt sein eigenes TOML-Format am besten, und die
`~/.codex/config.toml` enthält meist schon andere Abschnitte, die kein
selbstgebauter Schreiber anfassen sollte.

### Optionen

| Option | Wirkung |
|---|---|
| `--name <Name>` | Abweichender Gerätename |
| `--no-claude-config` | Claude-Code-Konfiguration nicht anfassen |

## Gerätenamen

Der Standardname ist `konto@rechner` plus Plattform, also etwa
`bastian@omarchy (linux x86_64)`.

Das Konto steht bewusst mit drin: Zwei Benutzerkonten auf derselben Maschine
sind aus Sicht von AI Brain zwei Geräte, weil die Verbindung pro Konto in
`~/.config` liegt. Ohne den Kontonamen entstünden zwei Einträge mit identischem
Namen — und weil dieser Name in der Zeiterfassung als Gerät erscheint, wäre die
Auswertung pro Rechner damit wertlos. Genau das ist beim ersten Feldtest
passiert.

Nachträglich umbenennen kannst du jedes Gerät unter „Meine Geräte" über das
Stift-Symbol. Neu koppeln ist dafür nicht nötig.

## Skills und Agents abgleichen

```bash
ai-brain-companion sync           # zeigt nur, was sich ändern würde
ai-brain-companion sync --apply   # schreibt
```

Skills landen unter `~/.claude/skills/`, Agents unter `~/.claude/agents/`.

### Was dabei nicht passiert

**Was nur bei dir liegt, bleibt liegen.** Ein Skill, den du selbst gebaut hast,
wird nicht gelöscht, nicht überschrieben und nicht hochgeladen. Die geteilte
Ablage geht er nichts an, solange du ihn nicht dorthin gibst.

**Was du geändert hast, wird nicht stillschweigend ersetzt.** Hat sich ein
Eintrag auf beiden Seiten bewegt, meldet `sync` einen Konflikt und lässt ihn in
Ruhe. Mit `--force` überschreibst du trotzdem — deine Fassung wandert dann als
`<name>.lokal` daneben, statt verloren zu gehen.

**Hooks werden nie eingerichtet.** Ein Hook ist keine Datei, sondern ein Befehl
in der `settings.json`, der von selbst ausgeführt wird. Er taucht in der Liste
auf, damit du weißt, dass es ihn gibt — eingerichtet wird er von Hand.

### Optionen

| Option | Wirkung |
|---|---|
| `--apply` | Änderungen wirklich schreiben |
| `--only <name>` | Nur diesen einen Eintrag |
| `--force` | Auch überschreiben, wo lokal geändert wurde |
| `--aufraeumen` | Nicht mehr Ausgewähltes entfernen (siehe unten) |

`--force` und `--aufraeumen` tun allein nichts — sie brauchen zusätzlich
`--apply`.

### Auswahl pro Rechner

Unter „Meine Geräte" legst du fest, was auf diesem Rechner liegen soll. Der
Companion meldet sich mit seinem Gerätezugang an, die Instanz weiß also, wer
fragt, und schickt genau diese Auswahl.

Pro Gerät gibt es zwei Betriebsarten:

| Modus | Bedeutung |
|---|---|
| **Alles** | Der Rechner bekommt das gesamte Repository |
| **Nur Auswahl** | Genau das Angehakte — auch nichts, wenn nichts angehakt ist |

**Neu verbundene Rechner starten auf „Nur Auswahl"** und damit leer. Du
entscheidest, was drauf soll, statt 194 Einträge vorgesetzt zu bekommen.

### Abgewähltes entfernen

```bash
ai-brain-companion sync --apply --aufraeumen
```

Entfernt Einträge, die nicht mehr ausgewählt sind. Aber nur die, die **der
Companion selbst geschrieben hat** und die seither **unverändert** sind.

Alles andere bleibt liegen, auch mit `--aufraeumen`: deine Eigenbauten, alles
was du verändert hast, und alles was er nie angelegt hat. Eine Synchronisation,
die fremde Arbeit löschen kann, ist keine Synchronisation mehr.

Ohne `--aufraeumen` wird nichts entfernt — es wird nur gemeldet.

## Etwas zurückgeben

```bash
ai-brain-companion publish mein-skill
ai-brain-companion publish mein-skill --grund "Abschnitt zu X ergänzt"
```

Nimmt einen Eintrag aus `~/.claude/skills/` oder `~/.claude/agents/` und legt
ihn im Repository ab. Existiert er dort schon, wird er aktualisiert statt
verdoppelt — die vorherige Fassung bleibt als Version erhalten und ist
zurückholbar.

Das ist bewusst ein eigener Befehl und passiert **nie** als Nebeneffekt von
`sync`. Ein Skill, den du für dich gebaut hast, mit Notizen oder Pfaden darin,
soll nicht ungefragt in einer Ablage landen, die alle lesen können.

Binärdateien überspringt der Befehl und sagt welche — lieber melden als etwas
Kaputtes hochladen.

### Wie Änderungen erkannt werden

Der Companion merkt sich beim Schreiben zwei Dinge: die Prüfsumme, die die
Instanz damals nannte, und einen eigenen Hash über die Dateien, wie er sie
hingelegt hat. Beim nächsten Lauf vergleicht er den ersten mit der Instanz
(hat sich dort etwas getan?) und den zweiten mit der Platte (hat hier jemand
Hand angelegt?).

Beide Vergleiche laufen gegen den eigenen Wert. Die Server-Prüfsumme
nachzurechnen wäre naheliegend, aber zerbrechlich: Zwei Sprachen müssten bis
aufs letzte Escaping identisch serialisieren, und ein einziger Unterschied
ließe alles als „geändert" erscheinen.

## Weitere Befehle

```bash
ai-brain-companion status            # Zeigt die aktuelle Verbindung
ai-brain-companion configure         # Trägt den MCP-Server nachträglich ein (Claude + Codex)
ai-brain-companion disconnect        # Entfernt die lokale Verbindung
ai-brain-companion mcp               # Läuft als MCP-Server (startet Claude Code selbst)
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

## Wenn etwas klemmt

| Meldung | Bedeutung |
|---|---|
| `command not found` | `~/.local/bin` fehlt im `PATH`. Neue Shell öffnen oder `. ~/.zshrc` |
| `… kennt noch keinen Companion-Zugang` | Auf der Instanz fehlt `php artisan mcp:companion-client` |
| `Der Code ist abgelaufen` | Zu lange gewartet. `connect` einfach neu starten |
| `Die Kopplung wurde im Browser abgelehnt` | Im Bestätigungsdialog auf „Ablehnen" geklickt |
| `Der Gerätezugang wurde abgelehnt` | Gerät wurde unter „Meine Geräte" entzogen. Neu koppeln |

Läuft die Instanz mit Octane, werden Änderungen an ihrem PHP-Code erst nach
einem Neustart des Dienstes wirksam — das betrifft dich nur, wenn du selbst an
AI Brain entwickelst.

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
