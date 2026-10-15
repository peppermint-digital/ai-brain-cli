# -*- coding: utf-8 -*-
import svg

ARCHITEKTUR = ('''
<p>Hermes war ein Programm, in dem alles sass: Agentenschleife, Sitzungen, Messenger, Zeitplan,
Kanban, Gedaechtnis. Bei uns liegt dasselbe auf drei Schichten. Das ist kein Nachbau mit anderen
Mitteln, sondern eine andere Bauform.</p>

<figure>''' + svg.drei_schichten() + '''
<figcaption>Wer Arbeit einstellt, wo sie liegt, wo sie ausgefuehrt wird. Die mittlere Schicht
ruft niemals nach unten an.</figcaption></figure>

<h2>Warum das Holprinzip</h2>
<p>AI Brain stellt nicht zu. Es legt einen Auftrag hin, und der Zielrechner fragt beim naechsten
Lauf, ob etwas fuer ihn da ist. Drei Gruende:</p>
<ul>
<li><strong>Kein Arbeitsplatz braucht einen offenen Port.</strong> Die Vertrauensrichtung bleibt
immer Geraet zur Instanz.</li>
<li><strong>Ein ausgeschalteter Rechner ist kein Fehlerfall.</strong> Der Auftrag liegt einfach da.</li>
<li><strong>Die Anmeldung ist dieselbe wie fuer alles andere.</strong> Kein zweiter Weg mit
eigenen Geheimnissen.</li>
</ul>

<figure>''' + svg.auftragsweg() + '''
<figcaption>Der Weg eines Auftrags. Das Annehmen ist atomar — zwei Laeufe koennen denselben
Auftrag nicht greifen.</figcaption></figure>

<h2>Was wo liegt</h2>
<table>
<tr><th>Teil</th><th>Wo</th><th>Warum dort</th></tr>
<tr><td>Agentenschleife</td><td>gemietet (claude, codex)</td><td>Das aelteste Stueck zuerst; wir
wuerden es nur schlechter nachbauen.</td></tr>
<tr><td>Hostzugriff</td><td>ai-brain-cli</td><td>Nur hier gibt es echten Shell- und Dateizugriff.
NanoClaw hat ihn mit Absicht nicht.</td></tr>
<tr><td>Auftraege, Wissen, Zeit</td><td>AI Brain</td><td>Damit es den Rechner ueberlebt und
zwischen Menschen geteilt wird.</td></tr>
<tr><td>Channels (WhatsApp, Mail)</td><td>NanoClaw</td><td>Container, ein Channel = ein Chat.
Bewusst nicht angefasst beim Bau der CLI.</td></tr>
<tr><td>Oberflaeche</td><td>imkr, Terminal, Web</td><td>Austauschbar. imkr ist eine Oberflaeche
davor, kein Teil des Harness.</td></tr>
</table>

<h2>Wie ein Lauf ablaeuft</h2>
<pre><code>ai-brain-cli agent --task 7779 --projekt ai-brain --imkr --schreibend</code></pre>
<ol>
<li>Aufgabentext aus AI Brain holen (ueber MCP, mit dem Geraetezugang).</li>
<li>Pruefen, ob das Arbeitsverzeichnis freigegeben ist — sonst bleibt es beim Fragen.</li>
<li>Kennung vergeben und im Register eintragen, bevor etwas startet.</li>
<li><code>claude -p</code> starten, bei <code>--imkr</code> in einem eigenen Pane.</li>
<li>Der Lauf arbeitet und schreibt sein Ergebnis selbst nach AI Brain — Memories, Zeit, Status.</li>
<li>Das Register merkt sich, wie es ausging. <code>agents</code>, <code>logs</code>, <code>stop</code>
greifen darauf zu.</li>
</ol>

<div class="warn"><p><strong>Eine Grenze, die man kennen muss.</strong> Der Harness weiss nicht, ob
die <em>Arbeit</em> getan ist — nur, ob der <em>Lauf</em> durchgelaufen ist. Deshalb schliesst ein
erfolgreicher Lauf die Aufgabe nicht ab. Das bleibt eine Entscheidung, die ein Mensch trifft
oder der Agent ausdruecklich per <code>complete-task-tool</code>.</p></div>

<h2>Was austauschbar ist</h2>
<p>Jede Schicht fuer sich. Ein anderes Modell: ein Schalter. Eine andere Oberflaeche: andockbar
ueber MCP. Ein anderer Rechner: ein weiterer Geraetezugang. Was nicht austauschbar ist, ist
AI Brain selbst — es ist der Ort, an dem die Wahrheit liegt, und genau das ist Absicht.</p>
''')

