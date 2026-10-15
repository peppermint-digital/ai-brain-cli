//! Daueraufträge dieses Rechners (#7814).
//!
//! **Der Abholer ist das fehlende Stück.** AI Brain kennt die Daueraufträge und
//! die Fristen, aber es kann auf diesem Rechner nichts ausführen. Dieses Modul
//! holt, was fällig ist, fährt es und meldet zurück.
//!
//! ## Ohne Modell, und das ist der Punkt
//!
//! Hermes' beide Wächter waren `no_agent: true` — Skripte auf einem Taktgeber.
//! Das war richtig: „Ist der jüngste Snapshot jünger als 26 Stunden" ist eine
//! Schwellenfrage, keine Denkaufgabe. Ein Modell pro Durchlauf kostet Geld für
//! eine Antwort, die feststeht. `art: befehl` fährt deshalb durch die
//! Werkzeugschicht aus `rechner.rs`, ohne dass ein Motor startet.
//!
//! `art: auftrag` ist der andere Fall — dafür gibt es den Motor, und dafür
//! ruft dieses Modul `agent`.
//!
//! ## Gemeldet wird immer
//!
//! Auch der Lauf, bei dem alles in Ordnung war. Das ist nicht Höflichkeit: Aus
//! den Meldungen ergibt sich, ob die Überwachung selbst noch läuft. Ein
//! Dauerauftrag, der nur bei Problemen meldet, kann seinen eigenen Ausfall
//! nicht anzeigen — und Schweigen sähe dann aus wie Ruhe.

use serde::Deserialize;

use crate::config::{self, Connection};
use crate::mcp;

#[derive(Debug, Deserialize)]
struct Antwort {
    #[serde(default)]
    auftraege: Vec<Dauerauftrag>,
}

#[derive(Debug, Deserialize)]
pub struct Dauerauftrag {
    pub id: u64,
    pub name: String,
    pub art: String,
    pub inhalt: String,
    #[serde(default)]
    pub aktiv: bool,
    #[serde(default)]
    pub takt_minuten: Option<u64>,
    #[serde(default)]
    pub cron: Option<String>,
    #[serde(default)]
    pub naechster_lauf: Option<String>,
    #[serde(default)]
    pub letzte_meldung: Option<String>,
    #[serde(default)]
    pub letzter_status: Option<String>,
    #[serde(default)]
    pub verstummt: bool,
}

const ART_BEFEHL: &str = "befehl";

/// Die Frist, nach der ein Befehl abgebrochen wird.
///
/// Fünf Minuten sind für eine Prüfung reichlich und für einen Taktgeber die
/// Obergrenze: Ein Befehl, der länger hängt, hält den nächsten Lauf auf und
/// verschiebt damit die eigene Meldung in die Stillfrist hinein.
const FRIST_SEKUNDEN: u64 = 300;

fn holen(connection: &Connection, nur_faellige: bool) -> Result<Vec<Dauerauftrag>, String> {
    let token = mcp::access_token(connection)?;

    let roh = mcp::call_tool(
        connection,
        &token,
        "list-device-jobs-tool",
        serde_json::json!({ "nur_faellige": nur_faellige, "json": true }),
    )?;

    let antwort: Antwort = serde_json::from_value(roh)
        .map_err(|e| format!("Antwort auf list-device-jobs-tool ist unlesbar: {e}"))?;

    Ok(antwort.auftraege)
}

fn melden(
    connection: &Connection,
    id: u64,
    status: &str,
    antwort: &str,
) -> Result<(), String> {
    let token = mcp::access_token(connection)?;

    mcp::call_tool_text(
        connection,
        &token,
        "report-device-job-tool",
        serde_json::json!({
            "id": id,
            "status": status,
            // Lang genug, um das Messergebnis zu tragen, kurz genug, um eine
            // Spalte nicht zu sprengen. Ein Befehl, der Seiten ausgibt, hat
            // sein Ergebnis am Ende — deshalb der Schwanz, nicht der Kopf.
            "antwort": schwanz(antwort, 2000),
        }),
    )?;

    Ok(())
}

/// Die letzten `grenze` Zeichen, auf einer Zeichengrenze.
///
/// `len()` zählt Bytes, nicht Zeichen — ein Schnitt mitten in einem Umlaut
/// ergibt ungültiges UTF-8 und die Meldung geht gar nicht durch.
pub fn schwanz(text: &str, grenze: usize) -> String {
    let zeichen: Vec<char> = text.trim().chars().collect();

    if zeichen.len() <= grenze {
        return zeichen.iter().collect();
    }

    let ab = zeichen.len() - grenze;

    format!("…{}", zeichen[ab..].iter().collect::<String>())
}

