//! Die Eingabemaske des Harness (#7805).
//!
//! **Was hier anders ist als in `chat.rs`.** `chat` gibt das Terminal an das
//! Werkzeug ab — danach gehoert die Eingabezeile `claude`, und alles, was wir
//! zeigen, steht davor. Die Maske behaelt das Terminal: sie fragt, sie stellt
//! dar, sie fuehrt Buch. Der Motor wird pro Zug gerufen und gibt das Terminal
//! nie zu sehen.
//!
//! **Warum das trotzdem einen fremden Motor benutzt.** Eine eigene
//! Agentenschleife braucht einen eigenen Modellzugang, und der ist eine
//! Kostenentscheidung, die nicht beim Bauen fallen darf. Die Trennung sitzt
//! deshalb in `motor_args()`: Oberflaeche, Werkzeuge, Verlauf und Buchfuehrung
//! gehoeren uns, das Rechnen ist eingekauft. Wird spaeter ein Schluessel
//! freigegeben, tauscht man diese eine Funktion.
//!
//! ## Zwei Werkzeugserver in derselben Hand
//!
//! Die Maske haengt beide an:
//!
//! - `ai-brain` — der Durchgriff auf die Instanz (Aufgaben, Gedaechtnis,
//!   Server, Rechte-Fenster). Das ist `ai-brain-cli mcp`.
//! - `rechner` — dieser Rechner (Dienste, Platten, Bildschirm, Tastatur).
//!   Das ist `ai-brain-cli rechner-mcp`, siehe `rechner.rs`.
//!
//! Damit hat ein Gespraech beides zugleich, und genau das war der Zweck:
//! Hermes konnte den Rechner, aber nicht unsere Daten; die Werkzeuge von AI
//! Brain konnten unsere Daten, aber nicht den Rechner.
//!
//! ## Der Verlauf liegt beim Motor, die Spur bei uns
//!
//! Jeder Zug laeuft als eigener Prozess. Zusammengehalten wird das Gespraech
//! von der Sitzungskennung, die der Motor im ersten Zug nennt und die jeder
//! weitere per `--resume` wieder aufgreift. Faellt die Maske aus, ist das
//! Gespraech nicht verloren — die Kennung steht im Register.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::agent::Rechte;
use crate::config::{self, Connection};
use crate::{aufgaben, auftrag, chat, erinnerungen, freigabe, register};

/// Was die Maske aus einem Zug des Motors herausliest.
struct Zug {
    sitzung: Option<String>,
    kosten: Option<f64>,
    abgebrochen: bool,
}

