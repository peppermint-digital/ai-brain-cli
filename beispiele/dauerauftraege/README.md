# Beispiele für Daueraufträge

Ein *Dauerauftrag* ist eine wiederkehrende Aufgabe, die in AI Brain als **Daten**
liegt und auf einem Rechner läuft (#7812). Der Harness holt das Fällige mit
`ai-brain-harness daueraufgaben --fahren` und meldet zurück.

Die Beispiele hier sind die Vorlagen, nach denen ein Harness eigene anlegt —
und sie zeigen, was bei einer Prüfung schiefgeht, wenn man es nicht weiss.

## `restic-pruefung.sh`

Prüft das Restic-Backup dieses Rechners: lief der letzte Lauf durch, ist der
Taktgeber an, wie alt ist der jüngste Snapshot, wann war der Integritätslauf.

Anlegen als Dauerauftrag (`art: befehl`, kein Modell beteiligt):

```
create-device-job-tool
  name:                Restic-Backup prüfen
  art:                 befehl
  inhalt:              ~/.local/share/guardian-backup/restic-pruefung.sh
  takt_minuten:        1440
  stillfrist_minuten:  2160
  grund:               Bastian verlässt sich auf das Backup; bis 15.10.2026 hat es niemand geprüft
```

### Drei Dinge, die beim Bauen gemessen wurden

**Der jüngste Snapshot über alle Gruppen, nicht je Gruppe.**
`restic snapshots --latest 1` liefert einen Eintrag pro Pfad-Kombination, und
die Pfadliste eines Backups ändert sich über die Jahre. Am 15.10.2026 lagen in
diesem Repository sieben Gruppen — die älteste 204 Tage, weil ihre Pfade
(`.clawdbot`, `.openclaw`) längst nicht mehr gesichert werden. Eine Prüfung je
Gruppe hätte **sechs Fehlalarme** erzeugt. Nach drei Fehlalarmen sieht niemand
mehr hin, und damit wäre die Prüfung schlechter als keine.

**`ExecMainExitTimestamp`, nicht `ActiveEnterTimestamp`.**
Bei `Type=oneshot` ist `ActiveEnterTimestamp` nach dem Beenden **leer**. Eine
Altersprüfung darauf meldet „hat nie stattgefunden" — gemessen an
`restic-check.service`, das vier Tage zuvor erfolgreich gelaufen war.

**Erst die Existenz der Unit prüfen, dann den Zeitstempel.**
`systemctl show` liefert für eine unbekannte Unit leere Werte, ohne zu
scheitern. Beim Integritätslauf hiess das: fehlt der Dienst, holt der Rückfall
den Zeitstempel des *Taktgebers* — und die Prüfung meldet Gesundheit, obwohl
nichts gelaufen ist. Mit einer Mutationsprobe gefunden, nicht beim Lesen.

### Mutationsproben

Eine Prüfung, die nicht fehlschlagen *kann*, prüft nichts. Fünf Proben, alle
schlagen an: Snapshot-Grenze auf 1 h, Integritäts-Grenze auf 1 Tag,
Zugangsdaten fehlen, Taktgeber existiert nicht, Dienst existiert nicht.
