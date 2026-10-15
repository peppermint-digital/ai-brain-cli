# Uebersicht

*Was dieser Harness ist, woraus er besteht, wo das Wissen liegt und wer ihn benutzen kann.*

Diese Doku beantwortet vier Fragen, die beim Bauen immer wieder aufkamen.
Sie ist fuer Menschen geschrieben, nicht fuer Maschinen.

## Die vier Fragen

| Frage | Kurzfassung | Seite |
|---|---|---|
| Was ist ueberhaupt ein Harness? | Alles, was zwischen einem Modell und getaner Arbeit liegt. Sieben Teile. | Was ist ein Harness? |
| Wie ist unserer gebaut? | Drei Schichten statt einem Programm. Das Denken ist gemietet. | Wie unserer gebaut ist |
| Wo liegt das Wissen? | In AI Brain. Auf dem Rechner bleibt nur Handwerkszeug. | Wo das Wissen liegt |
| Sind wir mehrbenutzerfaehig? | Ja — ein Geraetezugang je Konto, eine gemeinsame Wahrheit. | Mehrere Menschen |

## In drei Saetzen

**ai-brain-cli** ist ein Rahmen, der auf deinen Rechnern laeuft und das Denken
*mietet* — er startet `claude -p` oder `codex exec` und kuemmert sich um alles
drumherum: Rechte, Sitzungen, Protokoll, Auftraege, Nachrichten, Wissensabgleich.

**AI Brain** ist das Gedaechtnis und die Auftragsablage. Es ruft nirgends an; es legt hin,
und die Rechner holen ab.

**Ein Geraetezugang** verbindet beides. Er gehoert einer Person, ist einzeln entziehbar,
und aus ihm folgt alles andere: welche Auftraege ein Rechner sieht, welche Nachrichten er bekommt,
wem ein geschriebener Gedanke zugeschrieben wird.

## Sofort loslegen

```bash
# einmal pro Rechner
curl -fsSL https://brain.proxy.peppermint-digital.com/install.sh | sh
ai-brain-cli connect          # nur beim ersten Mal
ai-brain-cli configure        # MCP in Claude Code und Codex eintragen
ai-brain-cli sync --apply     # Skills und Agents auf die Platte
ai-brain-cli freigabe add ~/Development

# taeglich
ai-brain-cli aufgaben --projekt ai-brain
ai-brain-cli agent --task 7779 --imkr --schreibend
ai-brain-cli auftraege
```

Alle Befehle mit Schaltern: [Alle Befehle](befehle.md).

> **Was diese Doku nicht ist.** Kein Vergleich mit anderen Harnessen und keine Begruendung, warum man einen braucht. Sie beschreibt, was gebaut ist, und zwar so, dass man es nachpruefen kann — jede Zahl darin ist gemessen, nicht geschaetzt.
