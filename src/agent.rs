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
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::config::{self, Connection};
use crate::freigabe;
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

/// Wie lange ein Lauf höchstens dauert, wenn nichts anderes gesagt wird.
///
/// Eine Vorgabe und nicht „unbegrenzt": Ein unbeaufsichtigter Lauf, der hängt,
/// hält sonst ewig seinen Herzschlag und sieht für jeden Leser gesund aus.
/// Hermes hatte dafür `gateway_timeout: 1800`; eine Stunde ist für eine
/// Coding-Aufgabe die grosszügigere Wahl. `--zeitgrenze 0` hebt sie auf.
const ZEITGRENZE_VORGABE: Duration = Duration::from_secs(3600);

/// Welche Unterhaltung der Lauf benutzt.
///
/// Die grösste Lücke gegenüber Hermes war, dass unsere Läufe einmalig waren:
/// `claude -p` beginnt jedes Mal von vorn, während Hermes Sitzungen über
/// Monate hält. Die Schalter dafür gibt es in beiden Werkzeugen, wir reichten
/// sie nur nicht durch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sitzungswahl {
    /// Frische Unterhaltung. `kennung` ist eine selbst erzeugte UUID, mit der
    /// dieser Lauf später fortgesetzt werden kann — nur Claude nimmt eine
    /// vorgegebene (`--session-id`); codex vergibt seine eigene.
    Neu { kennung: Option<String> },
    /// Die letzte Unterhaltung in diesem Verzeichnis (`claude -c`,
    /// `codex exec resume --last`).
    Letzte,
    /// Eine bestimmte (`claude -r <id>`, `codex exec resume <id>`).
    Bestimmte(String),
}

/// Wie viel ein Lauf darf.
///
/// Die Stufen über `Fragend` brauchen eine Verzeichnis-Freigabe (#7669). Das
/// ist die ganze Idee des Riegels: Gesichert wird nicht „ein Agent läuft", sondern
/// „ein Agent fragt nicht mehr".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rechte {
    /// Vorgaben des Werkzeugs — es fragt, bevor es etwas tut.
    Fragend,
    /// Darf Dateien im Arbeitsbereich schreiben, Shell bleibt gefragt.
    Schreibend,
    /// Fragt nichts mehr. Der „God-Mode" aus Idee #493.
    Vollzugriff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modell {
    Claude,
    Codex,
}

