# AI Brain CLI

Verbindet einen Rechner mit einer AI-Brain-Instanz — und stellt deren
MCP-Server danach für Claude Code bereit.

Du gibst eine Domain an, bestätigst einmal einen kurzen Code im Browser, fertig.
Zugangsdaten tippst du nirgends ab; sie wandern nie durch die Zwischenablage.

## Was dabei passiert

```
ai-brain-cli                         AI Brain
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

Jede Instanz liefert die CLI selbst aus. Der Vorteil: **Die Domain steckt
schon im Befehl** — das Skript merkt sie sich, und danach genügt
`ai-brain-cli connect` ohne Argument.

Ohne AI Brain ist die CLI ohnehin nutzlos; die Instanz ist also ihr
natürlicher Ort. Wer AI Brain selbst betreibt, muss seine Leute damit nicht auf
ein fremdes Repository schicken.

Was eine Instanz bereithält, steht unter `https://deine-domain.de/install.json`.

> **Für Betreiber:** Die Wege kommen mit AI Brain mit, die Programme nicht — 8 MB
> Binärdateien gehören nicht ins Repository. Einmal
> `php artisan companion:fetch` auf der Instanz, danach versorgt sie ihre eigenen
> Rechner. Ohne Internetzugang: `companion:fetch --von=/pfad/zum/ordner`. Liegt
> für eine Plattform nichts bereit, sagt das Skript das und verweist auf den
> Quelltext, statt eine kaputte Datei zu installieren.
>
> **Bauen kannst du selbst — GitHub Actions braucht es dafür nicht.** Linux
> direkt mit `cargo build --release`, Windows in einem Container:
>
> ```bash
> docker run --rm -v "$PWD":/src:ro -v /tmp/aus:/out -w /work rust:1-bookworm sh -c '
>   apt-get update -qq && apt-get install -y -qq gcc-mingw-w64-x86-64
>   rustup target add x86_64-pc-windows-gnu
>   cp -r /src/. /work/ && rm -rf /work/target
>   cargo build --release --target x86_64-pc-windows-gnu
>   cp target/x86_64-pc-windows-gnu/release/ai-brain-cli.exe /out/'
> ```
>
> Dauert etwa 20 Sekunden. `mingw-w64` und Rust sind frei — hier steht keine
> Lizenz im Weg. **Für macOS geht das nicht:** Dafür braucht es Apples SDK, und
> dessen Lizenz erlaubt die Nutzung nur auf Apple-Hardware. Das lässt sich nicht
> wegcontainern, sondern nur auf einem Mac erledigen (siehe unten) — dessen
> Ergebnis du dann mit `companion:fetch --von=…` auf die Instanz legst, damit
> jeder weitere Mac es fertig bekommt.

### Von GitHub

```bash
curl -fsSL https://raw.githubusercontent.com/peppermint-digital/ai-brain-cli/main/install.sh | sh
```

> ⚠️ Aktuell existiert **kein Release**. Das Skript baut deshalb aus dem
> Quelltext (siehe unten) — schneller bist du mit dem Weg über deine Instanz.

Holt das gebaute Programm aus dem letzten Release, legt es nach `~/.local/bin`
und trägt das Verzeichnis in `~/.zshrc` und `~/.bashrc` ein, falls es dort noch
fehlt. Mehrfaches Ausführen ändert nichts doppelt; einen vorhandenen eigenen
Eintrag lässt es in Ruhe. `~/.bash_profile` und `~/.profile` werden nur
angefasst, wenn sie schon existieren.

Danach eine neue Shell öffnen (oder `. ~/.zshrc`).