/// `brain` — die Maske.
pub fn starten(args: &[String]) -> Result<(), String> {
    let hat = |name: &str| args.iter().any(|a| a == name);

    let wert = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .filter(|w| !w.starts_with("--"))
            .cloned()
    };

    let verzeichnis = match wert("--verzeichnis").or_else(|| wert("--dir")) {
        Some(p) => freigabe::aufloesen(&p)?,
        None => std::env::current_dir().map_err(|e| format!("Arbeitsverzeichnis unlesbar: {e}"))?,
    };

    let mut rechte = if hat("--steuernd") || hat("--vollzugriff") {
        Rechte::Vollzugriff
    } else {
        Rechte::Fragend
    };

    if rechte != Rechte::Fragend && !freigabe::ist_frei(&verzeichnis) {
        return Err(format!(
            "{} ist nicht freigegeben — steuernde Rechte gelten nur in freigegebenen Verzeichnissen.\n\
             Freigeben:  ai-brain-cli freigabe add {}",
            verzeichnis.display(),
            verzeichnis.display()
        ));
    }

    let projekt = wert("--projekt")
        .or_else(|| wert("--project"))
        .or_else(|| chat::projekt_aus_claude_md(&verzeichnis));

    let connection = config::load()?;

    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        return Err(
            "Die Maske braucht ein Terminal — die Eingabe kommt hier aus einer Datei oder Pipe.\n\
             Fuer einen unbeaufsichtigten Lauf:  ai-brain-cli agent --aufgabe \"…\""
                .to_string(),
        );
    }

    let lauf = register::anlegen(register::NeuerLauf {
        modell: "harness".to_string(),
        verzeichnis: verzeichnis.display().to_string(),
        projekt: projekt.clone(),
        task: None,
        auftrag: Some("Maske".to_string()),
        pid: Some(std::process::id()),
        pane: std::env::var("IMKR_PANE").ok().filter(|p| !p.is_empty()),
    })?;

    schirm(&connection, projekt.as_deref(), &verzeichnis, rechte, &lauf.id);

    // Das Gedaechtnis gleicht sich beim Aufgehen ab, nicht auf Zuruf. Ein
    // Abgleich, den man anstossen muss, laeuft irgendwann nicht mehr — und
    // dann merkt der Harness sich Dinge, die woanders schon anders stehen.
    abgleich_beim_start(projekt.as_deref());

    let mut sitzung: Option<String> = None;
    let stdin = std::io::stdin();

    loop {
        print!("\n\x1b[1;36m▸\x1b[0m ");
        let _ = std::io::stdout().flush();

        let mut eingabe = String::new();

        match stdin.lock().read_line(&mut eingabe) {
            // Ctrl-D: dasselbe wie `/ende`.
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                eprintln!("  Eingabe nicht lesbar: {e}");
                break;
            }
        }

        let text = eingabe.trim();

        if text.is_empty() {
            continue;
        }

        if text.starts_with('/') {
            match befehl(text, &mut rechte, &mut sitzung, &verzeichnis, projekt.as_deref()) {
                Weiter::Ja => continue,
                Weiter::Schluss => break,
            }
        }

        let _ = register::herzschlag(&lauf.id);

        match zug_fahren(text, &verzeichnis, rechte, sitzung.as_deref()) {
            Ok(zug) => {
                if let Some(neu) = zug.sitzung {
                    sitzung = Some(neu);
                }

                if zug.abgebrochen {
                    println!("\n  \x1b[2mZug abgebrochen.\x1b[0m");
                } else if let Some(kosten) = zug.kosten {
                    println!("\n  \x1b[2m{kosten:.4} $\x1b[0m");
                }
            }
            Err(e) => eprintln!("\n  \x1b[31m{e}\x1b[0m"),
        }
    }

    register::abschliessen(&lauf.id, Some(0), None)?;

    println!("\n  Bis dann.");

    Ok(())
}

enum Weiter {
    Ja,
    Schluss,
}

/// Die Befehle der Maske — alles, was ohne Modell geht, geht ohne Modell.
///
/// **Warum ueberhaupt eigene Befehle.** Ein Blick auf die offenen Aufgaben oder
/// die fehlgeschlagenen Dienste ist eine Nachfrage, keine Denkaufgabe. Wer sie
/// durch ein Modell schickt, zahlt dafuer und wartet darauf.
fn befehl(
    zeile: &str,
    rechte: &mut Rechte,
    sitzung: &mut Option<String>,
    verzeichnis: &Path,
    projekt: Option<&str>,
) -> Weiter {
    let mut teile = zeile.splitn(2, char::is_whitespace);
    let name = teile.next().unwrap_or("");
    let rest = teile.next().unwrap_or("").trim();

    let projekt_args = || -> Vec<String> {
        projekt
            .map(|p| vec!["--projekt".to_string(), p.to_string()])
            .unwrap_or_default()
    };

    match name {
        "/ende" | "/quit" | "/exit" => return Weiter::Schluss,

        "/hilfe" | "/help" | "/?" => hilfe(),

        "/neu" => {
            *sitzung = None;
            println!("  Neues Gespräch — der Verlauf des letzten bleibt beim Motor liegen.");
        }

        "/aufgaben" => melden(aufgaben::anzeigen(&projekt_args())),

        "/auftraege" => melden(auftrag::anzeigen(&[])),

        "/daueraufgaben" => match crate::dauerauftrag::fuer_maske() {
            Ok(t) => println!("{t}"),
            Err(e) => eprintln!("  {e}"),
        },

        "/erinnerungen" => melden(erinnerungen::abgleichen(&[])),

        "/dienste" => {
            // Direkt in die Werkzeugschicht, ohne Motor und ohne MCP.
            let args = if rest.is_empty() {
                serde_json::json!({})
            } else {
                serde_json::json!({ "dienste": rest.split_whitespace().collect::<Vec<_>>() })
            };

            match crate::rechner::fuer_maske("rechner-dienste", &args) {
                Ok(t) => println!("{t}"),
                Err(e) => eprintln!("  {e}"),
            }
        }

        "/rechner" => {
            for werkzeug in ["rechner-last", "rechner-datentraeger"] {
                match crate::rechner::fuer_maske(werkzeug, &serde_json::json!({})) {
                    Ok(t) => println!("{t}\n"),
                    Err(e) => eprintln!("  {e}"),
                }
            }
        }

        "/rechte" => match rest {
            "" => println!("  {}", rechte_lesbar(*rechte)),
            "steuernd" => {
                if freigabe::ist_frei(verzeichnis) {
                    *rechte = Rechte::Vollzugriff;
                    println!("  Rechte: {}", rechte_lesbar(*rechte));
                } else {
                    eprintln!(
                        "  {} ist nicht freigegeben.\n  Freigeben:  ai-brain-cli freigabe add {}",
                        verzeichnis.display(),
                        verzeichnis.display()
                    );
                }
            }
            "lesend" => {
                *rechte = Rechte::Fragend;
                println!("  Rechte: {}", rechte_lesbar(*rechte));
            }
            sonst => eprintln!("  `/rechte {sonst}` kenne ich nicht. Erlaubt: lesend, steuernd."),
        },

        sonst => eprintln!("  `{sonst}` kenne ich nicht. `/hilfe` zeigt, was es gibt."),
    }

    Weiter::Ja
}

