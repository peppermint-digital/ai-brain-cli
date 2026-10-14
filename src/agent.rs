//! `agent` — einen Agentenlauf auf diesem Rechner starten (#7666).
//!
//! **Die Schleife wird gemietet, nicht gebaut.** Dieses Programm ruft `claude`
//! bzw. `codex` auf. Kein eigener Modellaufruf, kein eigener Werkzeug-Dispatch,
//! keine eigene Kontextkürzung. Zwei Gründe:
//!
//! 1. Der Claude Agent SDK *ist* Claude Code als Bibliothek — eine eigene
//!    Schleife brächte bei Claude nichts, was es nicht schon gibt.
//! 2. Für Codex, Ollama und OpenRouter löst sie gar nichts. NanoClaw braucht
//!    heute **zwei** Mechanismen (SDK für Claude, codex-CLI für Codex); mieten
//!    ergibt **einen** für alle.
//!
//! Was dieses Modul beiträgt, ist das, was dem gemieteten Agenten fehlt:
//! Kontext aus AI Brain, eine geprüfte Umgebung, ein Eintrag im Lauf-Register
//! und ein Protokoll.
//!
//! **Rechte: bewusst keine Erweiterung.** Hier wird **kein**
//! `--permission-mode` gesetzt. Der unbeaufsichtigte Vollzugriff ist das Ziel
//! der Idee, aber er gehört hinter die Verzeichnis-Freigabe aus #7669 — und ein
//! Riegel, der nach dem Ding gebaut wird, das er sichern soll, hat nie
//! existiert. Bis dahin läuft ein Lauf mit den Vorgaben des jeweiligen
//! Werkzeugs.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::config::{self, Connection};
use crate::mcp;
use crate::register::{self, NeuerLauf};

/// Sitzungsvariablen, die ein von hier gestarteter Agent **nicht** erben darf.
///
/// Wird `claude` aus einer laufenden Claude-Code-Sitzung gestartet, hält es sich
/// für ein Kind davon: „Transcript saving is off — inherited
/// CLAUDE_CODE_CHILD_SESSION marker". Dieselbe Falle hat imkr bei seinem
/// Server-Daemon getroffen (v0.9.6). Ein langlebiger Prozess, den ein Agent
/// starten kann, darf dessen Sitzungsumgebung nicht weitergeben.
const FREMDE_SITZUNGSVARIABLEN: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
];

/// Wie oft ein Lebenszeichen ins Register geschrieben wird. Deutlich unter der
/// Toleranz des Registers (180 s), damit ein einzelner verpasster Takt den Lauf
/// nicht als verschwunden abstempelt.
const HERZSCHLAG_TAKT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Modell {
    Claude,
    Codex,
}

impl Modell {
    fn aus(wort: &str) -> Result<Self, String> {
        match wort {
            "claude" => Ok(Modell::Claude),
            "codex" => Ok(Modell::Codex),
            "ollama" | "openrouter" => Err(format!(
                "`{wort}` ist noch nicht verdrahtet. Es läuft über die codex-CLI \
                 mit eigener Basis-Adresse — bis das hier eingebaut ist, nimm \
                 `--modell codex` mit entsprechend eingerichtetem Konto."
            )),
            sonst => Err(format!(
                "Unbekanntes Modell `{sonst}`. Möglich: claude, codex."
            )),
        }
    }

    fn programm(&self) -> &'static str {
        match self {
            Modell::Claude => "claude",
            Modell::Codex => "codex",
        }
    }

    fn wort(&self) -> &'static str {
        match self {
            Modell::Claude => "claude",
            Modell::Codex => "codex",
        }
    }

    /// Der Aufruf für einen unbeaufsichtigten Lauf.
    fn befehl(&self, auftrag: &str) -> Vec<String> {
        match self {
            // `-p` ist der Druck-/Stapelmodus: ein Auftrag, eine Antwort, kein
            // Terminal nötig.
            Modell::Claude => vec!["-p".to_string(), auftrag.to_string()],
            Modell::Codex => vec!["exec".to_string(), auftrag.to_string()],
        }
    }
}

