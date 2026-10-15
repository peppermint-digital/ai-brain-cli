//! Die laufende Rückmeldung der Maske (#7832).
//!
//! **Das Problem.** Während ein Werkzeug arbeitet, kommt vom Motor nichts —
//! kein Ereignis, kein Zeichen. Ein Befehl mit 60 Sekunden Frist sieht dadurch
//! aus wie ein Hänger, und man drückt Ctrl-C auf etwas, das gerade richtig
//! läuft. Bastians Worte: „Ich sehe aber nicht, was der macht."
//!
//! **Warum ein eigener Faden.** Die Zeit müsste sonst von einem Ereignis
//! fortgeschrieben werden — und genau die kommen während der Arbeit nicht. Ein
//! Faden, der alle 400 ms malt, ist der einzige Weg zu einer Zahl, die sich
//! bewegt, während nichts passiert.
//!
//! **Eine Sperre für Zustand UND Bildschirm.** Zwei Fäden, die auf dieselbe
//! Zeile schreiben, überschreiben sich gegenseitig mitten im Zeichen. Deshalb
//! geht JEDE Ausgabe der Maske durch dieses Modul: der malende Faden und die
//! Hauptschleife teilen dieselbe Sperre, und wer schreibt, löscht vorher die
//! Fortschrittszeile.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Innen {
    /// Was gerade läuft, und seit wann.
    taetigkeit: Option<(String, Instant)>,
    /// Steht gerade eine Fortschrittszeile auf dem Bildschirm?
    gemalt: bool,
    /// Läuft gerade Text des Modells? Dann gehört kein Umbruch davor.
    im_text: bool,
}

pub struct Anzeige {
    innen: Mutex<Innen>,
    schluss: AtomicBool,
}

/// Zeichen für den Takt. Bewusst schlicht — ein Bildschirm, der flackert,
/// zieht mehr Aufmerksamkeit als der Text, um den es geht.
const TAKT: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

impl Anzeige {
    pub fn neu() -> Arc<Self> {
        Arc::new(Self {
            innen: Mutex::new(Innen {
                taetigkeit: None,
                gemalt: false,
                im_text: false,
            }),
            schluss: AtomicBool::new(false),
        })
    }

    /// Den malenden Faden starten. Endet, wenn `beenden` gerufen wurde.
    pub fn takten(anzeige: &Arc<Self>) -> std::thread::JoinHandle<()> {
        let a = Arc::clone(anzeige);
        let mut schritt = 0usize;

        std::thread::spawn(move || {
            while !a.schluss.load(Ordering::Relaxed) {
                a.malen(schritt);
                schritt = schritt.wrapping_add(1);
                std::thread::sleep(Duration::from_millis(400));
            }
        })
    }

    pub fn beenden(&self) {
        self.schluss.store(true, Ordering::Relaxed);
        self.loeschen();
    }

    /// Was gerade passiert. `None` heisst: nichts anzuzeigen.
    ///
    /// **Die Uhr läuft nur bei einer ANDEREN Tätigkeit neu an.** Der Motor
    /// schickt `status: requesting` mehrmals je Zug; wer darauf jedes Mal die
    /// Uhr zurücksetzt, zeigt nie eine Zahl über null. Beim ersten Echttest
    /// stand deshalb fünfzehn Mal „denkt" ohne eine einzige Sekunde daneben —
    /// genau die Angabe, um die es ging.
    pub fn taetigkeit(&self, was: Option<String>) {
        let Ok(mut i) = self.innen.lock() else { return };

        match (was, i.taetigkeit.take()) {
            (Some(neu), Some((alt, seit))) if neu == alt => {
                // Unverändert: Uhr weiterlaufen lassen.
                i.taetigkeit = Some((alt, seit));
            }
            (Some(neu), _) => {
                i.taetigkeit = Some((neu, Instant::now()));
            }
            (None, _) => {
                i.taetigkeit = None;
                Self::zeile_loeschen(&mut i);
            }
        }
    }

    /// Ein Textstück des Modells.
    pub fn text(&self, stueck: &str) {
        let Ok(mut i) = self.innen.lock() else { return };

        Self::zeile_loeschen(&mut i);

        if !i.im_text {
            println!();
            i.im_text = true;
        }

        print!("{stueck}");
        let _ = std::io::stdout().flush();
    }