Gibt es kein passendes Release, **baut das Skript aus dem Quelltext** — dann
braucht es Rust (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
und etwa eine halbe Minute.

### macOS

Solange auf der Instanz kein fertiges Programm für macOS liegt, baut der Mac es
sich selbst — zwei Befehle, ein bis zwei Minuten Rechenzeit:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo install --git https://github.com/peppermint-digital/ai-brain-cli
```

Danach liegt es in `~/.cargo/bin` und du verbindest wie überall:

```bash
ai-brain-cli connect brain.example.com
```

Das ist kein Notbehelf, sondern für einen einzelnen Mac der kürzere Weg — er
kommt ohne Release, ohne Token und ohne fremde Baumaschine aus.

Wer mehrere Macs anzubinden hat, baut einmal auf einem davon und legt das
Ergebnis auf die Instanz:

```bash
# auf dem Mac
cargo build --release
# Datei als ai-brain-cli-aarch64-apple-darwin auf die Instanz bringen,
# dort:  php artisan companion:fetch --von=/pfad/zum/ordner
```

Ab dann genügt auf jedem weiteren Mac der Einzeiler von der Instanz.

### Windows

```powershell
irm https://deine-brain-domain.de/install.ps1 | iex
```

Wie unter Linux liefert die Instanz das Skript selbst aus, mit ihrer Adresse
darin. Es holt die `.exe`, **prüft die Prüfsumme aus `/install.json`**, nimmt die
Herkunftsmarkierung weg (sonst fragt der SmartScreen beim ersten Start nach),
merkt sich die Adresse unter `%APPDATA%\ai-brain-cli\instanz` und ergänzt
den PATH — den des **Benutzers**, nicht den des Systems, der bräuchte
Administratorrechte.

Danach ein neues Terminal öffnen, dann genügt `ai-brain-cli connect` ohne
Argument.

Wenn du die Adresse nicht zur Hand hast, geht auch der Weg über dieses
Repository — das Skript fragt dann danach und holt den Rest von dort:

```powershell
irm https://raw.githubusercontent.com/peppermint-digital/ai-brain-cli/main/install.ps1 | iex
```

Alternativ funktioniert unter WSL die Linux-Anleitung unverändert.

### Von Hand

```bash
git clone https://github.com/peppermint-digital/ai-brain-cli.git
cd ai-brain-cli
cargo build --release
install -m 755 target/release/ai-brain-cli ~/.local/bin/
```

### Plattformen

| Plattform | Stand |
|---|---|
| Linux x86-64 | Ende zu Ende erprobt, fertiges Programm auf der Instanz |
| Windows x86-64 | gebaut (GNU-Ziel, im Container); Installer gegen die echte Instanz durchgespielt, aber **auf echtem Windows noch nicht gelaufen** |
| macOS | wird auf dem Mac selbst gebaut, **noch nicht im Feld erprobt** |

Die Pfadauflösung unterscheidet sich zwischen den Systemen: `HOME` gibt es unter
Windows nicht, dort heißt es `USERPROFILE`, und die Verbindungsdaten gehören
nach `%APPDATA%` statt in einen versteckten Ordner. Das ist berücksichtigt und
durch einen Test abgedeckt — aber ein Test ist kein echtes Windows.

**Signaturen brauchst du dafür nicht.** Ein Programm, das per `curl` im Terminal
ankommt, bekommt unter macOS kein Quarantäne-Merkmal — Gatekeeper greift bei
`.dmg`, `.pkg` und allem, was im Finder doppelgeklickt wird. Unter Windows
nimmt das Installationsskript die Markierung mit `Unblock-File` weg.

## Verbinden

```bash
ai-brain-cli connect brain.example.com
```

Die CLI zeigt einen Code und eine Adresse. Öffne sie, melde dich an,
bestätige. Der Browser meldet „Gerät verbunden", das Terminal zeigt:

```
Fertig.
  Gerät          : bastian@mein-laptop (linux x86_64)
  Zugerechnet an : Bastian
  Hinterlegt in  : ~/.config/ai-brain-cli/connection.json
  Claude Code    : Eintrag „ai-brain" in ~/.claude.json
```

Claude Code neu starten, dann stehen die Werkzeuge bereit.

**Codex wird mitbedient**, falls es installiert ist. Die CLI nutzt dafür
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
ai-brain-cli sync           # zeigt nur, was sich ändern würde
ai-brain-cli sync --apply   # schreibt
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

Unter „Meine Geräte" legst du fest, was auf diesem Rechner liegen soll. Die
CLI meldet sich mit ihrem Gerätezugang an, die Instanz weiß also, wer
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
ai-brain-cli sync --apply --aufraeumen
```

Entfernt Einträge, die nicht mehr ausgewählt sind. Aber nur die, die **der
ai-brain-cli selbst geschrieben hat** und die seither **unverändert** sind.

Alles andere bleibt liegen, auch mit `--aufraeumen`: deine Eigenbauten, alles
was du verändert hast, und alles was sie nie angelegt hat. Eine Synchronisation,
die fremde Arbeit löschen kann, ist keine Synchronisation mehr.

Ohne `--aufraeumen` wird nichts entfernt — es wird nur gemeldet.

## Etwas zurückgeben

```bash
ai-brain-cli publish mein-skill
ai-brain-cli publish mein-skill --grund "Abschnitt zu X ergänzt"
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

Die CLI merkt sich beim Schreiben zwei Dinge: die Prüfsumme, die die
Instanz damals nannte, und einen eigenen Hash über die Dateien, wie sie sie
hingelegt hat. Beim nächsten Lauf vergleicht sie den ersten mit der Instanz
(hat sich dort etwas getan?) und den zweiten mit der Platte (hat hier jemand
Hand angelegt?).

Beide Vergleiche laufen gegen den eigenen Wert. Die Server-Prüfsumme
nachzurechnen wäre naheliegend, aber zerbrechlich: Zwei Sprachen müssten bis
aufs letzte Escaping identisch serialisieren, und ein einziger Unterschied
ließe alles als „geändert" erscheinen.

## Agentenläufe

```bash
ai-brain-cli agent --task 7666
ai-brain-cli agent --projekt ai-brain "Changelog für heute schreiben"
ai-brain-cli agent --modell codex --dir ~/Development/crm "Tests reparieren"
ai-brain-cli agent --task 7666 --trocken     # nur zeigen, was passieren würde
```

Startet einen Agenten auf diesem Rechner, meldet ihn im Register an und
protokolliert, was er tut.

**Die Schleife ist gemietet, nicht gebaut.** Die CLI ruft `claude -p` bzw.
`codex exec` auf. Es gibt hier keinen eigenen Modellaufruf, keinen eigenen
Werkzeug-Dispatch und keine eigene Kontextkürzung — der Claude Agent SDK *ist*
Claude Code als Bibliothek, und für Codex, Ollama und OpenRouter löst eine
eigene Schleife ohnehin nichts. Was die CLI beiträgt, ist das, was dem
gemieteten Agenten fehlt: Kontext aus AI Brain, eine geprüfte Umgebung, ein
Registereintrag und ein Protokoll.

**Dein eigener Login zählt.** Der Lauf nutzt die Anmeldung, die auf diesem
Rechner ohnehin gilt — Abo oder `ANTHROPIC_API_KEY`. Es wird kein Token
weitergereicht und kein Proxy gebraucht.

**Der Vorlauf bleibt kurz.** Der Agent bekommt gesagt, welche Aufgabe gemeint
ist und dass `/brain` gilt — den Kontext holt er sich selbst über MCP. Alles
hineinzukopieren hiesse, eine zweite Kopie zu pflegen, die veraltet.

**Sitzungsvariablen werden entfernt.** `CLAUDECODE`, `CLAUDE_CODE_*`,
`CLAUDE_PID`, `CLAUDE_EFFORT`. Ohne das hält sich ein aus einer laufenden
Claude-Code-Sitzung gestarteter Agent für ein Kind davon und schreibt kein
eigenes Transkript.

### Was in AI Brain davon ankommt

Jeder Lauf meldet Anfang und Ende als Audit-Eintrag: Projekt, Aufgabe, Modell,
Verzeichnis, Rechte-Stufe, Weg, Exit-Code, Dauer.

**Person und Gerät muss die CLI dafür nicht mitgeben.** AI Brain nimmt die
Person aus dem Token und den Gerätenamen aus dem OAuth-Client — bei einem
Gerätezugang ist der Client-Name der Gerätename. Dieselbe Person an zwei
Rechnern bleibt damit auseinanderzuhalten.

**Ein fehlgeschlagenes Protokoll hält den Lauf nicht auf.** Die Arbeit passiert
auf diesem Rechner; ist AI Brain gerade nicht erreichbar, gibt es eine Zeile auf
stderr und der Lauf läuft. Ohne Verbindung geht `agent` überhaupt — nur `--task`
verlangt sie, denn ein Lauf „an Aufgabe #N", der die Aufgabe nicht kennt,
arbeitet an etwas Erfundenem.

**Zeiteinträge werden nicht angelegt.** Ein Agentenlauf ist Maschinenzeit, keine
Arbeitszeit eines Menschen; automatisch erfasst würde er die Zeiterfassung
aufblähen und sich mit dem eigenen Timer überlappen. Wer die Zeit haben will,
trägt sie bewusst ein.

### In einem imkr-Pane statt als Kindprozess

```bash
ai-brain-cli agent --imkr --task 7667
```

Ein beaufsichtigter Lauf ist ein Kindprozess: Die CLI hält ihn, schreibt sein
Protokoll, stempelt seinen Herzschlag. Richtig für unbeaufsichtigte Arbeit —
aber man kann ihm nicht zusehen und nichts hineinrufen.

Mit `--imkr` startet der Lauf **interaktiv in einem neuen imkr-Fenster**. Dann
gilt:

| | beaufsichtigt | `--imkr` |
|---|---|---|
| Protokoll | `protokoll.txt` | der Schirm (`logs` zeigt ihn) |
| Zustand | Herzschlag der CLI | von imkr (`imkr status`-Hooks) |
| Zusehen, mitreden | nein | `imkr read`, `imkr msg` |
| Zeitgrenze | ja | nein (das Pane gehört imkr) |

Die CLI baut davon **nichts** nach. Was imkr kann — Panes, Agentenzustand,
Nachrichten zwischen Agenten, Übersicht über mehrere Rechner — bleibt bei imkr.

**imkr ist optional.** Ohne imkr fehlt nur diese Betriebsart. `imkr new`
startet allerdings keine Sitzung: `--imkr` geht nur aus einer laufenden
imkr-Sitzung heraus, und wer es von aussen versucht, bekommt das gesagt.

**Ein Pane in einem unbekannten Verzeichnis fragt erst nach Vertrauen.** Claude
Code tut das beim ersten Mal; im Druckmodus (`-p`) passiert es nicht, in einem
Pane schon. Die CLI sagt das vorher — und beantwortet die Frage nicht für dich.
Ein Flag in deine Konfiguration zu schreiben, um eine Sicherheitsabfrage
wegzuklicken, ist keine Bequemlichkeit.

Solange dort etwas fragt, meldet `agents` den Lauf als `wartet` mit Grund — und
**nicht** als `fertig`. Das ist nicht selbstverständlich: imkr meldet ein Pane,
das an einem Dialog hängt, als `done`. Ein Lauf gilt erst dann als fertig, wenn
er vorher gearbeitet hat.

### Rechte und Grenzen

Drei Stufen, aufsteigend:

| | claude | codex | braucht Freigabe |
|---|---|---|---|
| (ohne Schalter) | Vorgaben | Vorgaben | nein |
| `--schreibend` | `--permission-mode acceptEdits` | `-s workspace-write` | **ja** |
| `--vollzugriff` | `--permission-mode bypassPermissions` | `--dangerously-bypass-approvals-and-sandbox` | **ja** |

**Gesichert wird nicht „ein Agent läuft", sondern „ein Agent fragt nicht
mehr".** Ein Lauf ohne Schalter fragt, bevor er etwas tut — damit ist er nicht
gefährlicher als ein Mensch, der dort `claude` tippt, und er braucht keine
Freigabe. Das ist Absicht: Ein Riegel, der am Alltag hängenbleibt, wird
abgeschaltet.

```bash
ai-brain-cli freigabe                        # was freigegeben ist
ai-brain-cli freigabe add ~/Development      # mit Unterverzeichnissen
ai-brain-cli freigabe rm ~/Development
```

**Ohne Freigabedatei gibt es keinen Vollzugriff — nirgends.** Pfade werden
aufgelöst, nicht verglichen: `~/frei/../geheim` ist als Zeichenkette etwas
anderes als `/home/du/geheim`, als Verzeichnis aber dasselbe. Und ein Nachbar
mit gleichem Namensanfang (`Development-geheim` neben `Development`) gilt nicht
mit.

Das Heimatverzeichnis oder `/` freizugeben verlangt zusätzlich `--trotzdem` —
das wären auch `~/.ssh` und die Zugangsdaten dieser CLI.

**Zeitgrenze:** Vorgabe eine Stunde, `--zeitgrenze <sekunden>`, `0` hebt sie
auf. Ein unbeaufsichtigter Lauf, der hängt, hielte sonst ewig seinen Herzschlag
und sähe für jeden Leser gesund aus. Beim Erreichen wird der Lauf beendet und
mit Grund vermerkt.

Eine **Zug-Grenze gibt es nicht**: `claude` kennt kein `--max-turns`. Einen
Schalter anzubieten, den die Gegenseite nicht hat, wäre ein Versprechen ohne
Deckung.

**`codex exec` braucht ein Git-Verzeichnis.** Ausserhalb bricht es mit „Not
inside a trusted directory" ab, ohne etwas zu tun. Die CLI prüft das vorher und
sagt es — umgangen wird es nicht (`--skip-git-repo-check` wäre ein Riegel, den
wir für dich wegklicken).

Was `--modell ollama` angeht: noch nicht verdrahtet. Der Weg führt über die
codex-CLI mit eigener Basis-Adresse; bis dahin sagt die CLI das, statt etwas
anderes zu starten.

### Laufende Läufe ansehen

Die CLI hält mehrere Agentenläufe gleichzeitig und gibt Auskunft über sie.

```bash
ai-brain-cli agents                  # was hier läuft
ai-brain-cli agents --json           # dasselbe für Programme
ai-brain-cli logs <kennung>          # was ein Lauf geschrieben hat
ai-brain-cli logs <kennung> --zeilen 500
ai-brain-cli stop <kennung>          # einen Lauf beenden
```

```
● laeuft    1760440000-999     claude     #7665    1h06
✓ fertig    1760436210-8821    codex      ai-brain 4m
✗ fehler    1760430004-7719    claude     #7612    12m
              Aufseher verschwunden — kein Lebenszeichen seit 412 s
```

Vier Zustände: `laeuft`, `wartet` (der Agent hat eine Rückfrage), `fertig`,
`fehler`.

**Wie ein verschwundener Lauf erkannt wird.** Der Aufseher stempelt im Takt ein
Lebenszeichen. Bleibt der Stempel länger als drei Minuten stehen, gilt der Lauf
als verschwunden und erscheint als `fehler` mit Grund. Das ist bewusst nicht
„läuft der Prozess noch?": Diese Frage braucht entweder eine weitere
Abhängigkeit oder `/proc`, das es unter macOS nicht gibt — und sie übersieht den
Fall, dass ein Prozess lebt, aber nichts mehr tut.

**Der Befund wird geschrieben, nicht nur angezeigt.** `agents` korrigiert den
Zustand in der Datei. Sonst stünde dort für immer „laeuft", und jeder weitere
Leser — das imkr-Plugin, AI Brain — müsste dieselbe Herleitung nachbauen.

**Was wann verschwindet:** abgeschlossene Läufe nach 14 Tagen samt Protokoll.
Laufende nie, egal wie alt.

### Wo das liegt

| Pfad | Inhalt |
|---|---|
| `${XDG_STATE_HOME:-~/.local/state}/ai-brain-cli/laeufe/<kennung>/lauf.json` | Zustand des Laufs |
| `…/laeufe/<kennung>/protokoll.txt` | seine Ausgabe |

Unter Windows `%LOCALAPPDATA%\ai-brain-cli\laeufe`.

Bewusst **nicht** in `~/.config`: Dort liegen die Zugangsdaten, das wird
gesichert und zieht bei einer Umbenennung mit um. Protokolle laufender Agenten
haben da nichts zu suchen.

Die Kennung ist `<sekunde>-<prozess>`, bei Kollision mit Zusatz. Der Ordner wird
mit `create_dir` beansprucht — das scheitert, wenn es ihn schon gibt, und ist
damit auch zwischen zwei Prozessen eindeutig.

## Weitere Befehle

```bash
ai-brain-cli status            # Zeigt die aktuelle Verbindung
ai-brain-cli configure         # Trägt den MCP-Server nachträglich ein (Claude + Codex)
ai-brain-cli disconnect        # Entfernt die lokale Verbindung
ai-brain-cli mcp               # Läuft als MCP-Server (startet Claude Code selbst)
```

`mcp` rufst du nicht von Hand auf — das macht Claude Code. Der Befehl hält die
Gerätezugangsdaten, besorgt sich damit Zugangstoken, erneuert sie rechtzeitig
und reicht die Aufrufe an die Instanz weiter.

Genau dafür gibt es die CLI überhaupt als dauerhaft laufendes Programm:
Ein Zugangstoken lebt eine Stunde, Claude Codes Konfiguration kann aber nur
einen festen Wert halten. Ohne etwas, das erneuert, wäre die Verbindung nach
einer Stunde tot.

## Wo was liegt

| Datei | Inhalt |
|---|---|
| `~/.config/ai-brain-cli/connection.json` | Gerätezugangsdaten, Rechte `0600` |
| `~/.claude.json` | Eintrag `mcpServers.ai-brain` |
| `~/.claude.json.vor-companion` | Sicherung, angelegt vor dem ersten Eintrag |

Der Name `vor-companion` ist historisch: Das Programm hiess bis Oktober 2026
`ai-brain-companion`. Die Datei behaelt ihren Namen, damit auf den verbundenen
Rechnern keine zweite Sicherung neben der vorhandenen entsteht.

### Umstieg von `ai-brain-companion`

Nichts zu tun. Beim ersten Start holt `ai-brain-cli` die Ablage von
`~/.config/ai-brain-companion/` herueber — die Verbindung, den Abgleichstand
und die gesicherten Fassungen — und sagt, dass es das getan hat. Neu koppeln
ist **nicht** noetig und waere schaedlich: `connect` legte einen zweiten
Geraetezugang an, derselbe Rechner stuende zweimal unter „Meine Geraete", und
die Zeiterfassung verteilte sich auf zwei Namen.

Was schon unter dem neuen Namen liegt, bleibt unangetastet. Konnte etwas nicht
uebernommen werden, bleibt der alte Ordner stehen und das Programm nennt den
Grund — eine halb umgezogene Ablage waere schlimmer als keine.

`disconnect` entfernt nur die lokale Ablage. Der Gerätezugang besteht auf der
Instanz weiter — endgültig entziehst du ihn unter „Meine Geräte".

## Voraussetzungen auf der Instanz

Die Instanz muss den Kopplungs-Client kennen. Einmalig dort ausführen:

```bash
php artisan mcp:companion-client
```

Fehlt er, sagt die CLI genau das — statt in einen OAuth-Fehler zu laufen.

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

Der Token, mit dem die CLI seinen Gerätezugang abholt, wird dabei
**verbraucht**. Ein zweiter Versuch mit demselben Token läuft ins Leere.

Bekannte Grenze: Die Gerätezugangsdaten liegen als Datei mit `0600` statt im
Schlüsselbund des Betriebssystems. Das steht auf der Liste.

## Lizenz

MIT
