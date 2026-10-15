# Alle Befehle

*Achtzehn Befehle, nach Zweck sortiert, mit den Schaltern, die man wirklich braucht.*

Achtzehn Befehle. Die Liste steht auch im Programm selbst — `ai-brain-cli` ohne
Argument zeigt sie.

## Einrichten

| Befehl | Was |
|---|---|
| connect [domain] | Rechner mit einer Instanz verbinden. Ohne Angabe die Instanz, von der das Programm kam. |
| status | Zeigt die aktuelle Verbindung. |
| configure | Traegt den MCP-Server in Claude Code und Codex ein. |
| disconnect | Entfernt die lokale Verbindung. |
| freigabe [add <pfad>] | Verzeichnisse, in denen Laeufe mehr duerfen als fragen. |

## Arbeiten

| Befehl | Was |
|---|---|
| agent [...] | Einen Lauf hier starten. |
| agents | Welche Laeufe hier laufen. |
| logs <kennung> | Was ein Lauf geschrieben hat. |
| stop <kennung> | Einen Lauf beenden. |

Schalter fuer `agent`:

| Schalter | Wirkung |
|---|---|
| --aufgabe "…" | Auftrag in Worten. |
| --task <nr> | Aufgabe aus AI Brain holen. |
| --projekt <slug> | Projektbezug. |
| --modell claude|codex | Vorgabe ist claude. |
| --schreibend, --vollzugriff | Mehr Rechte — nur in freigegebenen Verzeichnissen. |
| --verzeichnis <pfad> | Wo der Lauf arbeitet. |
| --imkr | In einem eigenen imkr-Pane starten. |
| --weiter, --sitzung <id> | Eine Sitzung fortsetzen. |
| --zeitgrenze <s> | Vorgabe 3600 Sekunden. |
| --trocken | Zeigt, was gestartet wuerde, ohne zu starten. |

## Aufgaben und Auftraege

> **Zwei Woerter, die sich aehneln und Verschiedenes meinen.** Eine **Aufgabe** ist ein Eintrag aus der Projektliste, den sich jemand aussucht. Ein **Auftrag** ist etwas, das AI Brain genau diesem Rechner hingelegt hat.

| Befehl | Was |
|---|---|
| aufgaben [--projekt <slug>] [--json] | Offene Aufgaben zum Auswaehlen. Ohne Projekt projektuebergreifend. |
| auftraege | Was AI Brain fuer diesen Rechner bereitlegt. |
| auftrag <nr> | Annehmen und abarbeiten. Nimmt die Schalter von agent mit. |
| auftrag-melden <nr> <status> | Ergebnis nachtragen, falls das automatische Melden scheiterte. |

## Nachrichten

| Befehl | Was |
|---|---|
| nachricht kontakte | Wen dieser Rechner erreichen kann. |
| nachricht senden <person> <text> | Mit --von und --ziel fuer imkr-Adressen. Hoechstens 8 KB. |
| nachricht abholen | Was fuer diesen Rechner anliegt. |
| nachricht bestaetigen <nr> | Eingereiht — kommt nicht wieder. |

## Wissen abgleichen

| Befehl | Was |
|---|---|
| sync [--apply] | Skills und Agents aus dem Repository holen, einzeln gewaehlte MCP-Server in Claude Code und Codex eintragen. Ohne --apply wird nur gezeigt. |
| publish <name> | Einen lokalen Eintrag zurueckgeben. |
| select | Waehlen, welche Eintraege dieser Rechner bekommt. MCP-Server nur einzeln, nie ueber „alles“. |

Weitere Schalter fuer `sync`: `--only`, `--force`,
`--aufraeumen`, `--ja`.

## Als MCP-Server

```bash
ai-brain-cli mcp
```

Laeuft als MCP-Server und gibt Claude Code und Codex alle AI-Brain-Werkzeuge. Wird von
`configure` eingetragen; man ruft es selten von Hand auf.