    /// Eine eigene Zeile der Maske (Werkzeuggriff, Hinweis, Fehler).
    pub fn zeile(&self, text: &str) {
        let Ok(mut i) = self.innen.lock() else { return };

        Self::zeile_loeschen(&mut i);

        if i.im_text {
            println!();
            i.im_text = false;
        }

        println!("{text}");
        let _ = std::io::stdout().flush();
    }

    /// Den Textfluss abschliessen, ohne etwas zu drucken.
    pub fn absatz(&self) {
        let Ok(mut i) = self.innen.lock() else { return };

        Self::zeile_loeschen(&mut i);

        if i.im_text {
            println!();
            i.im_text = false;
        }
    }

    fn loeschen(&self) {
        if let Ok(mut i) = self.innen.lock() {
            Self::zeile_loeschen(&mut i);
        }
    }

    fn zeile_loeschen(i: &mut Innen) {
        if i.gemalt {
            // Wagenrücklauf und Zeilenrest löschen — nicht `\n`, sonst
            // sammelt sich die Fortschrittszeile im Verlauf.
            print!("\r\x1b[K");
            let _ = std::io::stdout().flush();
            i.gemalt = false;
        }
    }

    fn malen(&self, schritt: usize) {
        let Ok(mut i) = self.innen.lock() else { return };

        let Some((was, seit)) = i.taetigkeit.clone() else {
            return;
        };

        // Während Text fliesst, nicht malen: der Satz des Modells wäre sonst
        // von einem Rad unterbrochen, das nichts hinzufügt.
        if i.im_text {
            return;
        }

        let sekunden = seit.elapsed().as_secs();
        let rad = TAKT[schritt % TAKT.len()];

        print!("\r\x1b[K  \x1b[2m{rad} {was}");

        // Die Zahl erst ab zwei Sekunden: bei allem darunter ist sie Zappeln.
        if sekunden >= 2 {
            print!(" · {}", dauer(sekunden));
        }

        print!("\x1b[0m");
        let _ = std::io::stdout().flush();
        i.gemalt = true;
    }
}

/// Sekunden lesbar. Über eine Minute zählt niemand mehr in Sekunden.
pub fn dauer(sekunden: u64) -> String {
    if sekunden < 60 {
        return format!("{sekunden}s");
    }

    format!("{}:{:02}", sekunden / 60, sekunden % 60)
}

/// Den Namen eines Werkzeugs so, wie ein Mensch ihn nennt.
///
/// `mcp__rechner__rechner-dienste` ist die Adresse, nicht der Name. Der
/// Mittelteil ist der Server, und den kennt man schon — er steht im Schirm.
/// Beim ersten Echttest stand `rechner__rechner-datentraeger` auf dem
/// Bildschirm: zweimal dasselbe Wort, weil nur `mcp__` abgeschnitten wurde.
pub fn werkzeugname(roh: &str) -> String {
    let ohne = roh.strip_prefix("mcp__").unwrap_or(roh);

    // Nach dem Server folgt der eigentliche Name. Gibt es kein `__`, ist es
    // ein eingebautes Werkzeug (Bash, Read) — das bleibt, wie es heisst.
    match ohne.split_once("__") {
        Some((_server, name)) => name.to_string(),
        None => ohne.to_string(),
    }
}

/// Argumente eines Werkzeugs auf eine Zeile bringen.
///
/// **Warum überhaupt.** `⚒ rechner-befehl` sagt, dass etwas passiert, aber
/// nicht was. `⚒ rechner-befehl  systemctl is-active reverb` sagt es. Wer
/// einem Programm die Hoheit über seinen Rechner gibt, soll sehen, womit es
/// sie benutzt.
pub fn argumente_kurz(roh: &str, grenze: usize) -> String {
    let Ok(wert) = serde_json::from_str::<serde_json::Value>(roh) else {
        // Unvollständiges JSON ist der Normalfall, solange die Argumente noch
        // strömen — dann eben der Rohtext, ohne Klammern am Anfang.
        return kurz(roh.trim_start_matches('{').trim(), grenze);
    };

    let Some(objekt) = wert.as_object() else {
        return kurz(&wert.to_string(), grenze);
    };

    // Der aussagekräftigste Wert zuerst: ein Befehl oder ein Pfad sagt mehr
    // als die Aufzählung aller Schlüssel.
    for name in ["befehl", "inhalt", "text", "query", "pfad", "file_path", "command"] {
        if let Some(v) = objekt.get(name).and_then(|v| v.as_str()) {
            return kurz(v, grenze);
        }
    }

    let teile: Vec<String> = objekt
        .iter()
        .map(|(k, v)| match v.as_str() {
            Some(s) => format!("{k}={s}"),
            None => format!("{k}={v}"),
        })
        .collect();

    kurz(&teile.join(" "), grenze)
}

