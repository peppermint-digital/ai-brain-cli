//! `brain` — der Einstieg (#502).
//!
//! **Ein Wort, und es geht auf.** Das ist der ganze Zweck. Technisch kann
//! `chat` nichts, was `claude` nicht auch koennte — aber niemand soll wissen
//! muessen, in welchem Verzeichnis er steht, welche Rechte gelten und wie das
//! Projekt heisst. Der Befehl nimmt das ab und zeigt es.
//!
//! **Das Fenster gehoert weiterhin dem Werkzeug.** Wir mieten die Schleife
//! (siehe `agent.rs`), also mieten wir auch die Eingabezeile. Was uns gehoert,
//! ist alles davor: Projekt, Identitaet, Rechte, Register — und der Schirm,
//! der zeigt, was man in der Hand hat.
//!
//! Unterschied zu `agent`: Dort ist die Eingabe zu und die Ausgabe geht in ein
//! Protokoll; hier erbt das Werkzeug das Terminal. Deshalb gibt es fuer ein
//! Gespraech kein Protokoll bei uns — der Verlauf liegt beim Werkzeug.

use std::path::Path;
use std::process::Command;

use crate::agent::{Modell, Rechte};
use crate::config::{self, Connection};
use crate::{freigabe, register};

/// Den Projekt-Slug aus der naechsten CLAUDE.md lesen, die einen nennt.
///
/// **Dieselbe Regel wie das imkr-Plugin** (`plugins/ai-brain/start.sh`):
/// Abschnitt `## AI Brain`, darin ein Slug in Grave-Zeichen. Zwei Programme,
/// die dasselbe Verzeichnis verschieden beantworten, waeren schlimmer als
/// keines von beiden.
pub fn projekt_aus_claude_md(start: &Path) -> Option<String> {
    let mut dir = Some(start);

    while let Some(d) = dir {
        if let Ok(text) = std::fs::read_to_string(d.join("CLAUDE.md")) {
            if let Some(slug) = slug_aus_text(&text) {
                return Some(slug);
            }
        }

        dir = d.parent();
    }

    None
}

/// Der Slug aus dem Abschnitt `## AI Brain`, sonst nichts.
///
/// Bewusst nur dort: Ein Grave-Wort irgendwo sonst in einer CLAUDE.md ist ein
/// Codebeispiel, kein Projektname.
fn slug_aus_text(text: &str) -> Option<String> {
    let mut im_abschnitt = false;

    for zeile in text.lines() {
        if zeile.starts_with("## ") {
            im_abschnitt = zeile.trim_end().eq_ignore_ascii_case("## AI Brain");
            continue;
        }

        if !im_abschnitt {
            continue;
        }

        let klein = zeile.to_ascii_lowercase();

        if !klein.contains("slug") {
            continue;
        }

        if let Some(rest) = zeile.split_once('`') {
            if let Some((wort, _)) = rest.1.split_once('`') {
                let wort = wort.trim();

                if !wort.is_empty() {
                    return Some(wort.to_string());
                }
            }
        }
    }

    None
}

/// Was die Umgebung ueber Anbieter und Modell sagt.
///
/// Die CLI verwaltet das NICHT — imkr tut es ueber Konten
/// (`imkr account add … --url … --model …`), und ein Kindprozess erbt die
/// Variablen. Hier werden sie nur gelesen, um sie anzuzeigen: Wer nicht sieht,
/// mit wem er spricht, kann es auch nicht pruefen.
fn anbieter() -> Option<String> {
    let modell = std::env::var("ANTHROPIC_MODEL").ok().filter(|s| !s.is_empty());
    let adresse = std::env::var("ANTHROPIC_BASE_URL").ok().filter(|s| !s.is_empty());

    match (adresse, modell) {
        (Some(a), Some(m)) => Some(format!("{m} über {}", kurz(&a))),
        (Some(a), None) => Some(kurz(&a)),
        (None, Some(m)) => Some(m),
        (None, None) => None,
    }
}

/// Von einer Adresse bleibt der Rechnername — mehr passt nicht in eine Zeile.
fn kurz(adresse: &str) -> String {
    adresse
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(adresse)
        .to_string()
}

/// Wie viele Eintraege der Abgleich auf die Platte gelegt hat.
fn skills_gezaehlt() -> usize {
    let Ok(heimat) = std::env::var("HOME") else {
        return 0;
    };

    ["skills", "agents"]
        .iter()
        .map(|was| {
            std::fs::read_dir(Path::new(&heimat).join(".claude").join(was))
                .map(|d| d.filter_map(Result::ok).filter(|e| e.path().is_dir()).count())
                .unwrap_or(0)
        })
        .sum()
}

