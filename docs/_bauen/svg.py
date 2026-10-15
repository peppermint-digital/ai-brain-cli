# -*- coding: utf-8 -*-
"""Diagramme als eingebettetes SVG — kein CDN, laeuft im Vault wie im Repo."""

def _t(x, y, s, groesse=13, farbe='var(--ink)', gewicht='400', anker='start'):
    return ('<text x="{}" y="{}" font-family="IBM Plex Sans,sans-serif" font-size="{}" '
            'fill="{}" font-weight="{}" text-anchor="{}">{}</text>').format(x, y, groesse, farbe, gewicht, anker, s)

def _box(x, y, w, h, farbe, titel, zeilen, fuell='var(--surface)'):
    o = ['<rect x="{}" y="{}" width="{}" height="{}" rx="8" fill="{}" stroke="{}" stroke-width="1.5"/>'
         .format(x, y, w, h, fuell, farbe)]
    o.append(_t(x + 14, y + 24, titel, 14, farbe, '700'))
    for i, z in enumerate(zeilen):
        o.append(_t(x + 14, y + 46 + i * 19, z, 12.5, 'var(--muted)'))
    return ''.join(o)

def _pfeil(x1, y1, x2, y2, beschriftung='', farbe='var(--muted)'):
    o = ['<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}" stroke-width="1.5" marker-end="url(#spitze)"/>'
         .format(x1, y1, x2, y2, farbe)]
    if beschriftung:
        o.append(_t((x1 + x2) / 2, (y1 + y2) / 2 - 7, beschriftung, 11.5, farbe, '500', 'middle'))
    return ''.join(o)

DEFS = ('<defs><marker id="spitze" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" '
        'orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="var(--muted)"/></marker></defs>')

def huelle(w, h, inhalt):
    return '<svg viewBox="0 0 {} {}" width="{}" height="{}" role="img">{}{}</svg>'.format(w, h, w, h, DEFS, inhalt)

# ---------------------------------------------------------------- Teile eines Harness
def teile_eines_harness():
    i = [_t(20, 24, 'Was zwischen Modell und getaner Arbeit liegt', 14, 'var(--ink)', '700')]
    teile = [
        ('1 Motor',        ['das Modell selbst',        'claude, codex, …'],      'var(--warn)',  'gemietet'),
        ('2 Schleife',     ['Werkzeug rufen, Ergebnis', 'lesen, weitermachen'],   'var(--warn)',  'gemietet'),
        ('3 Werkzeuge',    ['Shell, Dateien, MCP'],                               'var(--cli)',   'unseres'),
        ('4 Grenzen',      ['was ein Lauf anfassen',    'darf'],                  'var(--cli)',   'unseres'),
        ('5 Sitzung',      ['fortsetzen koennen'],                                'var(--warn)',  'gemietet'),
        ('6 Wissen',       ['Skills, Memories,',        'Aufgaben'],              'var(--brain)', 'AI Brain'),
        ('7 Betrieb',      ['starten, sehen, stoppen', 'protokollieren'],         'var(--cli)',   'unseres'),
    ]
    x, y = 20, 44
    for n, (titel, zeilen, farbe, herkunft) in enumerate(teile):
        sp, ze = n % 4, n // 4
        bx, by = x + sp * 185, y + ze * 112
        i.append(_box(bx, by, 170, 96, farbe, titel, zeilen))
        i.append(_t(bx + 156, by + 88, herkunft, 10.5, farbe, '600', 'end'))
    i.append(_t(20, 290, 'Orange = vom Werkzeug gemietet  ·  Gruen = in der CLI  ·  Blau = in AI Brain',
                12, 'var(--muted)'))
    return huelle(770, 305, ''.join(i))

# ---------------------------------------------------------------- Drei Schichten
def drei_schichten():
    i = []
    i.append(_box(20, 20, 710, 70, 'var(--muted)', 'Wer Arbeit einstellt',
                  ['Web-Oberflaeche  ·  Avatar  ·  Channels (WhatsApp, Mail, …)  ·  Terminal']))
    i.append(_box(20, 120, 710, 96, 'var(--brain)', 'AI Brain — das Gedaechtnis und die Auftraege',
                  ['Aufgaben  ·  Bugs  ·  Memories  ·  Wiki  ·  Zeit  ·  Pruefspur  ·  Zeitplan',
                   'Spricht nur MCP. Legt Auftraege hin — ruft nirgends an.']))
    i.append(_box(20, 258, 345, 112, 'var(--cli)', 'ai-brain-cli — auf deinem Rechner',
                  ['Hostzugriff: Shell, Dateien, SSH', 'Geraetezugang = eine Person',
                   'mietet claude -p / codex exec']))
    i.append(_box(385, 258, 345, 112, 'var(--nano)', 'NanoClaw — im Container',
                  ['kein Hostzugriff, mit Absicht', 'ein Channel = ein Chat',
                   'eigenes Gedaechtnis je Gruppe']))
    i.append(_pfeil(375, 90, 375, 118, 'legt ein'))
    i.append(_pfeil(192, 218, 192, 256, 'holt ab'))
    i.append(_pfeil(558, 218, 558, 256, 'bekommt'))
    i.append(_t(20, 392, 'Jede Schicht ist fuer sich austauschbar. Das ist der Unterschied zu einem Programm, das alles selbst macht.',
                12, 'var(--muted)'))
    return huelle(750, 405, ''.join(i))

