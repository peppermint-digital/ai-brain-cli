# Was ist ein Harness?

*Das Wort wird benutzt, als waere klar, was es heisst. Hier sind die sieben Teile — und welche davon wir selbst bauen.*

Das Wort wird benutzt, als waere klar, was es bedeutet. Ist es nicht. Am ehesten trifft es
das Bild aus dem Pferdegeschirr: Ein Harness ist nicht das Pferd. Es ist das, was das Pferd
mit dem Wagen verbindet, sodass aus Kraft Arbeit wird.

Ein Sprachmodell antwortet. Damit daraus *Arbeit* wird — Dateien aendern, Tests laufen
lassen, eine Aufgabe abschliessen — braucht es sieben Dinge drumherum. Die Summe davon ist der
Harness.

> **Schaubild:** Die sieben Teile. Entscheidend ist nicht, dass es sie gibt, sondern wer sie liefert — und ob man sie einzeln tauschen kann.
> (in der HTML-Fassung dieser Seite)

## Die sieben Teile, einzeln

### 1. Der Motor

Das Modell. Bei uns austauschbar ueber `--modell claude` oder `codex`.
Nichts im Harness haengt daran.

### 2. Die Schleife

Der Kern: Aufgabe lesen, Werkzeug aufrufen, Ergebnis lesen, weitermachen, bis fertig.
**Diesen Teil bauen wir nicht.** `claude -p` und `codex exec` bringen
ihn mit. Das ist die wichtigste Entscheidung am ganzen Entwurf — siehe unten.

### 3. Werkzeuge

Was der Lauf anfassen kann. Shell und Dateien kommen vom Motor; alles, was AI Brain kann,
kommt ueber MCP dazu — Aufgaben, Memories, Kalender, Server, Zeiterfassung.

### 4. Grenzen

Ohne Freigabe fragt ein Lauf vor jeder Tat. `--schreibend` und `--vollzugriff`
greifen nur in Verzeichnissen, die vorher freigegeben wurden. Das ist ein Riegel im Harness,
nicht im Modell.

### 5. Sitzung und Verlauf

Ein Lauf muss fortsetzbar sein. Auch das liefert der Motor; der Harness merkt sich nur,
*welche* Sitzung zu welchem Lauf gehoerte.

### 6. Wissen

Was der Agent weiss, bevor er anfaengt — Skills, frueher Gelerntes, der Stand des Projekts.
Das liegt bei uns vollstaendig in AI Brain. Eine eigene Seite dazu:
[Wo das Wissen liegt](gedaechtnis.md).

### 7. Betrieb

Starten, sehen was laeuft, stoppen, nachlesen. `agents`, `logs`,
`stop` — und ein Register auf der Platte, das auch einen Absturz ueberlebt.

## Die Entscheidung, die alles andere bestimmt

> **Wir bauen die Schleife nicht, wir mieten sie.** Deshalb ist der Harness klein genug, um ihn zu verstehen. Ein eigener Agentenkern waere das groesste Stueck Arbeit am ganzen Projekt gewesen — und das Stueck, das am schnellsten veraltet.

Die Folge sieht man an den Zahlen: Hermes hatte ueber siebzig Befehle, unsere CLI hat achtzehn.
Das ist kein Rueckstand. Alles, was dort im selben Prozess sass, liegt bei uns in einer Schicht,
die man einzeln anfassen kann.

## Woran man einen schlechten Harness erkennt

- **Er kennt nur ein Modell.** Dann ist der Motor kein Teil, sondern das Fundament.
- **Sein Gedaechtnis liegt auf der Platte.** Dann endet das Wissen am Rechner.
- **Seine Grenzen stehen im Prompt.** Eine Bitte ist kein Riegel.
- **Er meldet Erfolg, ohne nachzusehen.** Der haeufigste Fehler, und der teuerste.
