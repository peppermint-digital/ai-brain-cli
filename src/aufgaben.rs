//! Offene Aufgaben zum Auswaehlen statt Abtippen (#500).
//!
//! **Die Antwort wird durchgereicht, nicht neu gebaut.** Das imkr-Plugin
//! `plugins/ai-brain/start.sh` zerlegt `--json` mit `jq`. Ein zweiter Ort, der
//! dieselbe Form beschreibt, ist ein zweiter Ort, der von ihr abweichen kann —
//! deshalb geht die Nutzlast der Instanz unveraendert hinaus.
//!
//! Gegenstueck zu `auftraege`, und bewusst anders benannt: Ein **Auftrag** ist
//! etwas, das AI Brain diesem Rechner hingelegt hat; eine **Aufgabe** ist ein
//! Eintrag aus der Projektliste, den sich jemand aussucht.

use serde::Deserialize;

use crate::config;
use crate::mcp;

#[derive(Debug, Deserialize)]
struct Aufgabe {
    id: u64,
    titel: String,
    #[serde(default)]
    prioritaet: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    typ: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Liste {
    #[serde(default)]
    projekt: Option<String>,
    #[serde(default)]
    aufgaben: Vec<Aufgabe>,
}

/// Wie dringlich etwas aussieht, in einem Zeichen.
///
/// Kein Wort, weil die Liste in einer Auswahlmaske steht und jede Spalte
/// Platz kostet.
fn zeichen(prioritaet: Option<&str>) -> &'static str {
    match prioritaet {
        Some("critical") => "!!",
        Some("high") => "! ",
        Some("low") => "  ",
        _ => "· ",
    }
}

/// `aufgaben [--projekt SLUG] [--json]`
pub fn anzeigen(args: &[String]) -> Result<(), String> {
    let projekt = args
        .iter()
        .position(|a| a == "--projekt" || a == "--project")
        .and_then(|i| args.get(i + 1))
        .filter(|w| !w.starts_with("--"))
        .cloned();

    let connection = config::load()?;
    let token = mcp::access_token(&connection)?;

    let mut nutzlast = serde_json::json!({});

    if let Some(p) = &projekt {
        nutzlast["project"] = serde_json::json!(p);
    }

    let roh = mcp::call_tool(&connection, &token, "list-tasks-bundle-tool", nutzlast)?;

    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string(&roh).map_err(|e| format!("{e}"))?);

        return Ok(());
    }

    let liste: Liste = serde_json::from_value(roh)
        .map_err(|e| format!("Antwort auf list-tasks-bundle-tool ist unlesbar: {e}"))?;

    if liste.aufgaben.is_empty() {
        match &liste.projekt {
            Some(p) => println!("Keine offenen Aufgaben in {p}."),
            None => println!("Keine offenen Aufgaben."),
        }

        return Ok(());
    }

    match &liste.projekt {
        Some(p) => println!("{} offene Aufgabe(n) in {p}:", liste.aufgaben.len()),
        None => println!("{} offene Aufgabe(n), projektuebergreifend:", liste.aufgaben.len()),
    }

    println!();

    for a in &liste.aufgaben {
        println!(
            "  {} #{:<6} {:<12} {:<12} {}",
            zeichen(a.prioritaet.as_deref()),
            a.id,
            a.status.as_deref().unwrap_or("—"),
            a.typ.as_deref().unwrap_or("—"),
            a.titel
        );
    }

    println!();
    println!("Anfangen:  ai-brain-cli agent --task <nummer> --imkr");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::zeichen;

    #[test]
    fn zeichen_sind_gleich_breit() {
        // Eine Auswahlliste, deren Spalten verrutschen, liest sich schlechter
        // als eine ohne Kennzeichnung.
        for p in [Some("critical"), Some("high"), Some("medium"), Some("low"), None] {
            // chars(), nicht len(): `·` ist mehrbytig, und fuer die Spalte
            // zaehlen Zeichen, nicht Bytes. Genau diese Verwechslung hat der
            // Test beim ersten Lauf gefunden.
            assert_eq!(
                zeichen(p).chars().count(),
                2,
                "Prioritaet {p:?} bricht die Spalte"
            );
        }
    }

    #[test]
    fn unbekannte_prioritaet_faellt_nicht_durch() {
        assert_eq!(zeichen(Some("erfunden")), zeichen(Some("medium")));
    }
}
