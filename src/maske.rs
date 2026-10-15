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
//!   Server, Rechte-Fenster). Das ist `ai-brain-harness mcp`.
//! - `rechner` — dieser Rechner (Dienste, Platten, Bildschirm, Tastatur).
//!   Das ist `ai-brain-harness rechner-mcp`, siehe `rechner.rs`.
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

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::agent::Rechte;
use crate::anzeige::{self, Anzeige};
use crate::config::{self, Connection};
use crate::{aufgaben, auftrag, chat, erinnerungen, freigabe, konto, register};

/// Was die Maske aus einem Zug des Motors herausliest.
struct Zug {
    sitzung: Option<String>,
    kosten: Option<f64>,
    abgebrochen: bool,
    /// Der Name des Werkzeugs, dessen Argumente gerade stroemen.
    offenes_werkzeug: Option<String>,
    /// Die Argumente, Stueck fuer Stueck, wie sie ankommen.
    offene_argumente: String,
    /// Wie viele Werkzeuge in diesem Zug gegriffen wurden.
    griffe: usize,
    /// Der letzte Griff, wie er gedruckt wurde — damit sich pruefen laesst,
    /// was auf dem Bildschirm stand, und nicht nur, dass gedruckt wurde.
    letzter_griff: Option<(String, String)>,
}

impl Zug {
    fn neu() -> Self {
        Self {
            sitzung: None,
            kosten: None,
            abgebrochen: false,
            offenes_werkzeug: None,
            offene_argumente: String::new(),
            griffe: 0,
            letzter_griff: None,
        }
    }
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
             Freigeben:  ai-brain-harness freigabe add {}",
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
             Fuer einen unbeaufsichtigten Lauf:  ai-brain-harness agent --aufgabe \"…\""
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

    // Die Umgebung des gewaehlten Kontos. Leer heisst: das, was die Sitzung
    // ohnehin mitbringt. Steht VOR dem Schirm, weil der sie anzeigt.
    let mut umgebung: BTreeMap<String, String> = BTreeMap::new();

    // Die Rueckfallkette: imkr → dieser Rechner → AI Brain. AI Brain wird nur
    // gefragt, wenn die ersten zwei nichts hergeben — ein Token zu holen
    // bewegt eine Vollmacht ueber das Netz und hinterlaesst einen Eintrag im
    // Pruefprotokoll. Wer angemeldet ist, braucht das nicht.
    let (quelle, kettenhinweis) = konto::kette(&mut umgebung);

    schirm(
        &connection,
        projekt.as_deref(),
        &verzeichnis,
        rechte,
        &lauf.id,
        Kontolage {
            umgebung: &umgebung,
            quelle,
            label: if quelle == konto::Quelle::AiBrain {
                kettenhinweis.as_deref()
            } else {
                None
            },
        },
    );