GEDAECHTNIS = ('''
<p>Das war die Frage, die am laengsten offen war. Hermes war ein Einzelplatz-Programm und hat sein
Gedaechtnis auf der Platte gefuehrt — unter dem Konto, mit dem es lief. Fuer einen Menschen an
einem Rechner ist das richtig. Fuer mehrere ist es eine Sackgasse.</p>

<figure>''' + svg.gedaechtnis() + '''
<figcaption>Links der alte Zuschnitt, rechts unserer. Der Unterschied ist nicht die Technik,
sondern wem das Wissen gehoert.</figcaption></figure>

<h2>Was auf dem Rechner bleibt</h2>
<p>Unter <code>~/.config/ai-brain-cli/</code> — und das ist <em>alles</em>:</p>
<table>
<tr><th>Datei</th><th>Was</th><th>Verlust, wenn weg?</th></tr>
<tr><td><code>connection.json</code></td><td>Zugangsdaten des Geraets (0600)</td><td>Neu koppeln.</td></tr>
<tr><td><code>token.json</code></td><td>Zugangstoken, eine Stunde gueltig</td><td>Nichts, wird neu geholt.</td></tr>
<tr><td><code>sync-state.json</code></td><td>Welcher Stand zuletzt abgeglichen wurde</td><td>Der naechste Abgleich vergleicht Dateien statt Pruefsummen.</td></tr>
<tr><td><code>freigaben.json</code></td><td>Welche Verzeichnisse mehr duerfen</td><td>Laeufe fragen wieder vor jeder Tat.</td></tr>
<tr><td><code>gesicherte-fassungen/</code></td><td>Was <code>--force</code> ueberschrieben hat</td><td>Die Sicherungskopien.</td></tr>
<tr><td><code>…/state/ai-brain-cli/laeufe/</code></td><td>Laufregister und Protokolle</td><td>Die Liste vergangener Laeufe.</td></tr>
</table>
<p><strong>Kein Gedanke, keine Notiz, kein Gelerntes.</strong> Nichts davon ist Wissen — es ist
Handwerkszeug. Wer das Verzeichnis loescht, verliert keinen Inhalt.</p>

<h2>Wohin das Wissen geht</h2>
<p>Alles, was ein Lauf lernt, schreibt er ueber MCP nach AI Brain. Dort liegt es zweifach:
als Markdown-Datei in <code>~/knowledge-base/</code> und als Zeile in der Datenbank fuer die Suche.
Die Datei ist die Quelle, die Datenbank der Index.</p>
<table>
<tr><th>Art</th><th>Werkzeug</th><th>Liegt unter</th><th>Reichweite</th></tr>
<tr><td>Projektwissen</td><td><code>add-memory-tool</code></td><td><code>projekte/&lt;slug&gt;/memories/</code></td><td>ein Projekt</td></tr>
<tr><td>Uebergreifendes</td><td><code>add-wiki-tool</code></td><td><code>wiki/</code></td><td>alle Projekte</td></tr>
<tr><td>Fehler</td><td><code>create-bug-tool</code></td><td><code>projekte/&lt;slug&gt;/bugs/</code></td><td>ein Projekt</td></tr>
<tr><td>Aufgaben, Zeit, Pruefspur</td><td>eigene Werkzeuge</td><td>Datenbank</td><td>alle</td></tr>
</table>
<p>Gemessen am 15.10.2026: <strong>4239 Eintraege</strong> ueber <strong>47 Projekte</strong>.
An 4227 davon haengt die Person, die sie geschrieben hat.</p>

<h2>Ein drittes Gedaechtnis, das nicht unseres ist</h2>
<div class="merk"><p>In der Datenbank steht ausserdem <code>agent_memories</code> mit rund 8600
Zeilen. Das gehoert <strong>NanoClaw</strong>, nicht der CLI: Es ist das Gedaechtnis der
Container, abgegrenzt je Channel-Gruppe — <code>avatar-3</code>, <code>email_henneberg</code> und so
weiter. Wer dort sucht, sucht am falschen Ort.</p></div>

<h2>Der Unterschied in einem Satz</h2>
<p>Bei Hermes gehoerte das Gedaechtnis dem <em>Rechner</em>. Bei uns gehoert es dem
<em>Projekt</em> — und jeder Eintrag weiss, welche Person ihn beigesteuert hat.</p>

<h2>Was das praktisch heisst</h2>
<ul>
<li>Ein Lauf auf dem Laptop und einer auf dem Desktop schreiben in dasselbe Projektwissen.</li>
<li>Ein zweiter Mensch am selben Rechner schreibt ebenfalls dorthin — unter seinem Namen.</li>
<li>Ein neu aufgesetzter Rechner ist nach <code>connect</code> und <code>sync</code> sofort auf Stand.</li>
<li>Was ein Agent gestern gelernt hat, steht heute im Kontext — unabhaengig davon, wo er lief.</li>
</ul>
''')