#[derive(Debug, Clone)]
pub struct Auftrag {
    pub modell: Modell,
    pub verzeichnis: PathBuf,
    pub projekt: Option<String>,
    pub task: Option<u64>,
    pub text: Option<String>,
    pub trocken: bool,
}

/// Zerlegt die Kommandozeile.
///
/// Eigene Funktion, damit sie ohne Netz und ohne Prozessstart prüfbar ist — die
/// Zerlegung ist die Stelle, an der sich Flüchtigkeitsfehler einnisten.
pub fn auftrag_lesen(args: &[String], arbeitsverzeichnis: PathBuf) -> Result<Auftrag, String> {
    let mut modell = Modell::Claude;
    let mut verzeichnis = arbeitsverzeichnis;
    let mut projekt = None;
    let mut task = None;
    let mut trocken = false;
    let mut freie = Vec::new();

    let mut i = 0;

    while i < args.len() {
        let wert = |i: usize, was: &str| -> Result<String, String> {
            args.get(i + 1)
                .cloned()
                .ok_or_else(|| format!("`{was}` braucht eine Angabe."))
        };

        match args[i].as_str() {
            "--modell" | "--model" => {
                modell = Modell::aus(&wert(i, "--modell")?)?;
                i += 2;
            }
            "--dir" | "--verzeichnis" => {
                verzeichnis = PathBuf::from(wert(i, "--dir")?);
                i += 2;
            }
            "--projekt" | "--project" => {
                projekt = Some(wert(i, "--projekt")?);
                i += 2;
            }
            "--task" | "--aufgabe" => {
                let roh = wert(i, "--task")?;
                let zahl = roh.trim_start_matches('#');
                task = Some(
                    zahl.parse::<u64>()
                        .map_err(|_| format!("`{roh}` ist keine Aufgaben-Nummer."))?,
                );
                i += 2;
            }
            "--trocken" | "--dry-run" => {
                trocken = true;
                i += 1;
            }
            sonst if sonst.starts_with("--") => {
                return Err(format!("Unbekannte Option `{sonst}`."));
            }
            sonst => {
                freie.push(sonst.to_string());
                i += 1;
            }
        }
    }

    if projekt.is_none() && task.is_none() && freie.is_empty() {
        return Err("Ohne Auftrag, Projekt oder Aufgabe gibt es nichts zu tun.\n\
                    Beispiele:\n  \
                    ai-brain-cli agent --task 7666\n  \
                    ai-brain-cli agent --projekt ai-brain \"Changelog schreiben\""
            .to_string());
    }

    Ok(Auftrag {
        modell,
        verzeichnis,
        projekt,
        task,
        text: if freie.is_empty() {
            None
        } else {
            Some(freie.join(" "))
        },
        trocken,
    })
}

/// Was dem Agenten vorab gesagt wird.
///
/// **Kurz halten, nicht alles hineinkopieren.** Der Agent hat den
/// AI-Brain-MCP; er kann Projekte, Memories und Aufgaben selbst holen. Würden
/// wir hier den ganzen Kontext einbetten, hätten wir eine zweite, veraltende
/// Kopie — dieselbe Form wie eine Kurzfassung, die von ihrer Quelle abdriftet.
/// Was hier hingehört, ist das, was der Agent **nicht** erfragen kann: welche
/// Aufgabe gemeint ist, und dass die Hausregeln gelten.
pub fn vorlauf(auftrag: &Auftrag, aufgabe: Option<&str>) -> String {
    let mut text = String::new();

    text.push_str("Du läufst als AI-Brain-Agent auf diesem Rechner, gestartet über `ai-brain-cli agent`.\n\n");

    if let Some(projekt) = &auftrag.projekt {
        text.push_str(&format!("Projekt: {projekt}\n"));
    }

    text.push_str(&format!(
        "Arbeitsverzeichnis: {}\n\n",
        auftrag.verzeichnis.display()
    ));

    text.push_str(
        "Beginne mit dem `/brain`-Workflow (Kontext laden, Timer, Dokumentation). \
         Die AI-Brain-Werkzeuge stehen dir über MCP zur Verfügung — hol dir \
         Projektkontext, Memories und Aufgaben selbst, statt zu raten.\n\n",
    );

    if let Some(aufgabe) = aufgabe {
        text.push_str("Die Aufgabe, an der du arbeitest:\n\n");
        text.push_str(aufgabe);
        text.push_str("\n\n");
    }

    if let Some(eigener) = &auftrag.text {
        text.push_str("Auftrag:\n\n");
        text.push_str(eigener);
        text.push('\n');
    }

    text
}