/// `daueraufgaben` — was ist für diesen Rechner eingerichtet?
pub fn anzeigen(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "--fahren") {
        return fahren(args);
    }

    let connection = config::load()?;
    let auftraege = holen(&connection, false)?;

    if args.iter().any(|a| a == "--json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "auftraege": auftraege.iter().map(|a| serde_json::json!({
                    "id": a.id,
                    "name": a.name,
                    "art": a.art,
                    "aktiv": a.aktiv,
                    "verstummt": a.verstummt,
                })).collect::<Vec<_>>()
            }))
            .map_err(|e| format!("{e}"))?
        );

        return Ok(());
    }

    if auftraege.is_empty() {
        println!("Für diesen Rechner ist kein Dauerauftrag eingerichtet.");
        println!();
        println!("Einen anlegen lässt sich im Gespräch — sag der Maske, was überwacht");
        println!("werden soll. Sie nutzt dafür `create-device-job-tool`.");

        return Ok(());
    }

    println!("{} Dauerauftrag/Daueraufträge für diesen Rechner:", auftraege.len());
    println!();
    println!(
        "  \x1b[2m{:<5} {:<30} {:<10} {:<8} {:<13} Zustand\x1b[0m",
        "#", "Name", "Takt", "Art", "Letzte Meld."
    );

    for a in &auftraege {
        let takt = match (&a.cron, a.takt_minuten) {
            (Some(c), _) if !c.is_empty() => c.clone(),
            (_, Some(m)) => format!("{m} min"),
            _ => "—".to_string(),
        };

        let zustand = if !a.aktiv {
            "\x1b[2mABGESCHALTET\x1b[0m".to_string()
        } else if a.verstummt {
            "\x1b[31mVERSTUMMT\x1b[0m".to_string()
        } else {
            a.letzter_status.clone().unwrap_or_else(|| "—".to_string())
        };

        println!(
            "  #{:<4} {:<30} {:<10} {:<8} {:<13} {}",
            a.id,
            kurz(&a.name, 30),
            takt,
            a.art,
            // Wann zuletzt etwas kam, ist die Zahl, auf die es ankommt: die
            // Stillfrist rechnet von hier.
            crate::auftrag::seit(a.letzte_meldung.as_deref()),
            zustand
        );
    }

    println!();

    if let Some(naechster) = auftraege
        .iter()
        .filter(|a| a.aktiv)
        .filter_map(|a| a.naechster_lauf.as_deref())
        .min()
    {
        println!("Nächster Lauf:  {}", crate::auftrag::seit(Some(naechster)));
    }

    println!("Jetzt Fälliges abarbeiten:  ai-brain-cli daueraufgaben --fahren");

    Ok(())
}

fn kurz(text: &str, grenze: usize) -> String {
    let zeichen: Vec<char> = text.chars().collect();

    if zeichen.len() <= grenze {
        return text.to_string();
    }

    format!("{}…", zeichen[..grenze.saturating_sub(1)].iter().collect::<String>())
}

/// `daueraufgaben --fahren` — holen, fahren, melden.
///
/// Dafür gedacht, in einem Taktgeber zu laufen (systemd-Timer). Jeder Lauf
/// meldet, auch der erfolgreiche — siehe Modulkopf.
pub fn fahren(args: &[String]) -> Result<(), String> {
    let still = args.iter().any(|a| a == "--still");
    let connection = config::load()?;
    let faellige = holen(&connection, true)?;

    if faellige.is_empty() {
        if !still {
            println!("Nichts fällig.");
        }

        return Ok(());
    }

    let mut gefahren = 0;
    let mut gescheitert = 0;

    for auftrag in &faellige {
        if !auftrag.aktiv {
            continue;
        }

        if auftrag.art != ART_BEFEHL {
            // `art: auftrag` braucht einen Motor. Den startet dieser Weg
            // bewusst NICHT: ein Taktgeber, der unbeaufsichtigt Modelle
            // anwirft, ist eine Kostenquelle ohne Aufsicht. Das gehört in
            // einen eigenen Schritt, mit eigener Entscheidung.
            if !still {
                println!(
                    "  #{} {} — Art `{}` braucht einen Motor, hier übersprungen.",
                    auftrag.id, auftrag.name, auftrag.art
                );
            }

            continue;
        }

        let ergebnis = crate::rechner::fuer_maske(
            "rechner-befehl",
            &serde_json::json!({ "befehl": auftrag.inhalt, "frist": FRIST_SEKUNDEN }),
        );

        let (status, text) = match ergebnis {
            Ok(aus) => ("ok", aus),
            // Ein Befehl, der mit einem Rückgabewert ungleich null endet, ist
            // bei einer Prüfung die NORMALE Art, „nicht in Ordnung" zu sagen.
            // Das ist `fehler` für den Dauerauftrag — und nicht ein Fehler des
            // Harness.
            Err(e) => ("fehler", e),
        };

        if !still {
            println!("  #{} {} → {}", auftrag.id, auftrag.name, status);
        }

        match melden(&connection, auftrag.id, status, &text) {
            Ok(()) => gefahren += 1,
            Err(e) => {
                gescheitert += 1;
                eprintln!("  #{} {}: Meldung nicht angekommen — {e}", auftrag.id, auftrag.name);
            }
        }
    }

    if !still || gescheitert > 0 {
        println!();
        println!("{gefahren} gemeldet, {gescheitert} ohne Meldung.");
    }

    // **Eine nicht angekommene Meldung ist ein Fehlschlag des Laufs.** Sonst
    // meldet der Taktgeber Erfolg, während AI Brain den Auftrag für verstummt
    // hält — und man sucht den Fehler auf der falschen Seite.
    if gescheitert > 0 {
        return Err(format!(
            "{gescheitert} Meldung(en) sind nicht angekommen. AI Brain hält diese Daueraufträge \
             weiter für offen und schlägt nach ihrer Stillfrist Alarm."
        ));
    }

    Ok(())
}

