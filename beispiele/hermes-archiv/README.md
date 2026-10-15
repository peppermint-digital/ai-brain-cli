# Was von Hermes übrig ist

Hermes wurde am 15.10.2026 von diesem Rechner entfernt. Hier liegen die Teile,
die als Herkunftsnachweis zählen — 24 KB statt 2,1 GB.

## Die vier Wächter-Skripte

| Datei | Läufe | Was daraus wurde |
|---|---|---|
| `ai-brain-avatar-tts-watchdog.sh` | **1422**, pausiert 05.07.2026 | `avatar:health` im Avatar-Plugin, alle 30 Min im Zeitplan (#7833) |
| `backup-server-bigdata-watchdog.py` | **0**, pausiert 30.06.2026 | ZFS-Prüfung im Server-Collector, für alle drei TrueNAS-Kisten und **ohne Sudo** (#7808) |
| `bigdata-resilver-watchdog.py` | — | nichts: er bewachte einen Plattentausch, der durch ist |
| `ai-brain-post-reboot-check.sh` | nur von Hand | seine vier Dienste stehen in der Dienst-Überwachung (#7835) |

`ai-brain-mcp.sh` war der Starter für unseren MCP-Server — ersetzt durch den
Eintrag, den `ai-brain-harness configure` schreibt.

## `cron-jobs.json` — der eigentliche Lehrsatz

Beide Wächter standen mit Takt in dieser Datei, und beide waren `enabled:
false`. Der für das Backup hat **nie gelaufen**: `completed: 0`,
`last_run_at: null`, fünfzehn Minuten nach dem Anlegen pausiert.
Dreieinhalb Monate sah das aus wie Ruhe.

Ein Wächter meldet sich, **wenn** etwas nicht stimmt. Ein abgeschalteter
meldet sich ebenfalls nicht — und sieht damit genauso aus wie ein zufriedener.
Von diesen beiden Zuständen ist der falsche der beruhigende.

Deshalb hat jeder Dauerauftrag in AI Brain eine **Stillfrist**: bleibt die
Meldung aus, entsteht eine Aufgabe. Und deshalb gilt ein nie gelaufener
Dauerauftrag ab seinem Anlegen als verstummt, nicht erst ab der ersten
Meldung — sonst wäre genau dieser Fall wieder unsichtbar.

Siehe Wiki: „Ein pausierter Wächter sieht aus wie Ruhe".
