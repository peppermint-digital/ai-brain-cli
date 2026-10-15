# -*- coding: utf-8 -*-
import svg

INDEX = ('''
<p>Diese Doku beantwortet vier Fragen, die beim Bauen immer wieder aufkamen.
Sie ist fuer Menschen geschrieben, nicht fuer Maschinen.</p>

<h2>Die vier Fragen</h2>
<table>
<tr><th>Frage</th><th>Kurzfassung</th><th>Seite</th></tr>
<tr><td>Was ist ueberhaupt ein Harness?</td>
    <td>Alles, was zwischen einem Modell und getaner Arbeit liegt. Sieben Teile.</td>
    <td><a href="was-ist-ein-harness.html">Was ist ein Harness?</a></td></tr>
<tr><td>Wie ist unserer gebaut?</td>
    <td>Drei Schichten statt einem Programm. Das Denken ist gemietet.</td>
    <td><a href="architektur.html">Wie unserer gebaut ist</a></td></tr>
<tr><td>Wo liegt das Wissen?</td>
    <td>In AI Brain. Auf dem Rechner bleibt nur Handwerkszeug.</td>
    <td><a href="gedaechtnis.html">Wo das Wissen liegt</a></td></tr>
<tr><td>Sind wir mehrbenutzerfaehig?</td>
    <td>Ja — ein Geraetezugang je Konto, eine gemeinsame Wahrheit.</td>
    <td><a href="mehrbenutzer.html">Mehrere Menschen</a></td></tr>
</table>

<h2>In drei Saetzen</h2>
<p><strong>ai-brain-cli</strong> ist ein Rahmen, der auf deinen Rechnern laeuft und das Denken
<em>mietet</em> — er startet <code>claude -p</code> oder <code>codex exec</code> und kuemmert sich um alles
drumherum: Rechte, Sitzungen, Protokoll, Auftraege, Nachrichten, Wissensabgleich.</p>
<p><strong>AI Brain</strong> ist das Gedaechtnis und die Auftragsablage. Es ruft nirgends an; es legt hin,
und die Rechner holen ab.</p>
<p><strong>Ein Geraetezugang</strong> verbindet beides. Er gehoert einer Person, ist einzeln entziehbar,
und aus ihm folgt alles andere: welche Auftraege ein Rechner sieht, welche Nachrichten er bekommt,
wem ein geschriebener Gedanke zugeschrieben wird.</p>

<h2>Sofort loslegen</h2>
<pre><code><span class="d"># einmal pro Rechner</span>
curl -fsSL https://brain.proxy.peppermint-digital.com/install.sh | sh
ai-brain-cli connect          <span class="d"># nur beim ersten Mal</span>
ai-brain-cli configure        <span class="d"># MCP in Claude Code und Codex eintragen</span>
ai-brain-cli sync --apply     <span class="d"># Skills und Agents auf die Platte</span>
ai-brain-cli freigabe add ~/Development

<span class="d"># taeglich</span>
ai-brain-cli aufgaben --projekt ai-brain
ai-brain-cli agent --task 7779 --imkr --schreibend
ai-brain-cli auftraege</code></pre>
<p>Alle Befehle mit Schaltern: <a href="befehle.html">Alle Befehle</a>.</p>

<div class="merk"><p><strong>Was diese Doku nicht ist.</strong> Kein Vergleich mit anderen Harnessen
und keine Begruendung, warum man einen braucht. Sie beschreibt, was gebaut ist, und zwar so,
dass man es nachpruefen kann — jede Zahl darin ist gemessen, nicht geschaetzt.</p></div>
''')