    // Ein geholtes Token gehoert gesagt: es verbraucht das Kontingent eines
    // gemeinsamen Kontos, nicht das eigene.
    if quelle == konto::Quelle::AiBrain {
        if let Some(label) = &kettenhinweis {
            println!(
                "  \x1b[2mKein lokales Konto gefunden — Token von AI Brain geholt ({label}).\x1b[0m"
            );
            println!(
                "  \x1b[2mDauerhaft eigenes Konto:  claude login\x1b[0m"
            );
        }
    }

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
            match befehl(
                text,
                &mut rechte,
                &mut sitzung,
                &mut umgebung,
                &verzeichnis,
                projekt.as_deref(),
            ) {
                Weiter::Ja => continue,
                Weiter::Schluss => break,
            }
        }

        // **Vor dem Zug, nicht danach.** Ohne Anmeldung reicht der Motor eine
        // eigene Meldung durch, die nach einem Programmfehler klingt. Der
        // Mensch davor braucht einen Satz, der sagt, was zu tun ist.
        if !konto::angemeldet(&umgebung) {
            eprintln!();
            for zeile in konto::anmeldehinweis(&umgebung).lines() {
                eprintln!("  {zeile}");
            }

            continue;
        }

        let _ = register::herzschlag(&lauf.id);

        match zug_fahren(text, &verzeichnis, rechte, sitzung.as_deref(), &umgebung) {
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
    umgebung: &mut BTreeMap<String, String>,
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

        "/konto" => match rest {
            "" => {
                println!("  Jetzt:  {}", konto::aktuelles(umgebung));
                println!("  Modell: {}", konto::modell(umgebung));

                if !konto::angemeldet(umgebung) {
                    println!();
                    for zeile in konto::anmeldehinweis(umgebung).lines() {
                        println!("  {zeile}");
                    }
                }

                // **Zwei Quellen, beide gekennzeichnet.** Wer imkr benutzt,
                // hat seine Konten dort; wer nicht, hat sie in AI Brain.
                // Beides kommt vor, und eine Maske, die nur eine Quelle
                // kennt, laesst auf dem jeweils anderen Rechner nichts
                // anbieten.
                let mut etwas_gefunden = false;

                match konto::liste() {
                    Some(liste) => {
                        etwas_gefunden = true;
                        println!();
                        println!("  \x1b[2mAuf diesem Rechner (imkr)\x1b[0m");
                        for z in liste.lines() {
                            println!("  {z}");
                        }
                    }
                    None => {
                        println!();
                        println!("  \x1b[2mAuf diesem Rechner: kein imkr\x1b[0m");
                    }
                }

                match konto::aus_ai_brain() {
                    Ok(konten) if !konten.is_empty() => {
                        etwas_gefunden = true;
                        println!();
                        println!("  \x1b[2mIn AI Brain\x1b[0m");
                        for k in &konten {
                            println!("{}", k.zeile());
                        }
                    }
                    Ok(_) => {
                        println!();
                        println!("  \x1b[2mIn AI Brain: kein Modellkonto angelegt\x1b[0m");
                    }
                    // Ein Fehler hier darf die imkr-Liste nicht verschlucken.
                    Err(e) => {
                        println!();
                        println!("  \x1b[2mIn AI Brain: nicht abfragbar — {e}\x1b[0m");
                    }
                }

                println!();

                if etwas_gefunden {
                    println!("  Wechseln:  /konto <name>   \x1b[2m(imkr-Konten, z. B. claude:department)\x1b[0m");
                } else {
                    println!("  Kein Konto zur Auswahl. Der Lauf nutzt, was der Rechner mitbringt.");
                }
            }
            name => match konto::wechseln(name) {
                Ok(neu) => {
                    umgebung.extend(neu);
                    // Das Gespraech gehoert dem Konto, mit dem es begann.
                    // Ein Wechsel beginnt deshalb ein neues.
                    *sitzung = None;
                    println!("  Konto: {}  \x1b[2m(neues Gespräch)\x1b[0m", konto::aktuelles(umgebung));
                }
                Err(e) => eprintln!("  {e}"),
            },
        },

        "/rechte" => match rest {
            "" => println!("  {}", rechte_lesbar(*rechte)),
            "steuernd" => {
                if freigabe::ist_frei(verzeichnis) {
                    *rechte = Rechte::Vollzugriff;
                    println!("  Rechte: {}", rechte_lesbar(*rechte));
                } else {
                    eprintln!(
                        "  {} ist nicht freigegeben.\n  Freigeben:  ai-brain-harness freigabe add {}",
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
    /konto [name]      welches Konto arbeitet — ohne Name: Liste
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
    umgebung: &BTreeMap<String, String>,
) -> Result<Zug, String> {
    let eigen = eigener_pfad()?;

    let mut befehl = Command::new("claude");
    befehl.current_dir(verzeichnis);
    befehl.args(motor_args(&eigen, rechte, sitzung));

    // Das gewaehlte Konto. Wirkt ab DIESEM Zug — ein Wechsel mitten im
    // Gespraech gilt nicht rueckwirkend, der Verlauf gehoert dem Konto, mit
    // dem er begonnen hat.
    for (k, v) in umgebung {
        befehl.env(k, v);
    }

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

    let mut zug = Zug::neu();

    // Der malende Faden. Er ist der einzige Weg zu einer Zahl, die sich
    // bewegt, waehrend ein Werkzeug arbeitet und vom Motor nichts kommt.
    let anzeige = Anzeige::neu();
    let maler = Anzeige::takten(&anzeige);

    anzeige.taetigkeit(Some("denkt".to_string()));

    for zeile in BufReader::new(aus).lines() {
        let Ok(zeile) = zeile else { break };

        let Ok(e) = serde_json::from_str::<serde_json::Value>(&zeile) else {
            continue;
        };

        darstellen(&e, &mut zug, &anzeige);
    }

    let status = kind
        .wait()
        .map_err(|e| format!("Der Zug laesst sich nicht beobachten: {e}"))?;

    anzeige.beenden();
    let _ = maler.join();
    anzeige.absatz();

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
///
/// **Der Werkzeugname kommt aus dem BLOCKANFANG, nicht aus der fertigen
/// Nachricht.** Das ist der Unterschied zwischen „ich sehe, was er tut" und
/// „ich sehe hinterher, was er getan hat": `content_block_start` nennt das
/// Werkzeug, bevor es laeuft, und `input_json_delta` liefert die Argumente
/// dazu. Die fertige Nachricht kommt erst, wenn der Griff schon vorbei ist.
fn darstellen(e: &serde_json::Value, zug: &mut Zug, anzeige: &Anzeige) {
    let art = e.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match art {
        "system" => match e.get("subtype").and_then(|s| s.as_str()) {
            Some("init") => {
                if let Some(id) = e.get("session_id").and_then(|s| s.as_str()) {
                    zug.sitzung = Some(id.to_string());
                }
            }
            // Der Motor denkt. Ohne diese Zeile sieht die Wartezeit vor dem
            // ersten Zeichen aus wie ein Haenger.
            Some("status") => {
                if e.get("status").and_then(|s| s.as_str()) == Some("requesting") {
                    anzeige.taetigkeit(Some("denkt".to_string()));
                }
            }
            // Ein abgelehnter Werkzeuggriff gehoert gesagt, nicht verschluckt:
            // sonst steht am Ende „ich konnte es nicht" ohne Grund.
            Some("permission_denied") => {
                let werkzeug = e
                    .get("tool_name")
                    .and_then(|t| t.as_str())
                    .unwrap_or("ein Werkzeug");

                anzeige.zeile(&format!(
                    "  \x1b[33m⊘ {} abgelehnt\x1b[0m \x1b[2m(die Rechte dieses Laufs)\x1b[0m",
                    anzeige::werkzeugname(werkzeug)
                ));
            }
            _ => {}
        },

        "stream_event" => {
            let ereignis = e.get("event").unwrap_or(&serde_json::Value::Null);
            let typ = ereignis.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match typ {
                "content_block_start" => {
                    let block = ereignis.get("content_block").unwrap_or(&serde_json::Value::Null);

                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                        let name = anzeige::werkzeugname(
                            block.get("name").and_then(|n| n.as_str()).unwrap_or("?"),
                        );

                        zug.offenes_werkzeug = Some(name.clone());
                        zug.offene_argumente.clear();
                        anzeige.taetigkeit(Some(format!("{name} …")));
                    }
                }

                "content_block_delta" => {
                    let delta = ereignis.get("delta").unwrap_or(&serde_json::Value::Null);

                    match delta.get("type").and_then(|t| t.as_str()) {
                        Some("text_delta") => {
                            if let Some(stueck) = delta.get("text").and_then(|t| t.as_str()) {
                                anzeige.taetigkeit(None);
                                anzeige.text(stueck);
                            }
                        }
                        // Die Argumente stroemen als JSON-Bruchstuecke ein.
                        Some("input_json_delta") => {
                            if let Some(stueck) = delta.get("partial_json").and_then(|t| t.as_str()) {
                                zug.offene_argumente.push_str(stueck);

                                if let Some(name) = &zug.offenes_werkzeug {
                                    let args = anzeige::argumente_kurz(&zug.offene_argumente, 60);
                                    anzeige.taetigkeit(Some(if args.is_empty() {
                                        format!("{name} …")
                                    } else {
                                        format!("{name}  {args}")
                                    }));
                                }
                            }
                        }
                        _ => {}
                    }
                }

                // Die Argumente sind vollstaendig: Griff zeigen und ab jetzt
                // die Laufzeit zaehlen — das ist die Zeit, in der sonst
                // nichts passiert.
                "content_block_stop" => {
                    if let Some(name) = zug.offenes_werkzeug.take() {
                        let args = anzeige::argumente_kurz(&zug.offene_argumente, 72);
                        zug.griffe += 1;

                        anzeige.zeile(&format!(
                            "  \x1b[2m⚒ {name}\x1b[0m{}",
                            if args.is_empty() {
                                String::new()
                            } else {
                                format!("  \x1b[2m{args}\x1b[0m")
                            }
                        ));

                        anzeige.taetigkeit(Some(format!("{name} arbeitet")));
                        zug.letzter_griff = Some((name, args));
                        zug.offene_argumente.clear();
                    }
                }

                _ => {}
            }
        }

        // Das Ergebnis eines Werkzeugs ist da — ab hier denkt der Motor wieder.
        "user" => {
            let leer = Vec::new();
            let bloecke = e
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
                .unwrap_or(&leer);

            if bloecke
                .iter()
                .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result"))
            {
                anzeige.taetigkeit(Some("denkt".to_string()));
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

/// Welches Konto arbeitet, woher es kommt, und wie es heisst.
///
/// Die drei gehoeren zusammen: die Umgebung traegt das Token, die Quelle sagt,
/// welche Stufe der Kette gegriffen hat, und die Bezeichnung ist nur bei
/// Stufe 3 noetig (dort steht in `CLAUDE_CONFIG_DIR` ein leerer Ordner, und
/// dessen Name ist nicht das Konto). Einzeln durchgereicht waren es acht
/// Parameter fuer den Schirm — und drei davon beantworten dieselbe Frage.
struct Kontolage<'a> {
    umgebung: &'a BTreeMap<String, String>,
    quelle: konto::Quelle,
    label: Option<&'a str>,
}

impl Kontolage<'_> {
    /// Der Name, der im Schirm steht.
    fn name(&self) -> String {
        match (self.quelle, self.label) {
            (konto::Quelle::AiBrain, Some(label)) => label.to_string(),
            _ => konto::aktuelles(self.umgebung),
        }
    }
}

/// Der Schirm beim Aufgehen.
///
/// **Der Motor gehoert hierher.** Bastian: „es waere gut, wenn ich dann oben
/// direkt sehe, auf was das laeuft." Vorher stand hier, WELCHE Werkzeuge
/// bereitstehen, aber nicht, welches Modell und welches Konto arbeitet — und
/// das ist die Frage, die man sich bei jedem Zug stellt.
fn schirm(
    connection: &Connection,
    projekt: Option<&str>,
    verzeichnis: &Path,
    rechte: Rechte,
    lauf: &str,
    lage: Kontolage<'_>,
) {
    // Der Name der Instanz frisch statt gemerkt: auf diesem Rechner stand in
    // der Ablage noch „Laravel", waehrend die Instanz laengst „AI Brain" heisst.
    let instanz = chat::instanzname(connection);
    let rechner = gethostname::gethostname().to_string_lossy().to_string();
    let heimat = std::env::var("HOME").unwrap_or_default();
    let ort = verzeichnis.display().to_string();
    let ort = match ort.strip_prefix(&heimat) {
        Some(rest) if !heimat.is_empty() => format!("~{rest}"),
        _ => ort,
    };

    let feld = |name: &str, wert: &str| println!("  \x1b[2m{name:<10}\x1b[0m {wert}");

    println!();
    println!(
        "  \x1b[1;36m▰\x1b[0m \x1b[1mAI Brain Harness\x1b[0m \x1b[2m·\x1b[0m {instanz}"
    );
    println!("  \x1b[2m{}\x1b[0m", "─".repeat(66));

    feld(
        "Wer",
        &format!(
            "{} \x1b[2mauf\x1b[0m {rechner}  \x1b[2m·  Konto\x1b[0m {} \x1b[2m({})\x1b[0m",
            connection.owner,
            lage.name(),
            lage.quelle.lesbar()
        ),
    );

    let mut motorzeile = match konto::anbieter(lage.umgebung) {
        Some(a) => format!("claude \x1b[2m·\x1b[0m {}  \x1b[2m·\x1b[0m {a}", konto::modell(lage.umgebung)),
        None => format!("claude \x1b[2m·\x1b[0m {}", konto::modell(lage.umgebung)),
    };

    // Eine fehlende Anmeldung gehoert in die Motorzeile, nicht in die erste
    // Fehlermeldung eines Zuges. Wer das hier sieht, probiert nicht erst.
    if !konto::angemeldet(lage.umgebung) {
        motorzeile.push_str("  \x1b[33mnicht angemeldet\x1b[0m");
    }

    feld("Motor", &motorzeile);

    feld(
        "Hier",
        &match projekt {
            Some(p) => format!("{p} \x1b[2m·\x1b[0m {ort}"),
            None => format!("\x1b[2mkein Projekt ·\x1b[0m {ort}"),
        },
    );

    feld(
        "Greift auf",
        &format!(
            "{instanz} \x1b[2m·\x1b[0m diesen Rechner  \x1b[2m·\x1b[0m {}",
            rechte_lesbar(rechte)
        ),
    );

    println!("  \x1b[2m{}\x1b[0m", "─".repeat(66));
    println!(
        "  \x1b[2mLauf {lauf}  ·  /hilfe für Befehle ohne Modell  ·  Ctrl-D beendet\x1b[0m"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lesende_rechte_zaehlen_die_erlaubten_werkzeuge_auf() {
        let args = motor_args(Path::new("/usr/bin/ai-brain-harness"), Rechte::Fragend, None);

        assert!(args.iter().any(|a| a == "--allowedTools"));
        assert!(args.iter().any(|a| a.contains("mcp__rechner")));
        // Scharf: lesend darf nie den Riegel des Motors ausschalten.
        assert!(!args.iter().any(|a| a == "bypassPermissions"));
    }

    #[test]
    fn steuernde_rechte_zaehlen_nichts_auf() {
        let args = motor_args(Path::new("/usr/bin/ai-brain-harness"), Rechte::Vollzugriff, None);

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
        let mut zug = Zug::neu();
        let anzeige = Anzeige::neu();

        darstellen(
            &serde_json::json!({ "type": "system", "subtype": "init", "session_id": "s-1" }),
            &mut zug,
            &anzeige,
        );

        assert_eq!(zug.sitzung.as_deref(), Some("s-1"));
    }

    #[test]
    fn ein_abgebrochener_zug_wird_nicht_als_erfolg_gezaehlt() {
        let mut zug = Zug::neu();
        let anzeige = Anzeige::neu();

        darstellen(
            &serde_json::json!({
                "type": "result",
                "subtype": "error_max_turns",
                "session_id": "s-2",
                "total_cost_usd": 0.5,
            }),
            &mut zug,
            &anzeige,
        );

        assert!(zug.abgebrochen);
        assert_eq!(zug.sitzung.as_deref(), Some("s-2"));
    }

    #[test]
    fn der_werkzeugname_kommt_aus_dem_blockanfang() {
        // Das ist der Unterschied zwischen „ich sehe, was er tut" und „ich
        // sehe hinterher, was er getan hat": der Blockanfang nennt das
        // Werkzeug, BEVOR es laeuft.
        let mut zug = Zug::neu();
        let anzeige = Anzeige::neu();

        darstellen(
            &serde_json::json!({
                "type": "stream_event",
                "event": {
                    "type": "content_block_start",
                    "content_block": { "type": "tool_use", "name": "mcp__rechner__rechner-dienste" }
                }
            }),
            &mut zug,
            &anzeige,
        );

        // Weder das Praefix `mcp__` noch der Servername sagen dem Menschen
        // etwas Neues — der Server steht im Schirm.
        assert_eq!(zug.offenes_werkzeug.as_deref(), Some("rechner-dienste"));
    }

    #[test]
    fn die_argumente_werden_aus_bruchstuecken_zusammengesetzt() {
        let mut zug = Zug::neu();
        let anzeige = Anzeige::neu();

        for stueck in ["{\"befehl\": \"systemctl", " is-active rev", "erb\"}"] {
            darstellen(
                &serde_json::json!({
                    "type": "stream_event",
                    "event": {
                        "type": "content_block_delta",
                        "delta": { "type": "input_json_delta", "partial_json": stueck }
                    }
                }),
                &mut zug,
                &anzeige,
            );
        }

        assert_eq!(zug.offene_argumente, "{\"befehl\": \"systemctl is-active reverb\"}");
    }

    #[test]
    fn die_echte_ereignisfolge_eines_bash_griffs_zeigt_den_befehl() {
        // Die Folge ist am 15.10.2026 an `claude -p` gemessen. Der erste
        // Echttest druckte „⚒ Bash" OHNE Argumente, und ohne diesen Test
        // liess sich nicht unterscheiden, ob die Bruchstuecke nicht ankommen
        // oder die Darstellung sie verliert.
        let mut zug = Zug::neu();
        let anzeige = Anzeige::neu();

        let folge = vec![
            serde_json::json!({"type":"stream_event","event":{
                "type":"content_block_start",
                "content_block":{"type":"tool_use","id":"t1","name":"Bash","input":{}}}}),
            serde_json::json!({"type":"stream_event","event":{
                "type":"content_block_delta",
                "delta":{"type":"input_json_delta","partial_json":""}}}),
            serde_json::json!({"type":"stream_event","event":{
                "type":"content_block_delta",
                "delta":{"type":"input_json_delta","partial_json":"{\"command\": \"sleep 8 && echo FER"}}}),
            serde_json::json!({"type":"stream_event","event":{
                "type":"content_block_delta",
                "delta":{"type":"input_json_delta","partial_json":"TIG\", \"description\": \"Warten\"}"}}}),
            serde_json::json!({"type":"stream_event","event":{"type":"content_block_stop"}}),
        ];

        for e in &folge {
            darstellen(e, &mut zug, &anzeige);
        }

        let (name, args) = zug.letzter_griff.clone().expect("ein Griff muss gezaehlt sein");

        assert_eq!(name, "Bash");
        assert_eq!(args, "sleep 8 && echo FERTIG", "der Befehl gehoert in die Zeile");
        assert_eq!(zug.griffe, 1);
    }

    #[test]
    fn der_blockschluss_zaehlt_den_griff_und_raeumt_auf() {
        let mut zug = Zug::neu();
        let anzeige = Anzeige::neu();

        zug.offenes_werkzeug = Some("rechner-befehl".into());
        zug.offene_argumente = "{\"befehl\":\"true\"}".into();

        darstellen(
            &serde_json::json!({
                "type": "stream_event",
                "event": { "type": "content_block_stop" }
            }),
            &mut zug,
            &anzeige,
        );

        assert_eq!(zug.griffe, 1);
        // Aufgeraeumt, sonst traegt der naechste Griff die alten Argumente.
        assert!(zug.offenes_werkzeug.is_none());
        assert!(zug.offene_argumente.is_empty());
    }

    #[test]
    fn ein_erfolgreicher_zug_gilt_als_erfolg() {
        let mut zug = Zug::neu();
        let anzeige = Anzeige::neu();

        darstellen(
            &serde_json::json!({
                "type": "result",
                "subtype": "success",
                "session_id": "s-3",
                "total_cost_usd": 0.25,
            }),
            &mut zug,
            &anzeige,
        );

        assert!(!zug.abgebrochen);
        assert_eq!(zug.kosten, Some(0.25));
    }
}