fn melden(ergebnis: Result<(), String>) {
    if let Err(e) = ergebnis {
        eprintln!("  {e}");
    }
}

fn hilfe() {
    println!(
        "
  Schreib einfach los — alles ohne `/` geht an den Motor.

  \x1b[1mOhne Modell\x1b[0m
    /aufgaben          offene Aufgaben des Projekts
    /auftraege         Aufträge, die für diesen Rechner anliegen
    /daueraufgaben     eingerichtete Daueraufträge dieses Rechners
    /erinnerungen      Gedächtnis mit AI Brain abgleichen
    /dienste [namen]   Zustand von Diensten (ohne Namen: alle fehlgeschlagenen)
    /rechner           Last, Speicher, Platten, Grafikkarte

  \x1b[1mGespräch\x1b[0m
    /neu               Verlauf fallen lassen und neu anfangen
    /rechte [stufe]    lesend | steuernd
    /ende              Schluss (oder Ctrl-D)"
    );
}

fn rechte_lesbar(rechte: Rechte) -> &'static str {
    match rechte {
        Rechte::Fragend => "lesend — nachsehen ja, anfassen nein",
        Rechte::Schreibend => "schreibend",
        Rechte::Vollzugriff => "steuernd — Dienste, Befehle, Tastatur",
    }
}

/// Einen Zug fahren: Frage hin, Antwort laufend heraus.
fn zug_fahren(
    text: &str,
    verzeichnis: &Path,
    rechte: Rechte,
    sitzung: Option<&str>,
) -> Result<Zug, String> {
    let eigen = eigener_pfad()?;

    let mut befehl = Command::new("claude");
    befehl.current_dir(verzeichnis);
    befehl.args(motor_args(&eigen, rechte, sitzung));

    // Die Rechte gehen ueber die Umgebung an den Werkzeugserver — ein Modell
    // kann sie dort nicht anfassen. Siehe `rechner::Hoheit`.
    befehl.env(
        "AI_BRAIN_RECHNER_RECHTE",
        if rechte == Rechte::Fragend { "lesend" } else { "steuernd" },
    );

    // **Die Frage geht ueber stdin, nicht als Argument.** `--allowedTools` und
    // `--mcp-config` nehmen mehrere Werte; ein nachgestelltes Argument wird von
    // ihnen aufgesammelt, und der Motor meldet dann „Input must be provided
    // either through stdin or as a prompt argument" — eine Meldung, die auf
    // eine fehlende Eingabe zeigt, obwohl die Eingabe nur im falschen Schalter
    // gelandet ist. Gemessen am 15.10.2026.
    befehl.stdin(Stdio::piped());
    befehl.stdout(Stdio::piped());
    befehl.stderr(Stdio::piped());

    let mut kind = befehl
        .spawn()
        .map_err(|e| format!("Der Motor liess sich nicht starten: {e}"))?;

    // Die Pipe wird geschlossen, sobald sie aus dem Gueltigkeitsbereich faellt
    // — ohne das wartet der Motor auf weitere Eingabe und der Zug endet nie.
    {
        let mut hinein = kind
            .stdin
            .take()
            .ok_or_else(|| "Der Motor nimmt keine Eingabe an.".to_string())?;

        hinein
            .write_all(text.as_bytes())
            .map_err(|e| format!("Die Frage liess sich nicht uebergeben: {e}"))?;
    }

    let Some(aus) = kind.stdout.take() else {
        return Err("Der Motor liefert keine Ausgabe.".to_string());
    };

    let mut zug = Zug {
        sitzung: None,
        kosten: None,
        abgebrochen: false,
    };

    let mut im_text = false;

    for zeile in BufReader::new(aus).lines() {
        let Ok(zeile) = zeile else { break };

        let Ok(e) = serde_json::from_str::<serde_json::Value>(&zeile) else {
            continue;
        };

        darstellen(&e, &mut zug, &mut im_text);
    }

    let status = kind
        .wait()
        .map_err(|e| format!("Der Zug laesst sich nicht beobachten: {e}"))?;

    if im_text {
        println!();
    }

    if !status.success() && zug.sitzung.is_none() && zug.kosten.is_none() {
        let mut meldung = String::new();

        if let Some(mut err) = kind.stderr.take() {
            use std::io::Read;
            let _ = err.read_to_string(&mut meldung);
        }

        return Err(format!(
            "Der Motor endete mit {}{}",
            status.code().map(|c| c.to_string()).unwrap_or_else(|| "Signal".into()),
            if meldung.trim().is_empty() {
                String::new()
            } else {
                format!(":\n{}", meldung.trim())
            }
        ));
    }

    Ok(zug)
}

/// Ein Ereignis des Motors in Bildschirmausgabe verwandeln.
///
/// Die Formen stammen aus einer Messung an `claude -p --output-format
/// stream-json --include-partial-messages`, nicht aus der Dokumentation.
fn darstellen(e: &serde_json::Value, zug: &mut Zug, im_text: &mut bool) {
    let art = e.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match art {
        "system" => {
            if e.get("subtype").and_then(|s| s.as_str()) == Some("init") {
                if let Some(id) = e.get("session_id").and_then(|s| s.as_str()) {
                    zug.sitzung = Some(id.to_string());
                }
            }
        }

        "stream_event" => {
            let ereignis = e.get("event").unwrap_or(&serde_json::Value::Null);

            if ereignis.get("type").and_then(|t| t.as_str()) == Some("content_block_delta") {
                if let Some(stueck) = ereignis
                    .get("delta")
                    .and_then(|d| d.get("text"))
                    .and_then(|t| t.as_str())
                {
                    if !*im_text {
                        println!();
                        *im_text = true;
                    }

                    print!("{stueck}");
                    let _ = std::io::stdout().flush();
                }
            }
        }

        // Werkzeuggriffe stehen in der fertigen Nachricht, nicht in den
        // Teilstuecken. Sie gehoeren sichtbar gemacht: wer den Rechner
        // anfassen laesst, soll sehen, womit.
        "assistant" => {
            let leer = Vec::new();
            let bloecke = e
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
                .unwrap_or(&leer);

            for block in bloecke {
                if block.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
                    continue;
                }

                let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("?");

                if *im_text {
                    println!();
                    *im_text = false;
                }

                println!("  \x1b[2m⚒ {}\x1b[0m", name.trim_start_matches("mcp__"));
            }
        }

        "result" => {
            if let Some(id) = e.get("session_id").and_then(|s| s.as_str()) {
                zug.sitzung = Some(id.to_string());
            }

            zug.kosten = e.get("total_cost_usd").and_then(|k| k.as_f64());

            if e.get("subtype").and_then(|s| s.as_str()) != Some("success") {
                zug.abgebrochen = true;
            }
        }

        _ => {}
    }
}

/// Die Schalter fuer einen Zug.
///
/// **`--strict-mcp-config` ist kein Beiwerk.** Ohne ihn erbt der Motor die
/// Werkzeugserver aus der Nutzerkonfiguration — dann haengen an einem Zug der
/// Maske fremde Server, die niemand hier angehaengt hat.
fn motor_args(eigen: &Path, rechte: Rechte, sitzung: Option<&str>) -> Vec<String> {
    let eigen = eigen.display().to_string();

    let server = serde_json::json!({
        "mcpServers": {
            "ai-brain": { "command": eigen, "args": ["mcp"] },
            "rechner": { "command": eigen, "args": ["rechner-mcp"] },
        }
    });

    let mut args: Vec<String> = vec![
        "-p".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--include-partial-messages".into(),
        "--verbose".into(),
        "--strict-mcp-config".into(),
        "--mcp-config".into(),
        server.to_string(),
    ];

    match rechte {
        // Lesend: nur Werkzeuge, die nachsehen. Die aendernden des
        // Rechner-Servers sind ohnehin nicht in dessen Liste (siehe
        // `rechner::liste`) — die Aufzaehlung hier riegelt zusaetzlich die
        // Werkzeuge des Motors selbst ab.
        Rechte::Fragend => {
            args.push("--permission-mode".into());
            args.push("dontAsk".into());
            args.push("--allowedTools".into());
            args.push("mcp__ai-brain mcp__rechner Read Grep Glob WebFetch".into());
        }
        Rechte::Schreibend | Rechte::Vollzugriff => {
            args.push("--permission-mode".into());
            args.push("bypassPermissions".into());
        }
    }

    if let Some(id) = sitzung {
        args.push("--resume".into());
        args.push(id.to_string());
    }

    args
}

/// Der eigene Pfad — die Werkzeugserver sind dieses Programm.
fn eigener_pfad() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("Eigener Pfad nicht ermittelbar: {e}"))
}

