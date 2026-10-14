//! Das Lauf-Register (#7665).
//!
//! Mehrere Agentenläufe gleichzeitig auf einem Rechner — und eine Stelle, die
//! Auskunft darüber gibt, welche laufen, was sie tun und was sie getan haben.
//! Das ist der Teil, den Hermes mit `max_live_sessions: 16` abgedeckt hat.
//!
//! **Dateien, keine Datenbank.** Ein Lauf ist ein Ordner mit `lauf.json` und
//! `protokoll.txt`. Das hält die CLI bei fünf Abhängigkeiten; SQLite dafür
//! einzuziehen wäre ein schlechter Tausch. Die Liste entsteht, indem die Ordner
//! gelesen werden — bei einer zweistelligen Zahl paralleler Läufe ist das
//! nicht messbar.
//!
//! **Die Kennung ist `<sekunden>-<prozess>`, nicht „höchste + 1“.** Mehrere
//! Läufe starten gleichzeitig, und „die höchste Nummer plus eins“ ist dann ein
//! Rennen: Zwei Prozesse lesen dieselbe Zahl und schreiben in denselben Ordner.
//!
//! Hier stand zuerst „Sekunde plus Prozessnummer braucht keine Absprache“. Das
//! war falsch, und der erste Testlauf hat es gezeigt: Startet **ein** Prozess
//! zwei Läufe in derselben Sekunde, ist die Kennung identisch und der zweite
//! überschreibt den ersten. Die Absprache ist jetzt das Dateisystem — der
//! Ordner wird mit `create_dir` beansprucht, das bei Vorhandensein scheitert,
//! und bei Kollision zählt ein Zusatz hoch. Das ist atomar und gilt auch
//! zwischen Prozessen.
//!
//! **Woran man einen verschwundenen Lauf erkennt: am Herzschlag, nicht am
//! Prozess.** Nach `kill -0` zu fragen braucht entweder eine weitere
//! Abhängigkeit oder `/proc`, das es unter macOS nicht gibt. Stattdessen
//! stempelt der Aufseher `zuletzt_gesehen`; bleibt der Stempel stehen, ist er
//! weg. Das erfasst zusätzlich den Fall, den eine Prozessabfrage übersieht: Der
//! Prozess lebt, tut aber nichts mehr.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Wie lange ein Herzschlag ausbleiben darf, bevor ein Lauf als verschwunden
/// gilt. Grosszügig gewählt: Ein Agent, der eine Minute an einem Werkzeugaufruf
/// hängt, ist nicht tot. Falsch „verschwunden“ zu melden wäre schlimmer als
/// eine Minute später Bescheid zu geben.
const HERZSCHLAG_TOLERANZ: u64 = 180;

/// Nach wie vielen Tagen ein abgeschlossener Lauf samt Protokoll verschwindet.
/// Laufende Läufe sind davon nie betroffen, egal wie alt.
const AUFBEWAHRUNG_TAGE: u64 = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Zustand {
    /// Arbeitet.
    Laeuft,
    /// Wartet auf eine Antwort — eine Rückfrage des Agenten.
    Wartet,
    /// Fertig, ohne Fehler.
    Fertig,
    /// Abgebrochen, abgestürzt oder verschwunden. `grund` sagt, was.
    Fehler,
}

