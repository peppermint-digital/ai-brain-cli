# Wie unserer gebaut ist

*Drei Schichten statt einem Programm. Warum AI Brain nirgends anruft, und was wo liegt.*

Hermes war ein Programm, in dem alles sass: Agentenschleife, Sitzungen, Messenger, Zeitplan,
Kanban, Gedaechtnis. Bei uns liegt dasselbe auf drei Schichten. Das ist kein Nachbau mit anderen
Mitteln, sondern eine andere Bauform.

> **Schaubild:** Wer Arbeit einstellt, wo sie liegt, wo sie ausgefuehrt wird. Die mittlere Schicht ruft niemals nach unten an.
> (in der HTML-Fassung dieser Seite)

## Warum das Holprinzip

AI Brain stellt nicht zu. Es legt einen Auftrag hin, und der Zielrechner fragt beim naechsten
Lauf, ob etwas fuer ihn da ist. Drei Gruende:

- **Kein Arbeitsplatz braucht einen offenen Port.** Die Vertrauensrichtung bleibt
immer Geraet zur Instanz.
- **Ein ausgeschalteter Rechner ist kein Fehlerfall.** Der Auftrag liegt einfach da.
- **Die Anmeldung ist dieselbe wie fuer alles andere.** Kein zweiter Weg mit
eigenen Geheimnissen.

> **Schaubild:** Der Weg eines Auftrags. Das Annehmen ist atomar — zwei Laeufe koennen denselben Auftrag nicht greifen.
> (in der HTML-Fassung dieser Seite)

## Was wo liegt

| Teil | Wo | Warum dort |
|---|---|---|
| Agentenschleife | gemietet (claude, codex) | Das aelteste Stueck zuerst; wir wuerden es nur schlechter nachbauen. |
| Hostzugriff | ai-brain-cli | Nur hier gibt es echten Shell- und Dateizugriff. NanoClaw hat ihn mit Absicht nicht. |
| Auftraege, Wissen, Zeit | AI Brain | Damit es den Rechner ueberlebt und zwischen Menschen geteilt wird. |
| Channels (WhatsApp, Mail) | NanoClaw | Container, ein Channel = ein Chat. Bewusst nicht angefasst beim Bau der CLI. |
| Oberflaeche | imkr, Terminal, Web | Austauschbar. imkr ist eine Oberflaeche davor, kein Teil des Harness. |

## Wie ein Lauf ablaeuft

```bash
ai-brain-cli agent --task 7779 --projekt ai-brain --imkr --schreibend
```

- Aufgabentext aus AI Brain holen (ueber MCP, mit dem Geraetezugang).
- Pruefen, ob das Arbeitsverzeichnis freigegeben ist — sonst bleibt es beim Fragen.
- Kennung vergeben und im Register eintragen, bevor etwas startet.
- `claude -p` starten, bei `--imkr` in einem eigenen Pane.
- Der Lauf arbeitet und schreibt sein Ergebnis selbst nach AI Brain — Memories, Zeit, Status.
- Das Register merkt sich, wie es ausging. `agents`, `logs`, `stop`
greifen darauf zu.

> **Eine Grenze, die man kennen muss.** Der Harness weiss nicht, ob die *Arbeit* getan ist — nur, ob der *Lauf* durchgelaufen ist. Deshalb schliesst ein erfolgreicher Lauf die Aufgabe nicht ab. Das bleibt eine Entscheidung, die ein Mensch trifft oder der Agent ausdruecklich per `complete-task-tool`.

## Was austauschbar ist

Jede Schicht fuer sich. Ein anderes Modell: ein Schalter. Eine andere Oberflaeche: andockbar
ueber MCP. Ein anderer Rechner: ein weiterer Geraetezugang. Was nicht austauschbar ist, ist
AI Brain selbst — es ist der Ort, an dem die Wahrheit liegt, und genau das ist Absicht.