/// Was die Maske unter `/daueraufgaben` zeigt.
pub fn fuer_maske() -> Result<String, String> {
    let connection = config::load()?;
    let auftraege = holen(&connection, false)?;

    if auftraege.is_empty() {
        return Ok("Für diesen Rechner ist kein Dauerauftrag eingerichtet.".to_string());
    }

    let mut zeilen = Vec::new();

    for a in &auftraege {
        let zustand = if !a.aktiv {
            "abgeschaltet"
        } else if a.verstummt {
            "VERSTUMMT"
        } else {
            a.letzter_status.as_deref().unwrap_or("—")
        };

        zeilen.push(format!(
            "  #{:<4} {:<30} {:<8} {:<13} {}",
            a.id,
            kurz(&a.name, 30),
            a.art,
            crate::auftrag::seit(a.letzte_meldung.as_deref()),
            zustand
        ));
    }

    Ok(zeilen.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn der_schwanz_schneidet_auf_zeichengrenzen() {
        // `len()` zaehlt Bytes: ein Schnitt mitten in einem Umlaut ergaebe
        // ungueltiges UTF-8, und die Meldung ginge gar nicht durch.
        let text = "ä".repeat(50);
        let gekuerzt = schwanz(&text, 10);

        assert_eq!(gekuerzt.chars().count(), 11, "zehn Zeichen plus Auslassung");
        assert!(gekuerzt.starts_with('…'));
    }

    #[test]
    fn kurzer_text_bleibt_unangetastet() {
        assert_eq!(schwanz("  knapp  ", 100), "knapp");
    }

    #[test]
    fn der_schwanz_behaelt_das_ende_nicht_den_anfang() {
        // Das Messergebnis eines Befehls steht am ENDE seiner Ausgabe.
        assert_eq!(schwanz("Anfang-Mitte-ENDE", 4), "…ENDE");
    }

    #[test]
    fn namen_werden_mit_auslassung_gekuerzt() {
        assert_eq!(kurz("abcdefghij", 5), "abcd…");
        assert_eq!(kurz("abc", 5), "abc");
    }

    #[test]
    fn die_antwort_wird_auch_ohne_auftraege_gelesen() {
        // Ohne `#[serde(default)]` scheiterte das Zerlegen einer leeren
        // Antwort — und „nichts faellig" sieht dann wie ein Ausfall aus.
        let a: Antwort = serde_json::from_value(serde_json::json!({})).unwrap();

        assert!(a.auftraege.is_empty());
    }

    #[test]
    fn ein_auftrag_wird_vollstaendig_gelesen() {
        let a: Antwort = serde_json::from_value(serde_json::json!({
            "geraet": "omarchy",
            "auftraege": [{
                "id": 7,
                "name": "Restic prüfen",
                "art": "befehl",
                "inhalt": "systemctl show restic-backup.service -p Result --value",
                "aktiv": true,
                "takt_minuten": 1440,
                "cron": null,
                "stillfrist_minuten": 2160,
                "naechster_lauf": "2026-10-16T03:00:00+00:00",
                "letzte_meldung": null,
                "letzter_status": null,
                "verstummt": true
            }]
        }))
        .unwrap();

        let j = &a.auftraege[0];

        assert_eq!(j.id, 7);
        assert_eq!(j.art, ART_BEFEHL);
        assert!(j.aktiv);
        assert!(j.verstummt);
        assert_eq!(j.takt_minuten, Some(1440));
        assert!(j.cron.is_none());
    }
}