/// Prüft, was fehlt — und sagt es, statt in einen Fehler zu laufen.
fn umgebung_pruefen(auftrag: &Auftrag) -> Result<Vec<String>, String> {
    let mut hinweise = Vec::new();

    if !auftrag.verzeichnis.is_dir() {
        return Err(format!(
            "{} ist kein Verzeichnis.",
            auftrag.verzeichnis.display()
        ));
    }

    // Liegt das Werkzeug überhaupt im Pfad? `--version` statt `which`: Das gibt
    // es unter Windows nicht, und ein Programm, das sich nicht starten lässt,
    // ist genauso wenig brauchbar wie ein fehlendes.
    let vorhanden = Command::new(auftrag.modell.programm())
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok();

    if !vorhanden {
        return Err(format!(
            "`{}` ist auf diesem Rechner nicht aufrufbar. Ein Lauf braucht das \
             Werkzeug selbst — die CLI mietet seine Schleife, sie ersetzt sie nicht.",
            auftrag.modell.programm()
        ));
    }

    // Ohne MCP-Eintrag läuft der Agent ohne AI-Brain-Werkzeuge. Das ist kein
    // Abbruchgrund — aber er muss es wissen, sonst wundert er sich über einen
    // Agenten, der seinen Kontext nicht findet.
    if !mcp_eingetragen() {
        hinweise.push(
            "Kein MCP-Eintrag für AI Brain gefunden — der Agent läuft dann ohne \
             die AI-Brain-Werkzeuge. `ai-brain-cli configure` trägt ihn ein."
                .to_string(),
        );
    }

    Ok(hinweise)
}

fn mcp_eingetragen() -> bool {
    let Ok(pfad) = config::claude_config_path() else {
        return false;
    };

    let Ok(roh) = std::fs::read_to_string(pfad) else {
        return false;
    };

    serde_json::from_str::<serde_json::Value>(&roh)
        .ok()
        .and_then(|w| w.pointer("/mcpServers/ai-brain").cloned())
        .is_some()
}

/// Holt die Aufgabenbeschreibung aus AI Brain, wenn eine Nummer angegeben ist.
fn aufgabe_holen(connection: &Connection, task: u64) -> Result<String, String> {
    let token = mcp::access_token(connection)?;

    mcp::call_tool_text(
        connection,
        &token,
        "get-task-tool",
        serde_json::json!({ "id": task }),
    )
}