/// Der Name der Instanz, frisch statt aus der Ablage.
///
/// **Warum nicht einfach `connection.instance_name`?** Weil der beim Koppeln
/// festgehalten wurde und seitdem veraltet sein kann: Auf diesem Rechner stand
/// dort am 15.10.2026 noch „Laravel", waehrend die Instanz laengst „AI Brain"
/// heisst. Ein Begruessungsschirm mit falschem Namen ist schlimmer als keiner.
///
/// Der Steckbrief braucht keine Anmeldung und ist eine kurze Anfrage. Scheitert
/// sie — kein Netz, Instanz aus —, gilt der gemerkte Name: Dass der Name alt
/// ist, darf das Gespraech nicht aufhalten.
fn instanzname(connection: &Connection) -> String {
    let url = format!("{}/.well-known/ai-brain", connection.base_url.trim_end_matches('/'));

    ureq::get(&url)
        .timeout(std::time::Duration::from_millis(1500))
        .call()
        .ok()
        .and_then(|a| a.into_json::<serde_json::Value>().ok())
        .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| connection.instance_name.clone())
}

fn schirm(
    connection: &Connection,
    modell: Modell,
    projekt: Option<&str>,
    verzeichnis: &Path,
    rechte: Rechte,
    lauf: &str,
) {
    let breite = 64;

    let name = instanzname(connection);

    println!();
    println!("  \x1b[1m{name}\x1b[0m · {}", connection.base_url);
    println!("  {}", "─".repeat(breite));

    // Bewusst NICHT `connection.device_name`: Der stammt vom Koppeln und kann
    // umbenannt worden sein — auf diesem Rechner stand dort „omarchy (linux
    // x86_64)", waehrend die Instanz ihn „Codingmachine" nennt. Besitzer und
    // Rechnername sind beide lokal ermittelbar und koennen nicht veralten.
    println!(
        "  Angemeldet {} · {}",
        connection.owner,
        gethostname::gethostname().to_string_lossy()
    );

    match projekt {
        Some(p) => println!("  Projekt    {p}  ·  {}", verzeichnis.display()),
        None => println!(
            "  Projekt    —  ·  {}  \x1b[2m(keine CLAUDE.md mit Slug)\x1b[0m",
            verzeichnis.display()
        ),
    }

    let anbieterzeile = anbieter()
        .map(|a| format!("{}  ·  {a}", modell.programm()))
        .unwrap_or_else(|| format!("{}  ·  Standardkonto", modell.programm()));

    println!("  Motor      {anbieterzeile}");
    println!(
        "  Rechte     {}",
        match rechte {
            Rechte::Fragend => "fragt vor jeder Tat",
            Rechte::Schreibend => "darf schreiben",
            Rechte::Vollzugriff => "Vollzugriff",
        }
    );

    let skills = skills_gezaehlt();

    println!(
        "  Dabei      Werkzeuge von {name}  ·  {skills} Eintrag(e) aus dem Repository"
    );
    println!("  Lauf       {lauf}");
    println!("  {}", "─".repeat(breite));
    println!();
}