/// Beim Aufgehen abgleichen, und zwar still.
///
/// Scheitert der Abgleich, ist das kein Grund, die Maske nicht aufzumachen —
/// aber es gehoert gesagt. Ein stiller Fehlschlag waere genau der Fehler, der
/// Hermes' Waechter wirkungslos gemacht hat.
fn abgleich_beim_start(projekt: Option<&str>) {
    let mut args: Vec<String> = Vec::new();

    if let Some(p) = projekt {
        args.push("--projekt".into());
        args.push(p.into());
    }

    args.push("--still".into());

    if let Err(e) = erinnerungen::abgleichen(&args) {
        eprintln!("  \x1b[33mGedächtnis nicht abgeglichen:\x1b[0m {e}");
    }
}

fn schirm(
    connection: &Connection,
    projekt: Option<&str>,
    verzeichnis: &Path,
    rechte: Rechte,
    lauf: &str,
) {
    let breite = 64;

    println!();
    println!("  \x1b[1mAI Brain Harness\x1b[0m · {}", connection.base_url);
    println!("  {}", "─".repeat(breite));
    println!(
        "  Angemeldet {} · {}",
        connection.owner,
        gethostname::gethostname().to_string_lossy()
    );

    match projekt {
        Some(p) => println!("  Projekt    {p}  ·  {}", verzeichnis.display()),
        None => println!("  Projekt    —  ·  {}", verzeichnis.display()),
    }

    println!("  Werkzeuge  AI Brain  ·  dieser Rechner");
    println!("  Rechte     {}", rechte_lesbar(rechte));
    println!("  Lauf       {lauf}");
    println!("  {}", "─".repeat(breite));
    println!("  \x1b[2m/hilfe zeigt, was ohne Modell geht. Ctrl-D beendet.\x1b[0m");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lesende_rechte_zaehlen_die_erlaubten_werkzeuge_auf() {
        let args = motor_args(Path::new("/usr/bin/ai-brain-cli"), Rechte::Fragend, None);

        assert!(args.iter().any(|a| a == "--allowedTools"));
        assert!(args.iter().any(|a| a.contains("mcp__rechner")));
        // Scharf: lesend darf nie den Riegel des Motors ausschalten.
        assert!(!args.iter().any(|a| a == "bypassPermissions"));
    }

    #[test]
    fn steuernde_rechte_zaehlen_nichts_auf() {
        let args = motor_args(Path::new("/usr/bin/ai-brain-cli"), Rechte::Vollzugriff, None);

        assert!(args.iter().any(|a| a == "bypassPermissions"));
        assert!(!args.iter().any(|a| a == "--allowedTools"));
    }

    #[test]
    fn beide_werkzeugserver_haengen_am_zug_und_zwar_nur_sie() {
        let args = motor_args(Path::new("/opt/harness"), Rechte::Fragend, None);
        let i = args.iter().position(|a| a == "--mcp-config").unwrap();
        let config: serde_json::Value = serde_json::from_str(&args[i + 1]).unwrap();

        assert_eq!(config["mcpServers"]["ai-brain"]["command"], "/opt/harness");
        assert_eq!(config["mcpServers"]["rechner"]["args"][0], "rechner-mcp");
        assert_eq!(config["mcpServers"].as_object().unwrap().len(), 2);

        // Ohne diesen Schalter kaemen fremde Server aus der Nutzerkonfiguration
        // mit — die Maske haette dann Werkzeuge, die sie nicht angehaengt hat.
        assert!(args.iter().any(|a| a == "--strict-mcp-config"));
    }

    #[test]
    fn die_frage_steht_in_keinem_argument() {
        // Scharf gegen den Rueckfall: `--allowedTools` und `--mcp-config` sind
        // mehrwertig und schlucken ein nachgestelltes Argument. Die Schalter
        // duerfen deshalb nichts enthalten, was wie eine Frage aussieht.
        for rechte in [Rechte::Fragend, Rechte::Vollzugriff] {
            let args = motor_args(Path::new("/x"), rechte, None);

            assert!(
                args.last().map(|a| a.starts_with("--") || a.starts_with("mcp__")
                    || a == "stream-json" || a == "dontAsk" || a == "bypassPermissions"
                    || a.starts_with('{'))
                    .unwrap_or(false),
                "letztes Argument sieht wie eine Frage aus: {:?}",
                args.last()
            );
        }
    }

    #[test]
    fn eine_sitzung_wird_fortgesetzt_statt_neu_begonnen() {
        let ohne = motor_args(Path::new("/x"), Rechte::Fragend, None);
        let mit = motor_args(Path::new("/x"), Rechte::Fragend, Some("abc-123"));

        assert!(!ohne.iter().any(|a| a == "--resume"));

        let i = mit.iter().position(|a| a == "--resume").unwrap();
        assert_eq!(mit[i + 1], "abc-123");
    }

    #[test]
    fn die_sitzungskennung_wird_aus_dem_anfang_gelesen() {
        let mut zug = Zug {
            sitzung: None,
            kosten: None,
            abgebrochen: false,
        };
        let mut im_text = false;

        darstellen(
            &serde_json::json!({ "type": "system", "subtype": "init", "session_id": "s-1" }),
            &mut zug,
            &mut im_text,
        );

        assert_eq!(zug.sitzung.as_deref(), Some("s-1"));
    }

    #[test]
    fn ein_abgebrochener_zug_wird_nicht_als_erfolg_gezaehlt() {
        let mut zug = Zug {
            sitzung: None,
            kosten: None,
            abgebrochen: false,
        };
        let mut im_text = false;

        darstellen(
            &serde_json::json!({
                "type": "result",
                "subtype": "error_max_turns",
                "session_id": "s-2",
                "total_cost_usd": 0.5,
            }),
            &mut zug,
            &mut im_text,
        );

        assert!(zug.abgebrochen);
        assert_eq!(zug.sitzung.as_deref(), Some("s-2"));
    }

    #[test]
    fn ein_erfolgreicher_zug_gilt_als_erfolg() {
        let mut zug = Zug {
            sitzung: None,
            kosten: None,
            abgebrochen: false,
        };
        let mut im_text = false;

        darstellen(
            &serde_json::json!({
                "type": "result",
                "subtype": "success",
                "session_id": "s-3",
                "total_cost_usd": 0.25,
            }),
            &mut zug,
            &mut im_text,
        );

        assert!(!zug.abgebrochen);
        assert_eq!(zug.kosten, Some(0.25));
    }
}