pub fn starten(args: &[String]) -> Result<(), String> {
    let hier = std::env::current_dir().map_err(|e| format!("Kein Arbeitsverzeichnis: {e}"))?;
    let auftrag = auftrag_lesen(args, hier)?;

    let hinweise = umgebung_pruefen(&auftrag)?;

    for hinweis in &hinweise {
        eprintln!("Hinweis: {hinweis}");
    }

    // Die Aufgabe kommt aus AI Brain — nur wenn eine Nummer dasteht. Ohne
    // Verbindung ist das ein harter Fehler: Ein Lauf „an Aufgabe #N", der die
    // Aufgabe nicht kennt, arbeitet an etwas Erfundenem.
    let aufgabe = match auftrag.task {
        Some(nummer) => {
            let connection = config::load()?;
            Some(aufgabe_holen(&connection, nummer)?)
        }
        None => None,
    };

    let text = vorlauf(&auftrag, aufgabe.as_deref());

    if auftrag.trocken {
        println!("Würde starten: {} {:?}", auftrag.modell.programm(), auftrag.modell.befehl("<vorlauf>"));
        println!("Verzeichnis  : {}", auftrag.verzeichnis.display());
        println!();
        println!("--- Vorlauf ---");
        print!("{text}");

        return Ok(());
    }

    let lauf = register::anlegen(NeuerLauf {
        modell: auftrag.modell.wort().to_string(),
        verzeichnis: auftrag.verzeichnis.display().to_string(),
        projekt: auftrag.projekt.clone(),
        task: auftrag.task,
        auftrag: auftrag.text.clone(),
        pid: None,
        pane: None,
    })?;

    let protokoll = register::protokoll_pfad(&lauf.id)?;
    let datei = std::fs::File::create(&protokoll)
        .map_err(|e| format!("{} lässt sich nicht anlegen: {e}", protokoll.display()))?;
    let fehler_datei = datei
        .try_clone()
        .map_err(|e| format!("Protokoll lässt sich nicht teilen: {e}"))?;

    let mut befehl = Command::new(auftrag.modell.programm());
    befehl
        .args(auftrag.modell.befehl(&text))
        .current_dir(&auftrag.verzeichnis)
        .stdin(Stdio::null())
        .stdout(Stdio::from(datei))
        .stderr(Stdio::from(fehler_datei));

    for name in FREMDE_SITZUNGSVARIABLEN {
        befehl.env_remove(name);
    }

    let mut kind = befehl
        .spawn()
        .map_err(|e| format!("{} liess sich nicht starten: {e}", auftrag.modell.programm()))?;

    // Prozessnummer nachtragen, damit `stop` ihn erreicht.
    let mut mit_pid = register::lesen(&lauf.id)?;
    mit_pid.pid = Some(kind.id());
    register::schreiben(&mit_pid)?;

    println!("Lauf {} gestartet ({}).", lauf.id, auftrag.modell.wort());
    println!("  Verzeichnis : {}", auftrag.verzeichnis.display());
    println!("  Protokoll   : {}", protokoll.display());
    println!();
    println!("  ai-brain-cli agents");
    println!("  ai-brain-cli logs {}", lauf.id);

    let weiter = Arc::new(AtomicBool::new(true));
    let takt = {
        let weiter = weiter.clone();
        let id = lauf.id.clone();

        std::thread::spawn(move || {
            takt_schleife(&weiter, HERZSCHLAG_TAKT, || {
                let _ = register::herzschlag(&id);
            })
        })
    };

    let ausgang = kind.wait();
    weiter.store(false, Ordering::Relaxed);
    let _ = takt.join();

    match ausgang {
        Ok(status) => {
            let code = status.code();
            register::abschliessen(&lauf.id, code, None)?;

            let mut raus = std::io::stdout();
            let _ = writeln!(
                raus,
                "\nLauf {} beendet{}.",
                lauf.id,
                code.map(|c| format!(" (Code {c})")).unwrap_or_default()
            );

            if code == Some(0) {
                Ok(())
            } else {
                Err(format!(
                    "Der Lauf endete mit Code {}. Protokoll: ai-brain-cli logs {}",
                    code.map(|c| c.to_string()).unwrap_or_else(|| "?".into()),
                    lauf.id
                ))
            }
        }
        Err(e) => {
            register::abschliessen(&lauf.id, None, Some(format!("{e}")))?;

            Err(format!("Der Lauf liess sich nicht abwarten: {e}"))
        }
    }
}