# ---------------------------------------------------------------- Gedaechtnis: Hermes vs. wir
def gedaechtnis():
    i = [_t(20, 20, 'Hermes', 15, 'var(--warn)', '700'),
         _t(400, 20, 'ai-brain-cli', 15, 'var(--cli)', '700')]
    i.append(_box(20, 34, 330, 86, 'var(--warn)', 'Ein Rechner, ein Mensch',
                  ['Gedaechtnis lag auf der Platte,', 'unter dem eigenen Konto.']))
    i.append(_box(20, 136, 330, 104, 'var(--warn)', 'Die Folgen',
                  ['Niemand sonst sah es.', 'Zweiter Mensch = zweites Gedaechtnis.',
                   'Rechner weg = Wissen weg.']))
    i.append(_box(400, 34, 330, 86, 'var(--cli)', 'Lokal bleibt nur Handwerkszeug',
                  ['Zugangsdaten, Token, Abgleichstand,', 'Freigaben, Laufprotokolle.']))
    i.append(_box(400, 136, 330, 104, 'var(--brain)', 'Wissen geht nach AI Brain',
                  ['Memories je Projekt  ·  Wiki global', 'Aufgaben  ·  Bugs  ·  Zeit',
                   'mit der Person daran, die es schrieb.']))
    i.append(_pfeil(565, 120, 565, 134, '', 'var(--brain)'))
    i.append(_t(20, 268, 'Derselbe Rechner, zwei Menschen: zwei Zugaenge, ein gemeinsames Projektwissen — je Eintrag steht, wer ihn schrieb.',
                12, 'var(--muted)'))
    return huelle(750, 282, ''.join(i))

# ---------------------------------------------------------------- Mehrbenutzer
def mehrbenutzer():
    i = [_t(20, 20, 'Ein Rechner kann mehrere Zugaenge tragen — einen je Konto', 14, 'var(--ink)', '700')]
    i.append(_box(20, 36, 215, 92, 'var(--cli)', 'Konto codingmachine',
                  ['~/.config/ai-brain-cli/', 'Zugang 587364a4', 'Besitzer: Bastian']))
    i.append(_box(255, 36, 215, 92, 'var(--cli)', 'Konto bastian',
                  ['/home/bastian/.config/…', 'Zugang 85649ea1', 'Besitzer: Bastian']))
    i.append(_box(490, 36, 240, 92, 'var(--cli)', 'Laptop, Konto bastian',
                  ['Zugang a53112c7', 'Besitzer: Bastian', 'eigene Auswahl, eigene Laeufe']))
    i.append(_box(20, 186, 710, 92, 'var(--brain)', 'AI Brain — eine Wahrheit fuer alle',
                  ['Jeder Zugang gehoert einer PERSON. Was ein Lauf schreibt, traegt diese Person.',
                   'Geteilt: Projektwissen, Aufgaben, Wiki.   Je Geraet: Repository-Auswahl, Laufliste, Freigaben.']))
    for x in (127, 362, 610):
        i.append(_pfeil(x, 130, x, 184))
    return huelle(750, 292, ''.join(i))

# ---------------------------------------------------------------- Weg eines Auftrags
def auftragsweg():
    i = []
    stationen = [
        (20,  'Mensch', ['Aufgabe anlegen', 'UI, Avatar, Channel']),
        (205, 'AI Brain', ['Auftrag hinlegen', 'fuer EINEN Rechner']),
        (390, 'Rechner', ['auftraege / auftrag', 'annehmen, atomar']),
        (575, 'Lauf', ['claude -p / codex', 'mit Hostzugriff']),
    ]
    for x, titel, zeilen in stationen:
        i.append(_box(x, 30, 155, 86, 'var(--brain)' if titel == 'AI Brain' else 'var(--cli)', titel, zeilen))
    for x in (175, 360, 545):
        i.append(_pfeil(x, 73, x + 29, 73))
    i.append(_pfeil(652, 120, 652, 150))
    i.append(_box(390, 152, 340, 68, 'var(--brain)', 'Ergebnis zurueck',
                  ['completed oder failed  ·  Pruefspur im Audit']))
    i.append(_pfeil(388, 186, 290, 186, 'meldet'))
    i.append(_t(20, 244, 'AI Brain ruft nirgends an. Der Rechner fragt — deshalb braucht kein Arbeitsplatz einen offenen Port.',
                12, 'var(--muted)'))
    return huelle(750, 258, ''.join(i))
