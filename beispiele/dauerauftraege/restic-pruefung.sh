#!/bin/bash
# Dauerauftrag: Restic-Backup pruefen (#7816).
#
# Gibt eine Zeile je Befund aus und endet mit 0 (alles gut), 1 (Fehler).
# Der Dauerauftrag meldet Ausgabe und Rueckgabewert an AI Brain zurueck.
set -u

ENV_DATEI="$HOME/.local/share/guardian-backup/backup.env"
MAX_ALTER_STUNDEN=26          # taeglicher Lauf plus Puffer
MAX_PRUEFUNG_TAGE=9           # woechentlicher Integritaetslauf plus Puffer

fehler=0
melde() { printf '%s\n' "$1"; }
schlimm() { melde "FEHLER: $1"; fehler=1; }

# Existiert die Unit ueberhaupt?
#
# Ohne diese Pruefung meldet `systemctl show` fuer eine unbekannte Unit leere
# Werte, und jede darauf gebaute Auswertung liest daraus, was sie will. Beim
# Integritaetslauf hiess das konkret: fehlt der Dienst, holt der Rueckfall den
# Zeitstempel des TAKTGEBERS — und die Pruefung meldet Gesundheit, obwohl
# nichts gelaufen ist. Mit einer Mutationsprobe gefunden.
gibt_es() {
    [ "$(systemctl show "$1" -p LoadState --value 2>/dev/null)" = "loaded" ]
}

for unit in restic-backup.service restic-backup.timer restic-check.service restic-check.timer; do
    gibt_es "$unit" || schlimm "$unit gibt es auf diesem Rechner nicht"
done

# 1) Lief der letzte Lauf durch?
ergebnis=$(systemctl show restic-backup.service -p Result --value 2>/dev/null)
[ "$ergebnis" = "success" ] || schlimm "letzter Backup-Lauf: $ergebnis"

# 2) Ist der Taktgeber ueberhaupt an? Ein abgeschalteter Timer ist still.
timer=$(systemctl is-active restic-backup.timer 2>/dev/null)
[ "$timer" = "active" ] || schlimm "restic-backup.timer ist $timer"

# 3) Wie alt ist der juengste Snapshot?
#
# Bewusst das Maximum ueber ALLE Gruppen, nicht je Gruppe. `restik snapshots
# --latest 1` liefert einen Eintrag pro Pfad-Kombination, und die Pfadliste des
# Backups hat sich ueber die Jahre geaendert: am 15.10.2026 lagen sechs
# historische Gruppen im Repository, die aelteste 204 Tage alt. Eine Pruefung je
# Gruppe erzeugte sechs Fehlalarme — und nach drei Fehlalarmen sieht niemand
# mehr hin.
if [ ! -r "$ENV_DATEI" ]; then
    schlimm "$ENV_DATEI nicht lesbar — ohne Zugangsdaten keine Pruefung"
else
    set -a; . "$ENV_DATEI"; set +a

    roh=$(timeout 120 restic snapshots --latest 1 --json 2>&1)

    if [ $? -ne 0 ]; then
        schlimm "Repository nicht erreichbar: $(printf '%s' "$roh" | tail -1)"
    else
        alter=$(printf '%s' "$roh" | python3 -c '
import json,sys,datetime
try:
    d=json.load(sys.stdin)
except Exception:
    print("-1"); raise SystemExit
if not d:
    print("-1"); raise SystemExit
jetzt=datetime.datetime.now(datetime.timezone.utc)
juengster=max(datetime.datetime.fromisoformat(s["time"]) for s in d)
print(int((jetzt-juengster).total_seconds()//3600))
')

        if [ "$alter" = "-1" ]; then
            schlimm "Snapshot-Liste nicht auswertbar"
        elif [ "$alter" -gt "$MAX_ALTER_STUNDEN" ]; then
            schlimm "juengster Snapshot ist ${alter}h alt (Grenze ${MAX_ALTER_STUNDEN}h)"
        else
            melde "juengster Snapshot: ${alter}h alt"
        fi
    fi
fi

# 4) Wie lange her ist der Integritaetslauf?
#
# `ExecMainExitTimestamp`, nicht `ActiveEnterTimestamp`: Bei `Type=oneshot` ist
# letzterer nach dem Beenden LEER. Eine Pruefung darauf meldet „hat nie
# stattgefunden", obwohl der Lauf vier Tage her ist — gemessen am 15.10.2026 an
# restic-check.service, das am 11.10. erfolgreich lief. Der Taktgeber
# (`LastTriggerUSec`) ist der Rueckfall, falls die Unit zwischenzeitlich neu
# geladen wurde.
pruefung=$(systemctl show restic-check.service -p ExecMainExitTimestamp --value 2>/dev/null)

if [ -z "$pruefung" ] || [ "$pruefung" = "n/a" ]; then
    pruefung=$(systemctl show restic-check.timer -p LastTriggerUSec --value 2>/dev/null)
fi

if [ -z "$pruefung" ] || [ "$pruefung" = "n/a" ]; then
    schlimm "Integritaetslauf hat noch nie stattgefunden"
else
    dann=$(date -d "$pruefung" +%s 2>/dev/null || echo 0)
    if [ "$dann" -eq 0 ]; then
        schlimm "Zeitpunkt des Integritaetslaufs nicht lesbar: $pruefung"
    else
        tage=$(( ( $(date +%s) - dann ) / 86400 ))
        if [ "$tage" -gt "$MAX_PRUEFUNG_TAGE" ]; then
            schlimm "Integritaetslauf ist $tage Tage her (Grenze $MAX_PRUEFUNG_TAGE)"
        else
            melde "Integritaetslauf: vor $tage Tagen"
        fi
    fi
fi

ergebnis_pruefung=$(systemctl show restic-check.service -p Result --value 2>/dev/null)
[ "$ergebnis_pruefung" = "success" ] || schlimm "letzter Integritaetslauf: $ergebnis_pruefung"

exit $fehler