fn kurz(text: &str, grenze: usize) -> String {
    let eine_zeile: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c })
        .collect();
    let gestrafft = eine_zeile.split_whitespace().collect::<Vec<_>>().join(" ");
    let zeichen: Vec<char> = gestrafft.chars().collect();

    if zeichen.len() <= grenze {
        return gestrafft;
    }

    format!("{}…", zeichen[..grenze.saturating_sub(1)].iter().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vom_werkzeugnamen_bleibt_der_name() {
        assert_eq!(werkzeugname("mcp__rechner__rechner-dienste"), "rechner-dienste");
        assert_eq!(werkzeugname("mcp__ai-brain__list-tasks-tool"), "list-tasks-tool");
        // Eingebaute Werkzeuge heissen, wie sie heissen.
        assert_eq!(werkzeugname("Bash"), "Bash");
        assert_eq!(werkzeugname("Read"), "Read");
    }

    #[test]
    fn die_uhr_laeuft_bei_gleicher_taetigkeit_weiter() {
        // Der Motor schickt `status: requesting` mehrmals je Zug. Wer darauf
        // jedes Mal die Uhr zuruecksetzt, zeigt nie eine Zahl ueber null.
        let a = Anzeige::neu();

        a.taetigkeit(Some("denkt".into()));
        let erste = a.innen.lock().unwrap().taetigkeit.as_ref().unwrap().1;

        std::thread::sleep(Duration::from_millis(20));
        a.taetigkeit(Some("denkt".into()));
        let zweite = a.innen.lock().unwrap().taetigkeit.as_ref().unwrap().1;

        assert_eq!(erste, zweite, "gleiche Taetigkeit darf die Uhr nicht zuruecksetzen");
    }

    #[test]
    fn eine_andere_taetigkeit_setzt_die_uhr_zurueck() {
        let a = Anzeige::neu();

        a.taetigkeit(Some("denkt".into()));
        let erste = a.innen.lock().unwrap().taetigkeit.as_ref().unwrap().1;

        std::thread::sleep(Duration::from_millis(20));
        a.taetigkeit(Some("rechner-befehl arbeitet".into()));
        let zweite = a.innen.lock().unwrap().taetigkeit.as_ref().unwrap().1;

        assert!(zweite > erste, "neue Taetigkeit braucht eine neue Uhr");
    }

    #[test]
    fn dauer_zaehlt_ab_einer_minute_in_minuten() {
        assert_eq!(dauer(5), "5s");
        assert_eq!(dauer(59), "59s");
        assert_eq!(dauer(60), "1:00");
        assert_eq!(dauer(125), "2:05");
    }

    #[test]
    fn der_aussagekraeftigste_wert_kommt_zuerst() {
        let roh = r#"{"frist":60,"befehl":"systemctl is-active reverb"}"#;

        assert_eq!(argumente_kurz(roh, 60), "systemctl is-active reverb");
    }

    #[test]
    fn sonst_werden_die_schluessel_aufgezaehlt() {
        let roh = r#"{"dienste":["reverb.service"]}"#;

        assert_eq!(argumente_kurz(roh, 60), r#"dienste=["reverb.service"]"#);
    }

    #[test]
    fn unvollstaendiges_json_ergibt_trotzdem_etwas() {
        // Der Normalfall, solange die Argumente noch stroemen.
        let aus = argumente_kurz("{\"query\": \"select:mc", 60);

        assert!(aus.contains("query"), "{aus}");
        assert!(!aus.starts_with('{'), "{aus}");
    }

    #[test]
    fn mehrzeiliges_wird_zu_einer_zeile() {
        let roh = r#"{"befehl":"zeile eins\nzeile zwei"}"#;
        let aus = argumente_kurz(roh, 60);

        assert!(!aus.contains('\n'), "{aus}");
        assert_eq!(aus, "zeile eins zeile zwei");
    }

    #[test]
    fn gekuerzt_wird_auf_zeichen_nicht_auf_bytes() {
        // `len()` zaehlt Bytes: ein Schnitt mitten in einem Umlaut ergaebe
        // ungueltiges UTF-8.
        let roh = format!(r#"{{"befehl":"{}"}}"#, "ä".repeat(50));
        let aus = argumente_kurz(&roh, 10);

        assert_eq!(aus.chars().count(), 10);
        assert!(aus.ends_with('…'));
    }
}