HARNESS = ('''
<p>Das Wort wird benutzt, als waere klar, was es bedeutet. Ist es nicht. Am ehesten trifft es
das Bild aus dem Pferdegeschirr: Ein Harness ist nicht das Pferd. Es ist das, was das Pferd
mit dem Wagen verbindet, sodass aus Kraft Arbeit wird.</p>

<p>Ein Sprachmodell antwortet. Damit daraus <em>Arbeit</em> wird — Dateien aendern, Tests laufen
lassen, eine Aufgabe abschliessen — braucht es sieben Dinge drumherum. Die Summe davon ist der
Harness.</p>

<figure>''' + svg.teile_eines_harness() + '''
<figcaption>Die sieben Teile. Entscheidend ist nicht, dass es sie gibt, sondern wer sie
liefert — und ob man sie einzeln tauschen kann.</figcaption></figure>

<h2>Die sieben Teile, einzeln</h2>

<h3>1. Der Motor</h3>
<p>Das Modell. Bei uns austauschbar ueber <code>--modell claude</code> oder <code>codex</code>.
Nichts im Harness haengt daran.</p>

<h3>2. Die Schleife</h3>
<p>Der Kern: Aufgabe lesen, Werkzeug aufrufen, Ergebnis lesen, weitermachen, bis fertig.
<strong>Diesen Teil bauen wir nicht.</strong> <code>claude -p</code> und <code>codex exec</code> bringen
ihn mit. Das ist die wichtigste Entscheidung am ganzen Entwurf — siehe unten.</p>

<h3>3. Werkzeuge</h3>
<p>Was der Lauf anfassen kann. Shell und Dateien kommen vom Motor; alles, was AI Brain kann,
kommt ueber MCP dazu — Aufgaben, Memories, Kalender, Server, Zeiterfassung.</p>

<h3>4. Grenzen</h3>
<p>Ohne Freigabe fragt ein Lauf vor jeder Tat. <code>--schreibend</code> und <code>--vollzugriff</code>
greifen nur in Verzeichnissen, die vorher freigegeben wurden. Das ist ein Riegel im Harness,
nicht im Modell.</p>

<h3>5. Sitzung und Verlauf</h3>
<p>Ein Lauf muss fortsetzbar sein. Auch das liefert der Motor; der Harness merkt sich nur,
<em>welche</em> Sitzung zu welchem Lauf gehoerte.</p>

<h3>6. Wissen</h3>
<p>Was der Agent weiss, bevor er anfaengt — Skills, frueher Gelerntes, der Stand des Projekts.
Das liegt bei uns vollstaendig in AI Brain. Eine eigene Seite dazu:
<a href="gedaechtnis.html">Wo das Wissen liegt</a>.</p>

<h3>7. Betrieb</h3>
<p>Starten, sehen was laeuft, stoppen, nachlesen. <code>agents</code>, <code>logs</code>,
<code>stop</code> — und ein Register auf der Platte, das auch einen Absturz ueberlebt.</p>

<h2>Die Entscheidung, die alles andere bestimmt</h2>
<div class="merk"><p><strong>Wir bauen die Schleife nicht, wir mieten sie.</strong> Deshalb ist der
Harness klein genug, um ihn zu verstehen. Ein eigener Agentenkern waere das groesste Stueck
Arbeit am ganzen Projekt gewesen — und das Stueck, das am schnellsten veraltet.</p></div>
<p>Die Folge sieht man an den Zahlen: Hermes hatte ueber siebzig Befehle, unsere CLI hat achtzehn.
Das ist kein Rueckstand. Alles, was dort im selben Prozess sass, liegt bei uns in einer Schicht,
die man einzeln anfassen kann.</p>

<h2>Woran man einen schlechten Harness erkennt</h2>
<ul>
<li><strong>Er kennt nur ein Modell.</strong> Dann ist der Motor kein Teil, sondern das Fundament.</li>
<li><strong>Sein Gedaechtnis liegt auf der Platte.</strong> Dann endet das Wissen am Rechner.</li>
<li><strong>Seine Grenzen stehen im Prompt.</strong> Eine Bitte ist kein Riegel.</li>
<li><strong>Er meldet Erfolg, ohne nachzusehen.</strong> Der haeufigste Fehler, und der teuerste.</li>
</ul>
''')