/// `brain` / `ai-brain-harness chat [...]`
pub fn starten(args: &[String]) -> Result<(), String> {
    let wert = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .filter(|w| !w.starts_with("--"))
            .cloned()
    };

    let hat = |name: &str| args.iter().any(|a| a == name);

    let modell = match wert("--modell").or_else(|| wert("--model")) {
        Some(w) => Modell::aus(&w)?,
        None => Modell::Claude,
    };

    let verzeichnis = match wert("--verzeichnis").or_else(|| wert("--dir")) {
        Some(p) => freigabe::aufloesen(&p)?,
        None => std::env::current_dir().map_err(|e| format!("Arbeitsverzeichnis unlesbar: {e}"))?,
    };

    let rechte = if hat("--vollzugriff") {
        Rechte::Vollzugriff
    } else if hat("--schreibend") {
        Rechte::Schreibend
    } else {
        Rechte::Fragend
    };

    if rechte != Rechte::Fragend && !freigabe::ist_frei(&verzeichnis) {
        return Err(format!(
            "{} ist nicht freigegeben — mehr Rechte gelten nur in freigegebenen Verzeichnissen.\n\
             Freigeben:  ai-brain-harness freigabe add {}",
            verzeichnis.display(),
            verzeichnis.display()
        ));
    }

    let projekt = wert("--projekt")
        .or_else(|| wert("--project"))
        .or_else(|| projekt_aus_claude_md(&verzeichnis));

    let connection = config::load()?;

    // Ein Gespraech in einem imkr-Pane uebernimmt DESSEN Adresse, statt einen
    // neuen Pane zu erzeugen — `brain` wird aus dem Pane heraus gerufen, nicht
    // davor.
    let pane = std::env::var("IMKR_PANE").ok().filter(|p| !p.is_empty());

    let lauf = register::anlegen(register::NeuerLauf {
        modell: modell.programm().to_string(),
        verzeichnis: verzeichnis.display().to_string(),
        projekt: projekt.clone(),
        task: None,
        auftrag: Some("Gespräch".to_string()),
        pid: None,
        pane,
    })?;

    schirm(
        &connection,
        modell,
        projekt.as_deref(),
        &verzeichnis,
        rechte,
        &lauf.id,
    );

    // Ohne Terminal gibt es kein Gespraech. Ohne diese Pruefung faellt `claude`
    // in seinen Druckmodus und meldet „Input must be provided either through
    // stdin or as a prompt argument when using --print" — eine Fehlermeldung,
    // die auf das falsche Problem zeigt.
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        register::abschliessen(&lauf.id, None, Some("kein Terminal".to_string()))?;

        return Err(format!(
            "`chat` braucht ein Terminal — die Eingabe kommt hier aus einer Datei oder Pipe.\n\
             Fuer einen unbeaufsichtigten Lauf:  ai-brain-harness agent --aufgabe \"…\"{}",
            projekt.map(|p| format!(" --projekt {p}")).unwrap_or_default()
        ));
    }

    let mut befehl = Command::new(modell.programm());
    befehl.current_dir(&verzeichnis);
    befehl.args(modell.gespraech_args(rechte, hat("--weiter"))?);

    // Ein- und Ausgabe werden GEERBT. Das ist der ganze Unterschied zu einem
    // Lauf: Dort ist die Eingabe zu und die Ausgabe geht in eine Datei, hier
    // ist das Terminal die Oberflaeche.
    let mut kind = befehl
        .spawn()
        .map_err(|e| format!("{} liess sich nicht starten: {e}", modell.programm()))?;

    register::pid_setzen(&lauf.id, kind.id())?;

    // **Warum hier gewartet wird statt `status()`.** Ein Lauf ohne imkr-Pane
    // hat keinen anderen Aufseher als diesen Prozess. Ohne Lebenszeichen
    // erklaert ihn das Register nach drei Minuten fuer verschwunden — ein
    // Gespraech, an dem jemand gerade tippt, staende in `agents` als Fehler.
    // Gefunden, als imkr `brain` in ein eigenes Fenster haengte.
    let status = loop {
        match kind.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                let _ = register::herzschlag(&lauf.id);
                std::thread::sleep(std::time::Duration::from_secs(30));
            }
            Err(e) => return Err(format!("Der Lauf laesst sich nicht beobachten: {e}")),
        }
    };

    register::abschliessen(&lauf.id, status.code(), None)?;

    println!();
    println!("  Gespräch beendet. Fortsetzen:  ai-brain-harness chat --weiter");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{kurz, slug_aus_text};

    #[test]
    fn slug_kommt_nur_aus_dem_ai_brain_abschnitt() {
        let text = "# Projekt\n\n## Sonstiges\n\nProjekt-Slug: `falsch`\n\n\
                    ## AI Brain\n\nProjekt-Slug: `richtig`\n";

        assert_eq!(slug_aus_text(text).as_deref(), Some("richtig"));
    }

    #[test]
    fn ein_abschnitt_endet_an_der_naechsten_ueberschrift() {
        // Ohne das Beenden liefe der Abschnitt bis zum Dateiende und ein
        // Codebeispiel weiter unten wuerde zum Projektnamen.
        let text = "## AI Brain\n\nkein Slug hier\n\n## Lint\n\nProjekt-Slug: `falsch`\n";

        assert_eq!(slug_aus_text(text), None);
    }

    #[test]
    fn ohne_abschnitt_kein_slug() {
        assert_eq!(slug_aus_text("# Nur ein Titel\n\n`irgendwas`\n"), None);
    }

    #[test]
    fn adresse_wird_auf_den_rechnernamen_gekuerzt() {
        assert_eq!(kurz("https://openrouter.ai/api/v1"), "openrouter.ai");
        assert_eq!(kurz("http://127.0.0.1:11434"), "127.0.0.1:11434");
        assert_eq!(kurz("ollama.local"), "ollama.local");
    }
}