impl Zustand {
    pub fn zeichen(self) -> &'static str {
        match self {
            Zustand::Laeuft => "●",
            Zustand::Wartet => "?",
            Zustand::Fertig => "✓",
            Zustand::Fehler => "✗",
        }
    }

    pub fn wort(self) -> &'static str {
        match self {
            Zustand::Laeuft => "laeuft",
            Zustand::Wartet => "wartet",
            Zustand::Fertig => "fertig",
            Zustand::Fehler => "fehler",
        }
    }

    pub fn abgeschlossen(self) -> bool {
        matches!(self, Zustand::Fertig | Zustand::Fehler)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lauf {
    pub id: String,
    pub zustand: Zustand,

    /// Was gestartet wurde — für die Anzeige und fürs Wiederfinden.
    pub modell: String,
    pub verzeichnis: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projekt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auftrag: Option<String>,

    /// Der beaufsichtigte Prozess. Für `stop`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,

    /// Adresse des imkr-Panes, falls der Lauf dort liegt (#7667). Steht hier,
    /// damit `logs` und `stop` beide Wege bedienen können, ohne zu raten.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<String>,

    /// Hat imkr dieses Pane je als arbeitend gesehen?
    ///
    /// Gefunden am 14.10.2026 bei der ersten echten Probe: Claude Code fragt in
    /// einem unbekannten Verzeichnis zuerst nach Vertrauen. Das Pane hängt dann
    /// an einem Dialog, imkr meldet `done` — und daraus wurde `fertig`, obwohl
    /// nie etwas gelaufen ist. Ein stiller Falschbefund, und der schlimmste:
    /// Er sieht wie ein Erfolg aus.
    ///
    /// Also gilt `ruht` erst als „fertig", wenn es vorher `arbeitet` war.
    #[serde(default)]
    pub jemals_gearbeitet: bool,

    /// Kennung der Unterhaltung, mit der dieser Lauf fortgesetzt werden kann
    /// (`agent --sitzung <id>`). Bei Claude eine vorgegebene UUID, bei codex
    /// leer — dort vergibt das Werkzeug sie selbst.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sitzung: Option<String>,

    pub gestartet: u64,
    pub zuletzt_gesehen: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beendet: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grund: Option<String>,
}

impl Lauf {
    /// Laufzeit in Sekunden — bis zum Ende, oder bis jetzt.
    pub fn dauer(&self) -> u64 {
        self.beendet.unwrap_or_else(jetzt).saturating_sub(self.gestartet)
    }
}

/// Noch kein Aufrufer ausserhalb der Tests: Läufe **erzeugt** der
/// `agent`-Befehl (#7666), und der kommt als nächstes. Die Markierung steht
/// hier statt als Modul-Freigabe, damit sie mit dem Aufrufer verschwindet und
/// nicht dauerhaft echte Funde verdeckt.
///
/// Angaben, die beim Anlegen bekannt sind. Alles andere setzt das Register.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct NeuerLauf {
    pub modell: String,
    pub verzeichnis: String,
    pub projekt: Option<String>,
    pub task: Option<u64>,
    pub auftrag: Option<String>,
    pub pid: Option<u32>,
    pub pane: Option<String>,
}

pub fn jetzt() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Wo die Läufe liegen.
///
/// Zustand gehört nicht in die Konfigurationsablage: Die wird gesichert und
/// zieht bei einer Umbenennung mit um (siehe `config::uebernimm_alte_ablage`).
/// Protokolle laufender Agenten haben dort nichts zu suchen — deshalb
/// `XDG_STATE_HOME`, und unter Windows `%LOCALAPPDATA%`.
pub fn wurzel() -> Result<PathBuf, String> {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(state).join("ai-brain-cli").join("laeufe"));
    }

    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return Ok(PathBuf::from(local).join("ai-brain-cli").join("laeufe"));
        }
    }

    Ok(crate::config::heimat()?
        .join(".local")
        .join("state")
        .join("ai-brain-cli")
        .join("laeufe"))
}

fn ordner(id: &str) -> Result<PathBuf, String> {
    // Keine Kennung mit Pfadtrennern oder `..` — sonst wäre `logs ../../x` ein
    // Weg, beliebige Dateien ausgeben zu lassen. Die Kennungen kommen aus
    // unserer eigenen Vergabe, aber sie kommen über die Kommandozeile zurück.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!("„{id}“ ist keine gültige Lauf-Kennung."));
    }

    Ok(wurzel()?.join(id))
}

pub fn protokoll_pfad(id: &str) -> Result<PathBuf, String> {
    Ok(ordner(id)?.join("protokoll.txt"))
}

fn lauf_pfad(id: &str) -> Result<PathBuf, String> {
    Ok(ordner(id)?.join("lauf.json"))
}

#[allow(dead_code)] // Aufrufer: `agent` (#7666)
pub fn anlegen(neu: NeuerLauf) -> Result<Lauf, String> {
    let nun = jetzt();
    let (id, _dir) = kennung_beanspruchen(nun)?;

    let lauf = Lauf {
        id: id.clone(),
        zustand: Zustand::Laeuft,
        modell: neu.modell,
        verzeichnis: neu.verzeichnis,
        projekt: neu.projekt,
        task: neu.task,
        auftrag: neu.auftrag,
        pid: neu.pid,
        pane: neu.pane,
        gestartet: nun,
        zuletzt_gesehen: nun,
        beendet: None,
        exit_code: None,
        grund: None,
        jemals_gearbeitet: false,
        sitzung: None,
    };

    schreiben(&lauf)?;

    Ok(lauf)
}

