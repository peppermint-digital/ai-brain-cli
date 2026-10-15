# -*- coding: utf-8 -*-
CSS = '''
:root{--bg:#f7f6f2;--surface:#fff;--ink:#1c1e23;--muted:#5c616c;--line:#e0ddd4;
--akzent:#2f6f4e;--akzent-weich:#dfece5;--warn:#b4532a;--warn-weich:#f7e6dd;
--term-bg:#16181d;--term-fg:#d8dae0;--term-dim:#7b818d;--brain:#3a6ea8;--cli:#2f6f4e;--nano:#8a5fb0;}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]){--bg:#15171b;--surface:#1d2026;
--ink:#e7e8ea;--muted:#9aa0ab;--line:#2d313a;--akzent:#6cc295;--akzent-weich:#1d3429;
--warn:#e08a5f;--warn-weich:#3a2419;--term-bg:#0f1115;--term-fg:#d8dae0;--brain:#7fb0e8;--cli:#6cc295;--nano:#b894dc;}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--ink);
font-family:"IBM Plex Sans",-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;
font-size:16.5px;line-height:1.65;-webkit-font-smoothing:antialiased}
.huelle{display:grid;grid-template-columns:230px minmax(0,1fr);gap:3rem;max-width:1180px;margin:0 auto;padding:2.5rem 1.5rem 6rem}
@media(max-width:900px){.huelle{grid-template-columns:1fr;gap:1.5rem;padding-top:1.5rem}}
nav{position:sticky;top:2rem;align-self:start;font-size:14.5px}
@media(max-width:900px){nav{position:static}}
nav .marke{font-family:"Bricolage Grotesque",sans-serif;font-weight:800;font-size:17px;letter-spacing:-.02em;margin-bottom:.2rem}
nav .unter{color:var(--muted);font-size:13px;margin-bottom:1.3rem}
nav a{display:block;padding:.3rem 0;color:var(--muted);text-decoration:none;border-left:2px solid transparent;padding-left:.7rem;margin-left:-.7rem}
nav a:hover{color:var(--ink)}
nav a.hier{color:var(--akzent);border-left-color:var(--akzent);font-weight:600}
main{min-width:0;max-width:72ch}
h1{font-family:"Bricolage Grotesque",sans-serif;font-weight:800;font-size:2.1rem;line-height:1.15;letter-spacing:-.025em;margin:0 0 .4rem}
h2{font-family:"Bricolage Grotesque",sans-serif;font-weight:700;font-size:1.35rem;letter-spacing:-.015em;margin:2.6rem 0 .7rem;padding-top:1.2rem;border-top:1px solid var(--line)}
h3{font-weight:600;font-size:1.05rem;margin:1.8rem 0 .4rem}
.vorspann{color:var(--muted);font-size:1.07rem;margin:0 0 2rem}
p{margin:0 0 1rem}
code{font-family:"JetBrains Mono",ui-monospace,monospace;font-size:.88em;background:var(--akzent-weich);padding:.1em .35em;border-radius:3px}
pre{background:var(--term-bg);color:var(--term-fg);padding:1rem 1.1rem;border-radius:8px;overflow-x:auto;font-size:13.5px;line-height:1.6}
pre code{background:none;padding:0;color:inherit;font-size:inherit}
pre .d{color:var(--term-dim)}
figure{margin:1.8rem 0;background:var(--surface);border:1px solid var(--line);border-radius:10px;padding:1.2rem;overflow-x:auto}
figure svg{display:block;max-width:100%;height:auto;min-width:520px}
figcaption{margin-top:.9rem;font-size:14px;color:var(--muted);line-height:1.5}
table{border-collapse:collapse;width:100%;font-size:14.5px;margin:1.3rem 0;display:block;overflow-x:auto}
th,td{text-align:left;padding:.55rem .7rem;border-bottom:1px solid var(--line);vertical-align:top}
th{font-weight:600;color:var(--muted);font-size:13px;text-transform:uppercase;letter-spacing:.04em}
.merk{background:var(--akzent-weich);border-left:3px solid var(--akzent);padding:.9rem 1.1rem;border-radius:0 8px 8px 0;margin:1.5rem 0}
.warn{background:var(--warn-weich);border-left:3px solid var(--warn);padding:.9rem 1.1rem;border-radius:0 8px 8px 0;margin:1.5rem 0}
.merk p:last-child,.warn p:last-child{margin-bottom:0}
ul,ol{margin:0 0 1rem;padding-left:1.3rem}
li{margin-bottom:.35rem}
.fuss{margin-top:3.5rem;padding-top:1.2rem;border-top:1px solid var(--line);color:var(--muted);font-size:13.5px}
a{color:var(--akzent)}
'''

SEITEN = [
    ('index',               'Uebersicht'),
    ('was-ist-ein-harness', 'Was ist ein Harness?'),
    ('architektur',         'Wie unserer gebaut ist'),
    ('gedaechtnis',         'Wo das Wissen liegt'),
    ('mehrbenutzer',        'Mehrere Menschen'),
    ('befehle',             'Alle Befehle'),
]

def seite(datei, titel, vorspann, rumpf):
    nav = '\n'.join(
        '<a href="{}.html"{}>{}</a>'.format(d, ' class="hier"' if d == datei else '', t)
        for d, t in SEITEN)
    return '''<!doctype html>
<html lang="de">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>{titel} &middot; ai-brain-cli</title>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Bricolage+Grotesque:opsz,wght@12..96,700;12..96,800&amp;family=IBM+Plex+Sans:wght@400;500;600&amp;family=JetBrains+Mono:wght@400;600&amp;display=swap">
<style>{css}</style>
</head>
<body>
<div class="huelle">
<nav>
<div class="marke">ai-brain-cli</div>
<div class="unter">Der Harness, Stand 15.10.2026</div>
{nav}
</nav>
<main>
<h1>{titel}</h1>
<p class="vorspann">{vorspann}</p>
{rumpf}
<div class="fuss">ai-brain-cli &middot; Doku vom 15.10.2026 &middot; <a href="https://github.com/peppermint-digital/ai-brain-cli">github.com/peppermint-digital/ai-brain-cli</a></div>
</main>
</div>
</body>
</html>
'''.format(titel=titel, css=CSS, nav=nav, vorspann=vorspann, rumpf=rumpf)