/// Ruft `stempeln` im Abstand `takt`, bis `weiter` false wird.
///
/// Die Tat wird eingespeist, damit die **Taktlogik** prüfbar ist, ohne eine
/// Ablage und ohne `XDG_STATE_HOME` zu setzen — die Umgebungsvariable gehört
/// schon dem Register-Test, und zwei Tests, die sich eine Variable teilen,
/// ziehen sie sich gegenseitig weg.
///
/// Es wird in Scheiben geschlafen, nicht einen ganzen Takt: Sonst wartet das
/// Ende eines Laufs bis zu 30 Sekunden auf diesen Faden, und die Meldung
/// „beendet" käme eine halbe Minute nach dem Ende.
fn takt_schleife(weiter: &AtomicBool, takt: Duration, mut stempeln: impl FnMut()) {
    let scheibe = takt.min(Duration::from_millis(200));
    let mut gewartet = Duration::ZERO;

    while weiter.load(Ordering::Relaxed) {
        std::thread::sleep(scheibe);
        gewartet += scheibe;

        if gewartet >= takt {
            gewartet = Duration::ZERO;

            // Nach dem Schlafen noch einmal prüfen: Ist der Lauf inzwischen
            // vorbei, darf kein Lebenszeichen mehr geschrieben werden — das
            // würde einen abgeschlossenen Lauf wieder frisch aussehen lassen.
            if !weiter.load(Ordering::Relaxed) {
                break;
            }

            stempeln();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auftrag_lesen_versteht_die_kommandozeile() {
        let hier = PathBuf::from("/tmp");

        // Vorgabe ist claude und das Arbeitsverzeichnis.
        let a = auftrag_lesen(&["Mach etwas".to_string()], hier.clone()).unwrap();
        assert_eq!(a.modell, Modell::Claude);
        assert_eq!(a.verzeichnis, hier);
        assert_eq!(a.text.as_deref(), Some("Mach etwas"));

        // Mehrere freie Wörter werden ein Auftrag — damit Anführungszeichen
        // nicht zur Pflicht werden.
        let a = auftrag_lesen(
            &["Mach".to_string(), "etwas".to_string(), "Gutes".to_string()],
            hier.clone(),
        )
        .unwrap();
        assert_eq!(a.text.as_deref(), Some("Mach etwas Gutes"));

        // Optionen in beliebiger Reihenfolge, Raute vor der Nummer erlaubt.
        let a = auftrag_lesen(
            &[
                "--task".to_string(),
                "#7666".to_string(),
                "--modell".to_string(),
                "codex".to_string(),
                "--dir".to_string(),
                "/tmp/x".to_string(),
                "--projekt".to_string(),
                "ai-brain".to_string(),
            ],
            hier.clone(),
        )
        .unwrap();
        assert_eq!(a.task, Some(7666));
        assert_eq!(a.modell, Modell::Codex);
        assert_eq!(a.verzeichnis, PathBuf::from("/tmp/x"));
        assert_eq!(a.projekt.as_deref(), Some("ai-brain"));

        // Fehlende Angaben und Unsinn werden gemeldet, nicht verschluckt.
        assert!(auftrag_lesen(&[], hier.clone()).is_err());
        assert!(auftrag_lesen(&["--modell".to_string()], hier.clone()).is_err());
        assert!(auftrag_lesen(&["--modell".to_string(), "gpt".to_string()], hier.clone()).is_err());
        assert!(auftrag_lesen(&["--task".to_string(), "bald".to_string()], hier.clone()).is_err());
        assert!(auftrag_lesen(&["--quatsch".to_string()], hier.clone()).is_err());

        // Ollama nennt den Weg, statt nur „unbekannt" zu sagen.
        let fehler = auftrag_lesen(
            &["--modell".to_string(), "ollama".to_string()],
            hier.clone(),
        )
        .unwrap_err();
        assert!(fehler.contains("codex"), "der Hinweis muss den Umweg nennen: {fehler}");
    }

    #[test]
    fn vorlauf_nennt_die_aufgabe_und_bettet_keinen_kontext_ein() {
        let auftrag = Auftrag {
            modell: Modell::Claude,
            verzeichnis: PathBuf::from("/tmp/projekt"),
            projekt: Some("ai-brain".to_string()),
            task: Some(7666),
            text: Some("Register verdrahten".to_string()),
            trocken: false,
        };

        let text = vorlauf(&auftrag, Some("# Task #7666\nLauf-Register anbinden"));

        assert!(text.contains("ai-brain"));
        assert!(text.contains("/tmp/projekt"));
        assert!(text.contains("/brain"), "die Hausregeln müssen drinstehen");
        assert!(text.contains("Lauf-Register anbinden"), "die Aufgabe gehört hinein");
        assert!(text.contains("Register verdrahten"));

        // Der Vorlauf bleibt klein: Er sagt, WO der Agent nachsieht, statt den
        // Kontext einzubetten — eine eingebettete Kopie veraltet.
        assert!(
            text.len() < 1200,
            "der Vorlauf soll kurz bleiben, ist aber {} Zeichen",
            text.len()
        );
    }

    #[test]
    fn takt_schleife_stempelt_im_abstand_und_hoert_sofort_auf() {
        use std::sync::atomic::AtomicUsize;

        let weiter = Arc::new(AtomicBool::new(true));
        let zaehler = Arc::new(AtomicUsize::new(0));

        let faden = {
            let weiter = weiter.clone();
            let zaehler = zaehler.clone();

            std::thread::spawn(move || {
                takt_schleife(&weiter, Duration::from_millis(50), || {
                    zaehler.fetch_add(1, Ordering::Relaxed);
                })
            })
        };

        std::thread::sleep(Duration::from_millis(260));
        weiter.store(false, Ordering::Relaxed);

        let vorher = std::time::Instant::now();
        faden.join().unwrap();
        let gebraucht = vorher.elapsed();

        let stempel = zaehler.load(Ordering::Relaxed);
        assert!(
            stempel >= 3,
            "in 260 ms bei 50-ms-Takt sollten mehrere Stempel fallen, es waren {stempel}"
        );

        assert!(
            gebraucht < Duration::from_millis(400),
            "der Faden hat {gebraucht:?} zum Beenden gebraucht"
        );

        // Und jetzt die Eigenschaft, auf die es in Produktion ankommt: Das Ende
        // darf NICHT auf einen ganzen Takt warten. Mit kleinem Takt oben wäre
        // der Test auch grün, wenn gar nicht in Scheiben geschlafen würde —
        // deshalb hier ein grosser Takt und ein sofortiges Ende.
        let weiter = Arc::new(AtomicBool::new(true));
        let faden = {
            let weiter = weiter.clone();

            std::thread::spawn(move || {
                takt_schleife(&weiter, Duration::from_secs(10), || {
                    panic!("bei 10-s-Takt darf in dieser Zeit nichts gestempelt werden");
                })
            })
        };

        std::thread::sleep(Duration::from_millis(50));
        weiter.store(false, Ordering::Relaxed);

        let vorher = std::time::Instant::now();
        faden.join().unwrap();

        assert!(
            vorher.elapsed() < Duration::from_millis(500),
            "ohne Scheiben wartet das Ende den ganzen Takt ab — hier {:?}",
            vorher.elapsed()
        );
    }

    /// Der Fall, der schon zweimal zugeschlagen hat: Ein Agent startet einen
    /// Prozess, und der erbt seine Sitzungsvariablen.
    #[test]
    fn fremde_sitzungsvariablen_sind_vollstaendig() {
        for pflicht in [
            "CLAUDECODE",
            "CLAUDE_CODE_SESSION_ID",
            "CLAUDE_CODE_CHILD_SESSION",
            "CLAUDE_PID",
        ] {
            assert!(
                FREMDE_SITZUNGSVARIABLEN.contains(&pflicht),
                "{pflicht} muss entfernt werden, sonst haelt sich der Agent fuer ein Kind"
            );
        }
    }
}