/// Beansprucht eine freie Kennung, indem der Ordner angelegt wird.
///
/// `create_dir` scheitert, wenn es ihn schon gibt — damit ist der Anspruch
/// atomar, ohne Sperrdatei und ohne Absprache zwischen Prozessen. Bei Kollision
/// zählt ein Zusatz hoch: `1760440000-12345`, dann `-2`, `-3`.
fn kennung_beanspruchen(nun: u64) -> Result<(String, PathBuf), String> {
    let wurzel = wurzel()?;
    fs::create_dir_all(&wurzel)
        .map_err(|e| format!("{} lässt sich nicht anlegen: {e}", wurzel.display()))?;

    let basis = format!("{nun}-{}", std::process::id());

    for versuch in 1..=1000 {
        let id = if versuch == 1 {
            basis.clone()
        } else {
            format!("{basis}-{versuch}")
        };

        let dir = ordner(&id)?;

        match fs::create_dir(&dir) {
            Ok(()) => return Ok((id, dir)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{} lässt sich nicht anlegen: {e}", dir.display())),
        }
    }

    Err("Keine freie Lauf-Kennung in dieser Sekunde gefunden.".to_string())
}

pub fn schreiben(lauf: &Lauf) -> Result<(), String> {
    let pfad = lauf_pfad(&lauf.id)?;
    let json = serde_json::to_string_pretty(lauf).map_err(|e| format!("{e}"))?;

    fs::write(&pfad, json).map_err(|e| format!("{} lässt sich nicht schreiben: {e}", pfad.display()))
}

pub fn lesen(id: &str) -> Result<Lauf, String> {
    let pfad = lauf_pfad(id)?;
    let roh = fs::read_to_string(&pfad).map_err(|_| format!("Lauf „{id}“ gibt es hier nicht."))?;

    serde_json::from_str(&roh).map_err(|e| format!("{} ist beschädigt: {e}", pfad.display()))
}

/// Lebenszeichen des Aufsehers. Ohne das gilt ein Lauf nach
/// `HERZSCHLAG_TOLERANZ` als verschwunden. Aufrufer: `agent` (#7666).
#[allow(dead_code)]
pub fn herzschlag(id: &str) -> Result<(), String> {
    let mut lauf = lesen(id)?;
    lauf.zuletzt_gesehen = jetzt();
    schreiben(&lauf)
}

pub fn zustand_setzen(id: &str, zustand: Zustand) -> Result<(), String> {
    let mut lauf = lesen(id)?;
    lauf.zustand = zustand;
    lauf.zuletzt_gesehen = jetzt();

    if zustand.abgeschlossen() && lauf.beendet.is_none() {
        lauf.beendet = Some(jetzt());
    }

    schreiben(&lauf)
}

pub fn abschliessen(id: &str, exit_code: Option<i32>, grund: Option<String>) -> Result<(), String> {
    let mut lauf = lesen(id)?;
    let nun = jetzt();

    lauf.zustand = if exit_code == Some(0) {
        Zustand::Fertig
    } else {
        Zustand::Fehler
    };
    lauf.exit_code = exit_code;
    lauf.grund = grund;
    lauf.beendet = Some(nun);
    lauf.zuletzt_gesehen = nun;

    schreiben(&lauf)
}

/// Alle Läufe dieses Rechners, neueste zuerst.
///
/// Räumt dabei zwei Dinge auf, und zwar **schreibend**: verschwundene Läufe
/// werden auf `fehler` gesetzt, zu alte abgeschlossene gelöscht. Dass ein
/// Lesebefehl schreibt, ist Absicht — würde die Liste den Zustand nur in der
/// Anzeige herleiten, bliebe in der Datei für immer „laeuft“ stehen, und jeder
/// andere Leser (das imkr-Plugin, AI Brain) müsste dieselbe Herleitung
/// nachbauen. Ein Zustand, den nur ein Weg kennt, ist keiner.
pub fn liste() -> Result<Vec<Lauf>, String> {
    let wurzel = wurzel()?;

    if !wurzel.exists() {
        return Ok(Vec::new());
    }

    let eintraege = fs::read_dir(&wurzel)
        .map_err(|e| format!("{} lässt sich nicht lesen: {e}", wurzel.display()))?;

    let nun = jetzt();
    let mut laeufe = Vec::new();

    for eintrag in eintraege.flatten() {
        let pfad = eintrag.path();

        if !pfad.is_dir() {
            continue;
        }

        let Some(id) = pfad.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        let Ok(mut lauf) = lesen(id) else {
            continue;
        };

        if !lauf.zustand.abgeschlossen() {
            // Ein Lauf in einem imkr-Pane hat keinen Aufseher, also auch keinen
            // Herzschlag. Würde hier die Verschwunden-Regel greifen, wäre jeder
            // Pane-Lauf nach drei Minuten „fehler", obwohl er arbeitet. Also
            // fragen wir den, der es weiss.
            if let Some(adresse) = lauf.pane.clone() {
                let (zustand, grund) = aus_pane_zustand(
                    crate::imkr::zustand(&adresse),
                    lauf.jemals_gearbeitet,
                    &adresse,
                );

                if zustand == Zustand::Laeuft {
                    lauf.jemals_gearbeitet = true;
                }

                if zustand.abgeschlossen() && lauf.beendet.is_none() {
                    lauf.beendet = Some(nun);
                }

                lauf.zustand = zustand;
                lauf.grund = grund;

                lauf.zuletzt_gesehen = nun;
                let _ = schreiben(&lauf);
            } else if nun.saturating_sub(lauf.zuletzt_gesehen) > HERZSCHLAG_TOLERANZ {
                lauf.zustand = Zustand::Fehler;
                lauf.beendet = Some(lauf.zuletzt_gesehen);
                lauf.grund = Some(format!(
                    "Aufseher verschwunden — kein Lebenszeichen seit {} s",
                    nun.saturating_sub(lauf.zuletzt_gesehen)
                ));
                let _ = schreiben(&lauf);
            }
        }

        let zu_alt = lauf
            .beendet
            .is_some_and(|b| nun.saturating_sub(b) > AUFBEWAHRUNG_TAGE * 86_400);

        if lauf.zustand.abgeschlossen() && zu_alt {
            let _ = fs::remove_dir_all(&pfad);
            continue;
        }

        laeufe.push(lauf);
    }

    // Neueste zuerst: absteigend, deshalb Reverse statt sort_by_key.
    laeufe.sort_by_key(|l| std::cmp::Reverse(l.gestartet));

    Ok(laeufe)
}

/// Übersetzt den Pane-Zustand in einen Lauf-Zustand.
///
/// Reine Funktion, damit die **Regel** prüfbar ist, ohne laufendes imkr — und
/// die Regel ist der Punkt: `ruht` heisst nur dann „fertig", wenn das Pane
/// vorher gearbeitet hat.
///
/// Gefunden bei der ersten echten Probe am 14.10.2026: Claude Code fragt in
/// einem unbekannten Verzeichnis zuerst nach Vertrauen. Das Pane hing an dem
/// Dialog, imkr meldete `done`, und das Register sagte `fertig` — ein stiller
/// Falschbefund, und der schlimmste von allen, weil er wie ein Erfolg aussieht.
pub fn aus_pane_zustand(
    pane: crate::imkr::PaneZustand,
    jemals_gearbeitet: bool,
    adresse: &str,
) -> (Zustand, Option<String>) {
    use crate::imkr::PaneZustand;

    match pane {
        PaneZustand::Arbeitet => (Zustand::Laeuft, None),
        PaneZustand::Wartet => (Zustand::Wartet, None),
        PaneZustand::Ruht if jemals_gearbeitet => (Zustand::Fertig, None),
        PaneZustand::Ruht => (
            Zustand::Wartet,
            Some(format!(
                "Pane {adresse} hat nie gearbeitet — sieh nach, ob dort etwas fragt: imkr read {adresse}"
            )),
        ),
        PaneZustand::Weg => (
            Zustand::Fehler,
            Some(format!("Pane {adresse} gibt es nicht mehr")),
        ),
    }
}

/// Beendet den Prozess eines Laufs.
///
/// Über `kill` bzw. `taskkill` statt über eine Bibliothek: Die CLI kommt mit
/// fünf Abhängigkeiten aus, und für einen Aufruf pro `stop` lohnt keine sechste.
/// Erst TERM, nicht KILL — ein Agent soll seine Sitzung noch schliessen können.
pub fn beenden(id: &str) -> Result<String, String> {
    let lauf = lesen(id)?;

    if lauf.zustand.abgeschlossen() {
        return Ok(format!(
            "Lauf {id} ist schon {} — nichts zu beenden.",
            lauf.zustand.wort()
        ));
    }

    let Some(pid) = lauf.pid else {
        // Kein Prozess bekannt (etwa ein Lauf in einem imkr-Pane): Zustand
        // setzen und das ehrlich sagen, statt Erfolg zu melden.
        zustand_setzen(id, Zustand::Fehler)?;

        return Ok(format!(
            "Lauf {id} kennt keinen Prozess{} — als fehler vermerkt, beenden musst du ihn dort.",
            lauf.pane
                .map(|p| format!(" (liegt in Pane {p})"))
                .unwrap_or_default()
        ));
    };

    let ausgang = if cfg!(windows) {
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T"])
            .output()
    } else {
        std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output()
    };

    match ausgang {
        Ok(a) if a.status.success() => {
            abschliessen(id, None, Some("von Hand beendet".to_string()))?;
            Ok(format!("Lauf {id} (Prozess {pid}) beendet."))
        }
        Ok(a) => {
            // Der Prozess war schon weg. Kein Fehlschlag — das Ergebnis ist
            // genau das gewünschte, also auch so vermerken.
            abschliessen(id, None, Some("Prozess war bereits beendet".to_string()))?;
            Ok(format!(
                "Prozess {pid} gab es nicht mehr ({}) — Lauf {id} als beendet vermerkt.",
                String::from_utf8_lossy(&a.stderr).trim()
            ))
        }
        Err(e) => Err(format!("Prozess {pid} liess sich nicht beenden: {e}")),
    }
}

/// `agents` — was läuft hier.
pub fn anzeigen(args: &[String]) -> Result<(), String> {
    let laeufe = liste()?;

    if args.iter().any(|a| a == "--json") {
        let json = serde_json::to_string_pretty(&laeufe).map_err(|e| format!("{e}"))?;
        println!("{json}");

        return Ok(());
    }

    if laeufe.is_empty() {
        println!("Keine Läufe auf diesem Rechner.");
        println!();
        println!("Einen starten:  ai-brain-cli agent --projekt <slug> \"<Auftrag>\"");

        return Ok(());
    }

    for lauf in &laeufe {
        let ziel = lauf
            .task
            .map(|t| format!("#{t}"))
            .or_else(|| lauf.projekt.clone())
            .unwrap_or_else(|| "—".to_string());

        println!(
            "{} {:<9} {:<18} {:<10} {:<8} {}",
            lauf.zustand.zeichen(),
            lauf.zustand.wort(),
            lauf.id,
            lauf.modell,
            ziel,
            dauer_lesbar(lauf.dauer())
        );

        if let Some(grund) = &lauf.grund {
            println!("              {grund}");
        }
    }

    Ok(())
}

/// `logs <id>` — was ein Lauf geschrieben hat.
pub fn protokoll_zeigen(args: &[String]) -> Result<(), String> {
    let Some(id) = args.first() else {
        return Err("Welcher Lauf? `ai-brain-cli logs <kennung>` (Liste: `agents`)".to_string());
    };

    let lauf = lesen(id)?;
    let pfad = protokoll_pfad(id)?;

    let zeilen: usize = args
        .windows(2)
        .find(|f| f[0] == "--zeilen")
        .and_then(|f| f[1].parse().ok())
        .unwrap_or(200);

    println!(
        "{} {} · {} · {} · {}",
        lauf.zustand.zeichen(),
        lauf.id,
        lauf.modell,
        lauf.verzeichnis,
        dauer_lesbar(lauf.dauer())
    );

    if let Some(auftrag) = &lauf.auftrag {
        println!("Auftrag: {auftrag}");
    }

    if let Some(sitzung) = &lauf.sitzung {
        println!("Sitzung: {sitzung}   (weiter mit: ai-brain-cli agent --sitzung {sitzung} \"…\")");
    }

    println!();

    // Ein Pane-Lauf schreibt kein Protokoll — sein Schirm IST das Protokoll.
    if let Some(adresse) = &lauf.pane {
        println!("Pane: {adresse}");
        println!();

        match crate::imkr::schirm(adresse, zeilen) {
            Ok(inhalt) => print!("{inhalt}"),
            Err(e) => println!("(Schirm nicht lesbar: {e})"),
        }

        return Ok(());
    }

    match fs::read_to_string(&pfad) {
        Ok(inhalt) => {
            let alle: Vec<&str> = inhalt.lines().collect();
            let ab = alle.len().saturating_sub(zeilen);

            if ab > 0 {
                let wort = if ab == 1 { "Zeile" } else { "Zeilen" };
                println!("… {ab} {wort} davor ausgelassen — alles mit `--zeilen {}`", alle.len());
            }

            for zeile in &alle[ab..] {
                println!("{zeile}");
            }
        }
        Err(_) => println!("(noch kein Protokoll)"),
    }

    Ok(())
}

/// `stop <id>`
pub fn stoppen(args: &[String]) -> Result<(), String> {
    let Some(id) = args.first() else {
        return Err("Welcher Lauf? `ai-brain-cli stop <kennung>` (Liste: `agents`)".to_string());
    };

    println!("{}", beenden(id)?);

    Ok(())
}

fn dauer_lesbar(sekunden: u64) -> String {
    match sekunden {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s => format!("{}h{:02}", s / 3600, (s % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wie in `config`: EIN Test, weil alles hier an `XDG_STATE_HOME` hängt und
    /// Rust Tests nebenläufig ausführt. Getrennte Tests würden sich die
    /// Variable gegenseitig wegziehen.
    #[test]
    fn register_haelt_zustand_und_erkennt_verschwundene() {
        let sicherung = std::env::var_os("XDG_STATE_HOME");
        let spielwiese =
            std::env::temp_dir().join(format!("ai-brain-cli-register-{}", std::process::id()));
        let _ = fs::remove_dir_all(&spielwiese);
        std::env::set_var("XDG_STATE_HOME", &spielwiese);

        // Leeres Register ist kein Fehler.
        assert!(liste().unwrap().is_empty());

        let lauf = anlegen(NeuerLauf {
            modell: "claude".to_string(),
            verzeichnis: "/tmp".to_string(),
            projekt: Some("ai-brain".to_string()),
            task: Some(7665),
            pid: Some(std::process::id()),
            ..Default::default()
        })
        .unwrap();

        assert_eq!(lauf.zustand, Zustand::Laeuft);
        assert_eq!(liste().unwrap().len(), 1);

        // Zustandswechsel überlebt das Schreiben.
        zustand_setzen(&lauf.id, Zustand::Wartet).unwrap();
        assert_eq!(lesen(&lauf.id).unwrap().zustand, Zustand::Wartet);

        // Ein Lauf ohne Lebenszeichen gilt als verschwunden — und die Liste
        // schreibt das in die Datei, statt es nur anzuzeigen.
        let mut alt = lesen(&lauf.id).unwrap();
        alt.zuletzt_gesehen = jetzt() - (HERZSCHLAG_TOLERANZ + 10);
        schreiben(&alt).unwrap();

        let nach_liste = &liste().unwrap()[0];
        assert_eq!(nach_liste.zustand, Zustand::Fehler);
        assert!(nach_liste.grund.as_deref().unwrap().contains("verschwunden"));
        assert_eq!(
            lesen(&lauf.id).unwrap().zustand,
            Zustand::Fehler,
            "der Befund muss in der Datei stehen, nicht nur in der Anzeige"
        );

        // Ein Herzschlag holt einen Lauf nicht zurück — abgeschlossen bleibt
        // abgeschlossen, sonst würde ein Zombie wieder als „laeuft“ erscheinen.
        herzschlag(&lauf.id).unwrap();
        assert_eq!(lesen(&lauf.id).unwrap().zustand, Zustand::Fehler);

        // Abschliessen mit Exit-Code 0 heisst fertig, alles andere fehler.
        let zweiter = anlegen(NeuerLauf {
            modell: "codex".to_string(),
            verzeichnis: "/tmp".to_string(),
            ..Default::default()
        })
        .unwrap();
        abschliessen(&zweiter.id, Some(0), None).unwrap();
        assert_eq!(lesen(&zweiter.id).unwrap().zustand, Zustand::Fertig);

        let dritter = anlegen(NeuerLauf {
            modell: "codex".to_string(),
            verzeichnis: "/tmp".to_string(),
            ..Default::default()
        })
        .unwrap();
        abschliessen(&dritter.id, Some(2), Some("Zeitgrenze".to_string())).unwrap();
        assert_eq!(lesen(&dritter.id).unwrap().zustand, Zustand::Fehler);

        // Zu alte abgeschlossene Läufe verschwinden, laufende NIE.
        let mut uralt = lesen(&zweiter.id).unwrap();
        uralt.beendet = Some(jetzt() - (AUFBEWAHRUNG_TAGE * 86_400 + 60));
        schreiben(&uralt).unwrap();

        let mut uralt_aber_laeuft = lesen(&dritter.id).unwrap();
        uralt_aber_laeuft.zustand = Zustand::Laeuft;
        uralt_aber_laeuft.beendet = None;
        uralt_aber_laeuft.gestartet = jetzt() - (AUFBEWAHRUNG_TAGE * 86_400 + 60);
        uralt_aber_laeuft.zuletzt_gesehen = jetzt();
        schreiben(&uralt_aber_laeuft).unwrap();

        let ids: Vec<String> = liste().unwrap().into_iter().map(|l| l.id).collect();
        assert!(!ids.contains(&zweiter.id), "alter abgeschlossener Lauf muss weg sein");
        assert!(
            ids.contains(&dritter.id),
            "ein laufender Lauf darf nie wegen Alters verschwinden"
        );

        // Zwei Läufe in DERSELBEN Sekunde aus DEMSELBEN Prozess brauchen
        // verschiedene Kennungen. Beim ersten Wurf hatten sie dieselbe, und der
        // zweite überschrieb den ersten — gefunden, weil der Test oben drei
        // Läufe hintereinander anlegt und der älteste nicht verschwand.
        let a = anlegen(NeuerLauf {
            modell: "claude".to_string(),
            verzeichnis: "/tmp".to_string(),
            ..Default::default()
        })
        .unwrap();
        let b = anlegen(NeuerLauf {
            modell: "claude".to_string(),
            verzeichnis: "/tmp".to_string(),
            ..Default::default()
        })
        .unwrap();

        assert_ne!(a.id, b.id, "zwei Läufe in einer Sekunde brauchen zwei Kennungen");
        assert!(lesen(&a.id).is_ok(), "der erste Lauf darf nicht überschrieben sein");
        assert!(lesen(&b.id).is_ok());

        // Die Regel für Pane-Läufe: `ruht` heisst nur dann fertig, wenn vorher
        // gearbeitet wurde. Sonst hing das Pane an einem Dialog und das
        // Register meldete einen Erfolg, den es nie gab.
        use crate::imkr::PaneZustand;

        assert_eq!(
            aus_pane_zustand(PaneZustand::Ruht, true, "s/1").0,
            Zustand::Fertig
        );

        let (zustand, grund) = aus_pane_zustand(PaneZustand::Ruht, false, "s/1");
        assert_eq!(
            zustand,
            Zustand::Wartet,
            "ohne vorherige Arbeit darf `ruht` nicht als fertig gelten"
        );
        assert!(grund.unwrap().contains("nie gearbeitet"));

        assert_eq!(
            aus_pane_zustand(PaneZustand::Arbeitet, false, "s/1").0,
            Zustand::Laeuft
        );
        assert_eq!(
            aus_pane_zustand(PaneZustand::Wartet, true, "s/1").0,
            Zustand::Wartet
        );

        let (zustand, grund) = aus_pane_zustand(PaneZustand::Weg, true, "s/1");
        assert_eq!(zustand, Zustand::Fehler);
        assert!(grund.unwrap().contains("gibt es nicht mehr"));

        // Pfad-Einschleusung über die Kennung muss scheitern, nicht lesen.
        assert!(lesen("../../etc/passwd").is_err());
        assert!(lesen("").is_err());

        let _ = fs::remove_dir_all(&spielwiese);
        match sicherung {
            Some(v) => std::env::set_var("XDG_STATE_HOME", v),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }
    }
}