impl Modell {
    pub fn aus(wort: &str) -> Result<Self, String> {
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

    pub fn programm(&self) -> &'static str {
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

    /// Die Rechte-Schalter, abgelesen an `--help` der Werkzeuge.
    /// Die Befehlszeile fuer ein GESPRAECH — wie `befehl_interaktiv`, nur ohne
    /// Auftragstext (#502).
    ///
    /// Der Unterschied ist nur der fehlende Text: Ohne ihn startet das Werkzeug
    /// in seinen Gespraechsmodus statt eine Aufgabe abzuarbeiten. Deshalb steht
    /// das hier und nicht in `chat.rs` — die Regeln, welcher Schalter welche
    /// Rechte bedeutet, gibt es genau einmal.
    pub fn gespraech_args(&self, rechte: Rechte, weiter: bool) -> Result<Vec<String>, String> {
        let mut args = Vec::new();

        match (self, weiter) {
            (Modell::Claude, true) => args.push("-c".to_string()),
            // `codex resume --last` ist ein eigener Unterbefehl und muss VOR
            // den Schaltern stehen.
            (Modell::Codex, true) => {
                args.push("resume".to_string());
                args.push("--last".to_string());
            }
            _ => {}
        }

        // `in_resume` gilt nur fuer `codex exec resume`; das interaktive
        // `codex resume` kennt die Schalter weiterhin.
        args.extend(self.rechte_args(rechte, false)?);

        Ok(args)
    }

    fn rechte_args(&self, rechte: Rechte, in_resume: bool) -> Result<Vec<String>, String> {
        Ok(match (self, rechte) {
            (_, Rechte::Fragend) => Vec::new(),
            (Modell::Claude, Rechte::Schreibend) => {
                vec!["--permission-mode".into(), "acceptEdits".into()]
            }
            (Modell::Claude, Rechte::Vollzugriff) => {
                vec!["--permission-mode".into(), "bypassPermissions".into()]
            }
            // `codex exec resume` kennt KEIN `-s/--sandbox` — nur den
            // Vollzugriff-Schalter. Die Stufe stillschweigend fallen zu lassen
            // wäre das Schlimmste: Der Lauf liefe dann mit anderen Rechten als
            // verlangt, und niemand sähe es.
            (Modell::Codex, Rechte::Schreibend) if in_resume => {
                return Err("`codex exec resume` kennt kein `-s/--sandbox` — `--schreibend` \
                            ist beim Fortsetzen mit codex nicht ausdrückbar. Entweder \
                            `--vollzugriff`, oder die Rechte-Stufe weglassen, oder \
                            `--modell claude`."
                    .to_string());
            }
            (Modell::Codex, Rechte::Schreibend) => vec!["-s".into(), "workspace-write".into()],
            (Modell::Codex, Rechte::Vollzugriff) => {
                vec!["--dangerously-bypass-approvals-and-sandbox".into()]
            }
        })
    }

    /// Der Aufruf für ein Pane: **interaktiv**, mit dem Vorlauf als Argument.
    ///
    /// Kein `-p`: Ein Pane ist zum Zusehen und Mitreden da. Mit `-p` wäre die
    /// Antwort gedruckt und das Pane sofort wieder zu — dann hätte der Umweg
    /// über imkr keinen Sinn. Beide Werkzeuge nehmen einen Start-Auftrag als
    /// Argument (`claude [options] [prompt]`, `codex [OPTIONS] [PROMPT]`).
    fn befehl_interaktiv(
        &self,
        auftrag: &str,
        rechte: Rechte,
        sitzung: &Sitzungswahl,
    ) -> Result<Vec<String>, String> {
        let mut args = Vec::new();

        match (self, sitzung) {
            (Modell::Claude, Sitzungswahl::Neu { kennung: Some(k) }) => {
                args.push("--session-id".to_string());
                args.push(k.clone());
            }
            (Modell::Claude, Sitzungswahl::Letzte) => args.push("-c".to_string()),
            (Modell::Claude, Sitzungswahl::Bestimmte(id)) => {
                args.push("-r".to_string());
                args.push(id.clone());
            }
            // Interaktives codex setzt die Sitzung über `codex resume`; das ist
            // ein anderer Aufruf und hier bewusst nicht gebaut, statt einen
            // Schalter zu erfinden.
            (Modell::Codex, Sitzungswahl::Letzte) | (Modell::Codex, Sitzungswahl::Bestimmte(_)) => {
                return Err("Fortsetzen im Pane geht nur mit `--modell claude`; für codex \
                            heisst der interaktive Weg `codex resume` und ist hier nicht \
                            verdrahtet."
                    .to_string());
            }
            _ => {}
        }

        args.extend(self.rechte_args(rechte, false)?);
        args.push(auftrag.to_string());

        Ok(args)
    }

    /// Der Aufruf für einen unbeaufsichtigten Lauf.
    ///
    /// Die Schalter sind an `--help` abgelesen, nicht erfunden:
    /// `claude -p [--session-id|-c|-r] [--permission-mode …]` und
    /// `codex exec [resume [id|--last]] [-s …]`.
    ///
    /// Eine **Zug-Grenze gibt es nicht** — `claude` kennt kein `--max-turns`.
    /// Deshalb setzt die CLI nur die Zeitgrenze, und zwar selbst.
    fn befehl(
        &self,
        auftrag: &str,
        rechte: Rechte,
        sitzung: &Sitzungswahl,
    ) -> Result<Vec<String>, String> {
        let mut args = Vec::new();

        match self {
            Modell::Claude => {
                args.push("-p".to_string());

                match sitzung {
                    Sitzungswahl::Neu { kennung: Some(k) } => {
                        args.push("--session-id".to_string());
                        args.push(k.clone());
                    }
                    Sitzungswahl::Neu { kennung: None } => {}
                    Sitzungswahl::Letzte => args.push("-c".to_string()),
                    Sitzungswahl::Bestimmte(id) => {
                        args.push("-r".to_string());
                        args.push(id.clone());
                    }
                }

                args.extend(self.rechte_args(rechte, false)?);
            }
            Modell::Codex => {
                args.push("exec".to_string());

                let fortsetzung = match sitzung {
                    Sitzungswahl::Letzte => {
                        args.push("resume".to_string());
                        args.push("--last".to_string());
                        true
                    }
                    Sitzungswahl::Bestimmte(id) => {
                        args.push("resume".to_string());
                        args.push(id.clone());
                        true
                    }
                    Sitzungswahl::Neu { .. } => false,
                };

                args.extend(self.rechte_args(rechte, fortsetzung)?);
            }
        }

        args.push(auftrag.to_string());

        Ok(args)
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
    pub rechte: Rechte,
    /// Lauf in einem imkr-Pane statt als beaufsichtigter Kindprozess (#7667).
    pub in_imkr: bool,
    pub sitzung: Sitzungswahl,
    /// `None` heisst: keine Grenze. Vorgabe ist eine Stunde — ein
    /// unbeaufsichtigter Lauf, der haengt, haelt sonst ewig seinen Herzschlag.
    pub zeitgrenze: Option<Duration>,
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
    let mut rechte = Rechte::Fragend;
    let mut in_imkr = false;
    let mut sitzung = Sitzungswahl::Neu { kennung: None };
    let mut zeitgrenze = Some(ZEITGRENZE_VORGABE);
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
            "--fortsetzen" | "--weiter" => {
                sitzung = Sitzungswahl::Letzte;
                i += 1;
            }
            "--sitzung" | "--session" => {
                sitzung = Sitzungswahl::Bestimmte(wert(i, "--sitzung")?);
                i += 2;
            }
            "--imkr" | "--pane" => {
                in_imkr = true;
                i += 1;
            }
            "--schreibend" => {
                rechte = Rechte::Schreibend;
                i += 1;
            }
            "--vollzugriff" => {
                rechte = Rechte::Vollzugriff;
                i += 1;
            }
            "--zeitgrenze" => {
                let roh = wert(i, "--zeitgrenze")?;
                let sekunden: u64 = roh
                    .parse()
                    .map_err(|_| format!("`{roh}` ist keine Zahl in Sekunden."))?;
                zeitgrenze = if sekunden == 0 {
                    None
                } else {
                    Some(Duration::from_secs(sekunden))
                };
                i += 2;
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
                    ai-brain-harness agent --task 7666\n  \
                    ai-brain-harness agent --projekt ai-brain \"Changelog schreiben\""
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
        rechte,
        in_imkr,
        sitzung,
        zeitgrenze,
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

    text.push_str("Du läufst als AI-Brain-Agent auf diesem Rechner, gestartet über `ai-brain-harness agent`.\n\n");

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

    // `codex exec` verweigert ausserhalb eines Git-Verzeichnisses den Dienst:
    // „Not inside a trusted directory and --skip-git-repo-check was not
    // specified." — Exit 1, ohne etwas zu tun. Gefunden beim ersten echten
    // Codex-Lauf am 14.10.2026 (in einem Repo läuft derselbe Auftrag durch).
    //
    // Gemeldet, nicht umgangen: `--skip-git-repo-check` wäre ein Riegel, den
    // wir für den Nutzer wegklicken — dieselbe Entscheidung wie bei der
    // Vertrauensabfrage von Claude Code, und sie gehört ihm.
    if auftrag.modell == Modell::Codex && !ist_git_verzeichnis(&auftrag.verzeichnis) {
        hinweise.push(format!(
            "{} ist kein Git-Verzeichnis — `codex exec` bricht dort mit „Not inside a \
             trusted directory\" ab, ohne etwas zu tun. Nimm ein Repository, oder \
             `--modell claude`.",
            auftrag.verzeichnis.display()
        ));
    }

    if auftrag.in_imkr && verzeichnis_vertraut(&auftrag.verzeichnis) != Some(true) {
        hinweise.push(format!(
            "{} ist in Claude Code noch nicht als vertrauenswürdig vermerkt — das Pane \
             wird zuerst danach fragen und bis dahin nichts tun. Einmal dort bestätigen, \
             dann läuft es.",
            auftrag.verzeichnis.display()
        ));
    }

    if auftrag.in_imkr && !crate::imkr::verfuegbar() {
        return Err(
            "`--imkr` verlangt imkr, das hier nicht aufrufbar ist. Ohne den Schalter \
             läuft derselbe Auftrag als beaufsichtigter Lauf mit Protokoll."
                .to_string(),
        );
    }

    // Ohne MCP-Eintrag läuft der Agent ohne AI-Brain-Werkzeuge. Das ist kein
    // Abbruchgrund — aber er muss es wissen, sonst wundert er sich über einen
    // Agenten, der seinen Kontext nicht findet.
    if !mcp_eingetragen() {
        hinweise.push(
            "Kein MCP-Eintrag für AI Brain gefunden — der Agent läuft dann ohne \
             die AI-Brain-Werkzeuge. `ai-brain-harness configure` trägt ihn ein."
                .to_string(),
        );
    }

    Ok(hinweise)
}

/// Liegt das Verzeichnis in einem Git-Arbeitsbaum?
fn ist_git_verzeichnis(verzeichnis: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(verzeichnis)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Hat Claude Code dieses Verzeichnis schon als vertrauenswürdig vermerkt?
///
/// Gefunden bei der ersten Pane-Probe: In einem unbekannten Verzeichnis fragt
/// Claude Code zuerst „Is this a project you trust?". Im Druckmodus (`-p`)
/// passiert das nicht, in einem Pane schon — und dort hängt der Lauf dann an
/// einem Dialog, während imkr `done` meldet.
///
/// Wir **beantworten** die Frage ausdrücklich nicht. Eine
/// Sicherheitsabfrage für jemand anderen wegzuklicken, indem man ihm ein Flag
/// in die Konfiguration schreibt, ist keine Bequemlichkeit, sondern eine
/// Entscheidung, die ihm gehört. Wir sagen nur Bescheid, dass sie kommt.
fn verzeichnis_vertraut(verzeichnis: &Path) -> Option<bool> {
    let pfad = config::claude_config_path().ok()?;
    let roh = std::fs::read_to_string(pfad).ok()?;
    let wert: serde_json::Value = serde_json::from_str(&roh).ok()?;

    let eintrag = wert
        .get("projects")?
        .get(verzeichnis.to_str()?)?
        .get("hasTrustDialogAccepted")?;

    Some(eintrag.as_bool().unwrap_or(false))
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

/// Eine wohlgeformte UUID, abgeleitet aus der Lauf-Kennung.
///
/// Claude nimmt eine **vorgegebene** Sitzungskennung (`--session-id <uuid>`).
/// Das ist der saubere Weg: Wir kennen sie damit ohne die Ausgabe zu parsen —
/// `--output-format json` hätte das Protokoll von lesbarem Text in JSON
/// verwandelt, nur um eine Zeichenkette zu erfahren.
///
/// Abgeleitet statt gewürfelt, weil die CLI bei fünf Abhängigkeiten bleibt
/// (kein `uuid`, kein `rand`) — und weil die Zuordnung Lauf ↔ Sitzung damit
/// nachvollziehbar ist. Die Lauf-Kennung ist bereits eindeutig (das Dateisystem
/// hat sie beansprucht), also ist die Ableitung es auch.
fn sitzungskennung(lauf_id: &str) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(format!("ai-brain-harness-sitzung:{lauf_id}").as_bytes());
    let mut b = [0u8; 16];
    b.copy_from_slice(&digest[..16]);

    // Fassung 4 und Variante 1 setzen, damit es eine gültige UUID ist und nicht
    // nur so aussieht — `claude --session-id` verlangt eine echte.
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;

    let h: Vec<String> = b.iter().map(|x| format!("{x:02x}")).collect();
    let h = h.join("");

    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// Meldet ein Ereignis des Laufs an AI Brain — bestmöglich, nie blockierend.
///
/// **Ein fehlgeschlagenes Protokoll darf den Lauf nicht verhindern.** Die
/// Arbeit passiert auf diesem Rechner; ist AI Brain gerade nicht erreichbar,
/// ist das ein Grund für eine Zeile auf stderr, nicht dafür, die Arbeit
/// abzusagen. Umgekehrt wäre es die Sorte Abhängigkeit, die man später nicht
/// mehr los wird.
///
/// Person und Gerät muss hier niemand mitgeben: Die Person füllt AI Brain aus
/// dem Token (`BelongsToActingUser`), den Gerätenamen aus dem OAuth-Client.
fn melden(
    connection: Option<&Connection>,
    aktion: &str,
    projekt: Option<&str>,
    status: &str,
    kontext: &str,
    nutzlast: serde_json::Value,
    dauer_ms: Option<u64>,
) {
    let Some(connection) = connection else {
        return;
    };

    let token = match mcp::access_token(connection) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Hinweis: Lauf nicht an AI Brain gemeldet ({e}).");

            return;
        }
    };

    let mut argumente = serde_json::json!({
        "agent": "ai-brain-harness",
        "action": aktion,
        "status": status,
        "context": kontext,
        "input": nutzlast,
    });

    if let Some(projekt) = projekt {
        argumente["project"] = serde_json::json!(projekt);
    }

    if let Some(ms) = dauer_ms {
        argumente["duration_ms"] = serde_json::json!(ms);
    }

    if let Err(e) = mcp::call_tool_text(connection, &token, "log-agent-action-tool", argumente) {
        eprintln!("Hinweis: Lauf nicht an AI Brain gemeldet ({e}).");
    }
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

    rechte_pruefen(&auftrag)?;

    // Die Aufgabe kommt aus AI Brain — nur wenn eine Nummer dasteht. Ohne
    // Verbindung ist das ein harter Fehler: Ein Lauf „an Aufgabe #N", der die
    // Aufgabe nicht kennt, arbeitet an etwas Erfundenem.
    // Die Verbindung einmal laden — für die Aufgabe UND fürs Melden. Ohne
    // Verbindung läuft der Agent trotzdem: Die Arbeit ist lokal, und ein
    // Rechner, der gerade nicht an AI Brain hängt, soll nicht stillstehen.
    // Nur mit `--task` ist sie Pflicht, denn ein Lauf „an Aufgabe #N", der die
    // Aufgabe nicht kennt, arbeitet an etwas Erfundenem.
    let connection = match (config::load(), auftrag.task) {
        (Ok(c), _) => Some(c),
        (Err(e), Some(_)) => return Err(e),
        (Err(e), None) => {
            eprintln!("Hinweis: nicht mit AI Brain verbunden ({e}) — der Lauf wird nicht gemeldet.");

            None
        }
    };

    let aufgabe = match (auftrag.task, connection.as_ref()) {
        (Some(nummer), Some(c)) => Some(aufgabe_holen(c, nummer)?),
        _ => None,
    };

    let text = vorlauf(&auftrag, aufgabe.as_deref());

    if auftrag.trocken {
        println!(
            "Würde starten: {} {:?}",
            auftrag.modell.programm(),
            auftrag.modell.befehl("<vorlauf>", auftrag.rechte, &auftrag.sitzung)?
        );
        println!("Verzeichnis  : {}", auftrag.verzeichnis.display());
        println!("Rechte       : {}", rechte_wort(auftrag.rechte));
        println!(
            "Weg          : {}",
            if auftrag.in_imkr {
                "imkr-Pane (interaktiv, Zustand von imkr)"
            } else {
                "beaufsichtigt (Kindprozess, Protokoll, Herzschlag)"
            }
        );
        println!(
            "Zeitgrenze   : {}",
            auftrag
                .zeitgrenze
                .map(|g| format!("{} s", g.as_secs()))
                .unwrap_or_else(|| "keine".to_string())
        );
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

    melden(
        connection.as_ref(),
        "agent_run_started",
        auftrag.projekt.as_deref(),
        "success",
        &format!("Lauf {} ({})", lauf.id, auftrag.modell.wort()),
        serde_json::json!({
            "lauf": lauf.id,
            "modell": auftrag.modell.wort(),
            "verzeichnis": auftrag.verzeichnis.display().to_string(),
            "rechte": rechte_wort(auftrag.rechte),
            "weg": if auftrag.in_imkr { "imkr-pane" } else { "beaufsichtigt" },
            "task": auftrag.task,
        }),
        None,
    );

    // Der imkr-Weg endet hier: Das Pane gehört imkr, nicht uns. Es gibt
    // nichts zu beaufsichtigen, kein Protokoll zu schreiben und keinen
    // Herzschlag zu stempeln — den Zustand liefert imkr, und `logs` zeigt den
    // Schirm statt einer Datei. Würden wir trotzdem hier warten, hielte die
    // CLI ein Terminal fest, das niemand braucht.
    if auftrag.in_imkr {
        let args = auftrag
            .modell
            .befehl_interaktiv(&text, auftrag.rechte, &auftrag.sitzung)?;

        match crate::imkr::pane_starten(&auftrag.verzeichnis, auftrag.modell.programm(), &args) {
            Ok(adresse) => {
                let mut mit_pane = register::lesen(&lauf.id)?;
                mit_pane.pane = Some(adresse.clone());
                register::schreiben(&mit_pane)?;

                println!("Lauf {} läuft in Pane {adresse}.", lauf.id);
                println!("  Verzeichnis : {}", auftrag.verzeichnis.display());
                println!();
                println!("  imkr read {adresse}");
                println!("  imkr msg {adresse} \"...\"");
                println!("  ai-brain-harness agents");

                return Ok(());
            }
            Err(e) => {
                // Der Lauf ist schon angelegt — ohne diesen Abschluss bliebe er
                // für immer auf „laeuft" stehen, obwohl nie etwas startete.
                register::abschliessen(&lauf.id, None, Some(e.clone()))?;

                return Err(e);
            }
        }
    }

    // Jetzt steht die Lauf-Kennung, also auch die abgeleitete Sitzungskennung.
    // Nur Claude nimmt eine vorgegebene; codex vergibt seine eigene, dort
    // bleibt `Neu` ohne Kennung.
    let sitzung = match (&auftrag.sitzung, &auftrag.modell) {
        (Sitzungswahl::Neu { .. }, Modell::Claude) => Sitzungswahl::Neu {
            kennung: Some(sitzungskennung(&lauf.id)),
        },
        (sonst, _) => sonst.clone(),
    };

    if let Sitzungswahl::Neu { kennung: Some(k) } = &sitzung {
        let mut mit = register::lesen(&lauf.id)?;
        mit.sitzung = Some(k.clone());
        register::schreiben(&mit)?;
    } else if let Sitzungswahl::Bestimmte(id) = &sitzung {
        let mut mit = register::lesen(&lauf.id)?;
        mit.sitzung = Some(id.clone());
        register::schreiben(&mit)?;
    }

    let protokoll = register::protokoll_pfad(&lauf.id)?;
    let datei = std::fs::File::create(&protokoll)
        .map_err(|e| format!("{} lässt sich nicht anlegen: {e}", protokoll.display()))?;
    let fehler_datei = datei
        .try_clone()
        .map_err(|e| format!("Protokoll lässt sich nicht teilen: {e}"))?;

    let mut befehl = Command::new(auftrag.modell.programm());
    befehl
        .args(auftrag.modell.befehl(&text, auftrag.rechte, &sitzung)?)
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
    println!("  ai-brain-harness agents");
    println!("  ai-brain-harness logs {}", lauf.id);

    // Abfrage-Schleife im Hauptfaden: Lebenszeichen schreiben, Zeitgrenze
    // durchsetzen, Ende erkennen. In Sekundenscheiben, damit das Ende nicht bis
    // zum Ablauf eines Taktes wartet — bei 30 s Takt wären das bis zu 30 s
    // Verzögerung zwischen „fertig" und der Meldung.
    let beginn = std::time::Instant::now();
    let mut seit_stempel = Duration::ZERO;
    let mut abgebrochen = None;

    let ausgang = loop {
        match kind.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Err(e) => break Err(e),
            Ok(None) => {}
        }

        std::thread::sleep(Duration::from_secs(1));
        seit_stempel += Duration::from_secs(1);

        match naechste_tat(beginn.elapsed(), seit_stempel, auftrag.zeitgrenze) {
            Tat::Warten => {}
            Tat::Stempeln => {
                seit_stempel = Duration::ZERO;
                let _ = register::herzschlag(&lauf.id);
            }
            Tat::Abbrechen => {
                let grenze = auftrag.zeitgrenze.map(|g| g.as_secs()).unwrap_or(0);
                eprintln!("\nZeitgrenze von {grenze} s erreicht — der Lauf wird beendet.");

                // Erst freundlich, dann hart: Ein Agent soll seine Sitzung noch
                // schliessen können. Wartet man danach nicht, bleibt ein
                // Zombie, und `agents` zeigte einen Lauf, den es nicht gibt.
                let _ = kind.kill();
                abgebrochen = Some(format!("Zeitgrenze von {grenze} s erreicht"));

                break kind.wait();
            }
        }
    };

    match ausgang {
        Ok(status) => {
            let code = status.code();
            register::abschliessen(&lauf.id, code, abgebrochen.clone())?;

            melden(
                connection.as_ref(),
                "agent_run_finished",
                auftrag.projekt.as_deref(),
                if code == Some(0) && abgebrochen.is_none() {
                    "success"
                } else {
                    "error"
                },
                &format!("Lauf {} beendet", lauf.id),
                serde_json::json!({
                    "lauf": lauf.id,
                    "exit_code": code,
                    "grund": abgebrochen,
                    "task": auftrag.task,
                }),
                Some(beginn.elapsed().as_millis() as u64),
            );

            let mut raus = std::io::stdout();
            let _ = writeln!(
                raus,
                "\nLauf {} beendet{}.",
                lauf.id,
                code.map(|c| format!(" (Code {c})")).unwrap_or_default()
            );

            if let Some(grund) = abgebrochen {
                return Err(format!("{grund}. Protokoll: ai-brain-harness logs {}", lauf.id));
            }

            if code == Some(0) {
                Ok(())
            } else {
                Err(format!(
                    "Der Lauf endete mit Code {}. Protokoll: ai-brain-harness logs {}",
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

fn rechte_wort(rechte: Rechte) -> &'static str {
    match rechte {
        Rechte::Fragend => "fragend (Vorgaben des Werkzeugs)",
        Rechte::Schreibend => "schreibend",
        Rechte::Vollzugriff => "VOLLZUGRIFF — fragt nichts mehr",
    }
}

/// Der Riegel: Alles über `Fragend` braucht eine Verzeichnis-Freigabe.
///
/// Steht hier und nicht erst im Werkzeugaufruf, damit kein Lauf angelegt wird,
/// den es nicht geben darf — und damit die Meldung sagt, was zu tun ist, statt
/// nur „nicht erlaubt".
fn rechte_pruefen(auftrag: &Auftrag) -> Result<(), String> {
    if auftrag.rechte == Rechte::Fragend {
        return Ok(());
    }

    // Aufgelöst vergleichen, nicht als Zeichenkette: `--dir ~/frei/../geheim`
    // sieht sonst aus wie ein freigegebener Pfad.
    let verzeichnis = std::fs::canonicalize(&auftrag.verzeichnis)
        .map_err(|e| format!("{} lässt sich nicht auflösen: {e}", auftrag.verzeichnis.display()))?;

    if freigabe::ist_frei(&verzeichnis) {
        return Ok(());
    }

    Err(format!(
        "{} ist nicht freigegeben — `{}` gibt es dort nicht.\n\n\
         Ein Lauf ohne diese Schalter fragt, bevor er etwas tut, und braucht \
         keine Freigabe. Erst wenn er nicht mehr fragt, zählt das Verzeichnis:\n\n  \
         ai-brain-harness freigabe add {}\n\n\
         Was freigegeben ist:  ai-brain-harness freigabe",
        verzeichnis.display(),
        if auftrag.rechte == Rechte::Vollzugriff {
            "--vollzugriff"
        } else {
            "--schreibend"
        },
        verzeichnis.display()
    ))
}

/// Was der Aufseher in diesem Augenblick tun soll.
///
/// Reine Funktion über Zeiten — damit die **Regeln** prüfbar sind, ohne Fäden,
/// ohne Schlafen und ohne Ablage. Vorher stand hier eine Schleife mit
/// eingespeister Tat; die Zeitgrenze braucht ohnehin eine Abfrage-Schleife im
/// Hauptfaden (`try_wait`), und ein zweiter Faden daneben wäre dann nur noch
/// eine weitere Stelle, an der etwas schiefgehen kann.
#[derive(Debug, PartialEq, Eq)]
pub enum Tat {
    Warten,
    Stempeln,
    Abbrechen,
}

pub fn naechste_tat(verstrichen: Duration, seit_stempel: Duration, grenze: Option<Duration>) -> Tat {
    // Die Grenze schlägt den Stempel: Ist die Zeit um, wird nicht erst noch ein
    // Lebenszeichen für einen Lauf geschrieben, der gleich abgebrochen wird.
    // Andernfalls stünde im Register ein frisches Lebenszeichen neben dem
    // Abbruch — und wer das liest, hält den Lauf für gesund.
    if grenze.is_some_and(|g| verstrichen >= g) {
        return Tat::Abbrechen;
    }

    if seit_stempel >= HERZSCHLAG_TAKT {
        return Tat::Stempeln;
    }

    Tat::Warten
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
            rechte: Rechte::Fragend,
            in_imkr: false,
            sitzung: Sitzungswahl::Neu { kennung: None },
            zeitgrenze: Some(ZEITGRENZE_VORGABE),
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
    fn naechste_tat_haelt_die_regeln() {
        let s = Duration::from_secs;

        // Normalbetrieb: nichts tun, bis der Takt voll ist.
        assert_eq!(naechste_tat(s(5), s(5), Some(s(3600))), Tat::Warten);
        assert_eq!(naechste_tat(s(29), s(29), Some(s(3600))), Tat::Warten);
        assert_eq!(naechste_tat(s(30), s(30), Some(s(3600))), Tat::Stempeln);
        assert_eq!(naechste_tat(s(600), s(45), Some(s(3600))), Tat::Stempeln);

        // Ohne Grenze wird nie abgebrochen, egal wie lange es dauert.
        assert_eq!(naechste_tat(s(999_999), s(1), None), Tat::Warten);
        assert_eq!(naechste_tat(s(999_999), s(60), None), Tat::Stempeln);

        // Die Grenze greift genau beim Erreichen, nicht erst danach.
        assert_eq!(naechste_tat(s(60), s(1), Some(s(60))), Tat::Abbrechen);
        assert_eq!(naechste_tat(s(59), s(1), Some(s(60))), Tat::Warten);

        // Und sie schlägt den Stempel. Sonst stünde im Register ein frisches
        // Lebenszeichen neben dem Abbruch, und der Lauf sähe gesund aus.
        assert_eq!(naechste_tat(s(60), s(300), Some(s(60))), Tat::Abbrechen);
    }

    #[test]
    fn rechte_setzen_die_abgelesenen_schalter() {
        // Gegen `--help` der Werkzeuge abgeglichen, nicht aus dem Gedächtnis.
        let neu = Sitzungswahl::Neu { kennung: None };
        let claude = Modell::Claude;
        assert_eq!(claude.befehl("hi", Rechte::Fragend, &neu).unwrap(), vec!["-p", "hi"]);
        assert_eq!(
            claude.befehl("hi", Rechte::Schreibend, &neu).unwrap(),
            vec!["-p", "--permission-mode", "acceptEdits", "hi"]
        );
        assert_eq!(
            claude.befehl("hi", Rechte::Vollzugriff, &neu).unwrap(),
            vec!["-p", "--permission-mode", "bypassPermissions", "hi"]
        );

        let codex = Modell::Codex;
        assert_eq!(codex.befehl("hi", Rechte::Fragend, &neu).unwrap(), vec!["exec", "hi"]);
        assert_eq!(
            codex.befehl("hi", Rechte::Schreibend, &neu).unwrap(),
            vec!["exec", "-s", "workspace-write", "hi"]
        );
        assert_eq!(
            codex.befehl("hi", Rechte::Vollzugriff, &neu).unwrap(),
            vec!["exec", "--dangerously-bypass-approvals-and-sandbox", "hi"]
        );

        // Der Auftrag steht IMMER am Ende — stünde er vor einem Schalter,
        // nähme das Werkzeug ihn als Wert dieses Schalters.
        for rechte in [Rechte::Fragend, Rechte::Schreibend, Rechte::Vollzugriff] {
            for modell in [Modell::Claude, Modell::Codex] {
                let args = modell.befehl("DER-AUFTRAG", rechte, &neu).unwrap();
                assert_eq!(args.last().unwrap(), "DER-AUFTRAG");
            }
        }
    }

    #[test]
    fn imkr_weg_ist_interaktiv_und_optional() {
        // Vorgabe ist der beaufsichtigte Weg.
        let a = auftrag_lesen(&["x".to_string()], PathBuf::from("/tmp")).unwrap();
        assert!(!a.in_imkr);

        let a = auftrag_lesen(
            &["--imkr".to_string(), "x".to_string()],
            PathBuf::from("/tmp"),
        )
        .unwrap();
        assert!(a.in_imkr);

        // Der Pane-Aufruf darf KEIN -p tragen: Mit -p waere die Antwort
        // gedruckt und das Pane sofort wieder zu — dann hat der Umweg über
        // imkr keinen Sinn.
        for modell in [Modell::Claude, Modell::Codex] {
            let args = modell
                .befehl_interaktiv("AUFTRAG", Rechte::Vollzugriff, &Sitzungswahl::Neu { kennung: None })
                .unwrap();

            assert!(
                !args.contains(&"-p".to_string()),
                "interaktiv darf nicht -p setzen: {args:?}"
            );
            assert!(
                !args.contains(&"exec".to_string()),
                "interaktiv darf nicht `exec` setzen: {args:?}"
            );
            assert_eq!(args.last().unwrap(), "AUFTRAG");
        }

        // Die Rechte gelten auf beiden Wegen gleich.
        let neu = Sitzungswahl::Neu { kennung: None };
        assert_eq!(
            Modell::Claude.befehl_interaktiv("x", Rechte::Vollzugriff, &neu).unwrap(),
            vec!["--permission-mode", "bypassPermissions", "x"]
        );
        assert_eq!(
            Modell::Codex.befehl_interaktiv("x", Rechte::Schreibend, &neu).unwrap(),
            vec!["-s", "workspace-write", "x"]
        );
        assert_eq!(
            Modell::Claude.befehl_interaktiv("x", Rechte::Fragend, &neu).unwrap(),
            vec!["x"]
        );
    }

    #[test]
    fn fortsetzen_setzt_die_abgelesenen_schalter() {
        // Abgeglichen an `--help`, und am echten Werkzeug nachgewiesen: eine
        // selbst erzeugte UUID an `claude -p --session-id` übergeben, danach
        // mit `-r` fortgesetzt — das Modell erinnerte sich.
        let letzte = Sitzungswahl::Letzte;
        let bestimmte = Sitzungswahl::Bestimmte("abc-123".to_string());
        let mit_kennung = Sitzungswahl::Neu {
            kennung: Some("7f3a9c21-4e5b-4a7d-8c91-2b6e4f1a0d33".to_string()),
        };

        assert_eq!(
            Modell::Claude.befehl("w", Rechte::Fragend, &mit_kennung).unwrap(),
            vec!["-p", "--session-id", "7f3a9c21-4e5b-4a7d-8c91-2b6e4f1a0d33", "w"]
        );
        assert_eq!(
            Modell::Claude.befehl("w", Rechte::Fragend, &letzte).unwrap(),
            vec!["-p", "-c", "w"]
        );
        assert_eq!(
            Modell::Claude.befehl("w", Rechte::Fragend, &bestimmte).unwrap(),
            vec!["-p", "-r", "abc-123", "w"]
        );

        // codex nimmt KEINE vorgegebene Kennung — `Neu` bleibt dort nackt.
        assert_eq!(
            Modell::Codex.befehl("w", Rechte::Fragend, &mit_kennung).unwrap(),
            vec!["exec", "w"]
        );
        assert_eq!(
            Modell::Codex.befehl("w", Rechte::Fragend, &letzte).unwrap(),
            vec!["exec", "resume", "--last", "w"]
        );
        assert_eq!(
            Modell::Codex.befehl("w", Rechte::Vollzugriff, &bestimmte).unwrap(),
            vec![
                "exec",
                "resume",
                "abc-123",
                "--dangerously-bypass-approvals-and-sandbox",
                "w"
            ]
        );

        // Und die Kombination, die NICHT geht: `codex exec resume` kennt kein
        // `-s/--sandbox`. Stillschweigend fallen lassen wäre das Schlimmste —
        // der Lauf liefe mit anderen Rechten als verlangt, und niemand sähe es.
        let fehler = Modell::Codex
            .befehl("w", Rechte::Schreibend, &letzte)
            .unwrap_err();
        assert!(fehler.contains("resume"), "{fehler}");
        assert!(fehler.contains("vollzugriff") || fehler.contains("Vollzugriff"), "{fehler}");

        // Ohne Fortsetzung geht `--schreibend` bei codex sehr wohl.
        assert!(Modell::Codex
            .befehl("w", Rechte::Schreibend, &Sitzungswahl::Neu { kennung: None })
            .is_ok());
    }

    #[test]
    fn sitzungskennung_ist_eine_gueltige_uuid_und_stabil() {
        let a = sitzungskennung("1760440000-999");
        let b = sitzungskennung("1760440000-999");
        let c = sitzungskennung("1760440000-998");

        assert_eq!(a, b, "dieselbe Lauf-Kennung muss dieselbe Sitzung ergeben");
        assert_ne!(a, c, "verschiedene Läufe brauchen verschiedene Sitzungen");

        // `claude --session-id` verlangt eine ECHTE UUID — Form, Fassung 4 und
        // Variante müssen stimmen, nicht nur so aussehen.
        let teile: Vec<&str> = a.split('-').collect();
        assert_eq!(
            teile.iter().map(|t| t.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12],
            "{a}"
        );
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() || c == '-'), "{a}");
        assert_eq!(teile[2].chars().next().unwrap(), '4', "Fassung 4: {a}");
        assert!(
            matches!(teile[3].chars().next().unwrap(), '8' | '9' | 'a' | 'b'),
            "Variante 1: {a}"
        );
    }

    #[test]
    fn vorgaben_sind_vorsichtig() {
        let a = auftrag_lesen(&["etwas".to_string()], PathBuf::from("/tmp")).unwrap();

        assert_eq!(
            a.rechte,
            Rechte::Fragend,
            "ohne Schalter darf ein Lauf nicht mehr duerfen als fragen"
        );
        assert_eq!(
            a.zeitgrenze,
            Some(ZEITGRENZE_VORGABE),
            "ohne Angabe muss eine Grenze gelten — ein haengender Lauf haelt sonst ewig seinen Herzschlag"
        );

        // Und die Schalter wirken.
        let a = auftrag_lesen(
            &["--vollzugriff".to_string(), "x".to_string()],
            PathBuf::from("/tmp"),
        )
        .unwrap();
        assert_eq!(a.rechte, Rechte::Vollzugriff);

        let a = auftrag_lesen(
            &["--zeitgrenze".to_string(), "0".to_string(), "x".to_string()],
            PathBuf::from("/tmp"),
        )
        .unwrap();
        assert_eq!(a.zeitgrenze, None, "0 hebt die Grenze auf");

        // Vorgabe ist eine frische Unterhaltung, nicht die letzte — sonst
        // hängte ein Lauf unbemerkt an einem fremden Gespräch.
        let a = auftrag_lesen(&["x".to_string()], PathBuf::from("/tmp")).unwrap();
        assert_eq!(a.sitzung, Sitzungswahl::Neu { kennung: None });

        let a = auftrag_lesen(
            &["--fortsetzen".to_string(), "x".to_string()],
            PathBuf::from("/tmp"),
        )
        .unwrap();
        assert_eq!(a.sitzung, Sitzungswahl::Letzte);

        let a = auftrag_lesen(
            &["--sitzung".to_string(), "abc".to_string(), "x".to_string()],
            PathBuf::from("/tmp"),
        )
        .unwrap();
        assert_eq!(a.sitzung, Sitzungswahl::Bestimmte("abc".to_string()));

        assert!(auftrag_lesen(&["--sitzung".to_string()], PathBuf::from("/tmp")).is_err());

        assert!(auftrag_lesen(
            &["--zeitgrenze".to_string(), "bald".to_string()],
            PathBuf::from("/tmp")
        )
        .is_err());
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
