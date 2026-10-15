//! Aufträge, die AI Brain für diesen Rechner bereitgelegt hat (#7675).
//!
//! **Holprinzip.** AI Brain ruft hier nichts auf — dieser Rechner fragt, was
//! für ihn anliegt. Damit bleibt die Vertrauensrichtung Gerät → Instanz wie
//! bei allem anderen, und es muss kein Port offen stehen.
//!
//! **Das Ziel kommt aus dem Gerätezugang, nicht aus der Anfrage.** Es gibt
//! keinen Schalter, mit dem dieser Rechner fremde Aufträge erfragen könnte —
//! die Werkzeuge in AI Brain nehmen dafür keinen Parameter.
//!
//! Zwei Schritte, bewusst getrennt: `auftraege` zeigt nur, `auftrag <id>` nimmt
//! an und startet. Ein Befehl, der beim Hinsehen schon Arbeit anstösst, nimmt
//! einem die Entscheidung ab.

use serde::Deserialize;

use crate::config::{self, Connection};
use crate::mcp;

#[derive(Debug, Deserialize)]
struct Auftrag {
    id: u64,
    task_id: u64,
    #[serde(default)]
    titel: Option<String>,
    #[serde(default)]
    projekt: Option<String>,
    #[serde(default)]
    angelegt: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Antwort {
    #[serde(default)]
    auftraege: Vec<Auftrag>,
}

/// Wie lange ein Auftrag schon wartet, grob.
///
/// Nur Datum und Uhrzeit aus dem ISO-Zeitstempel — eine Dauer auszurechnen
/// hiesse, Zeitzonen und Schaltsekunden selbst zu behandeln, und dafür käme
/// eine sechste Abhängigkeit ins Haus. Das lohnt für eine Spalte nicht.
/// Ein ISO-Zeitstempel als `14.10. 12:34`.
///
/// Oeffentlich, weil die Dauerauftraege dieselbe Schreibweise brauchen — zwei
/// Datumsformate in einem Programm sind ein Fehler, den jeder sofort sieht.
pub fn seit(iso: Option<&str>) -> String {
    let Some(iso) = iso else {
        return "—".to_string();
    };

    // 2026-10-14T12:34:56+02:00 → 14.10. 12:34
    let (datum, rest) = match iso.split_once('T') {
        Some(paar) => paar,
        None => return iso.to_string(),
    };

    let teile: Vec<&str> = datum.split('-').collect();
    let uhrzeit: String = rest.chars().take(5).collect();

    if teile.len() == 3 {
        format!("{}.{}. {}", teile[2], teile[1], uhrzeit)
    } else {
        iso.to_string()
    }
}

/// Fragt AI Brain, was für diesen Rechner bereitliegt.
///
/// `annehmen` entscheidet, ob die Aufträge dabei gegriffen werden. Das ist
/// kein Zierschalter: Greifen heisst, dass kein zweiter Lauf sie bekommt —
/// also nur, wenn man sie wirklich startet.
fn holen(connection: &Connection, annehmen: bool) -> Result<Vec<Auftrag>, String> {
    let token = mcp::access_token(connection)?;

    let roh = mcp::call_tool(
        connection,
        &token,
        "fetch-dispatches-tool",
        serde_json::json!({ "annehmen": annehmen }),
    )?;

    let antwort: Antwort = serde_json::from_value(roh)
        .map_err(|e| format!("Antwort auf fetch-dispatches-tool ist unlesbar: {e}"))?;

    Ok(antwort.auftraege)
}

/// `auftraege` — was liegt an?
pub fn anzeigen(args: &[String]) -> Result<(), String> {
    let connection = config::load()?;
    let auftraege = holen(&connection, false)?;

    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "auftraege": auftraege.iter().map(|a| serde_json::json!({
                "id": a.id,
                "task_id": a.task_id,
                "titel": a.titel,
                "projekt": a.projekt,
            })).collect::<Vec<_>>()
        })).map_err(|e| format!("{e}"))?);

        return Ok(());
    }

    if auftraege.is_empty() {
        println!("Keine Aufträge für diesen Rechner.");
        println!();
        println!("Aufträge legt AI Brain an — aus der Oberfläche, dem Avatar oder per");
        println!("`dispatch-task-to-device-tool`. Dieser Rechner holt sie dann hier ab.");

        return Ok(());
    }

    println!("{} Auftrag/Aufträge für diesen Rechner:", auftraege.len());
    println!();

    for a in &auftraege {
        println!(
            "  #{:<5} Aufgabe #{:<6} {:<12} {:<11} {}",
            a.id,
            a.task_id,
            a.projekt.as_deref().unwrap_or("—"),
            // Wie lange liegt er schon? Ein Auftrag, der seit Tagen wartet,
            // ist eine andere Nachricht als einer von vor fünf Minuten.
            seit(a.angelegt.as_deref()),
            a.titel.as_deref().unwrap_or("(ohne Titel)")
        );
    }

    println!();
    println!("Annehmen und starten:  ai-brain-cli auftrag <nummer>");

    Ok(())
}

/// `auftrag <id> [--imkr] [--vollzugriff] …` — annehmen und arbeiten.
///
/// Der Auftrag wird **vor** dem Start angenommen. Andersherum könnte ein
/// zweiter Lauf ihn in der Zwischenzeit greifen, und zwei Agenten arbeiteten
/// an derselben Aufgabe.
pub fn annehmen(args: &[String]) -> Result<(), String> {
    let Some(roh) = args.first() else {
        return Err("Welcher Auftrag? `ai-brain-cli auftrag <nummer>` (Liste: `auftraege`)".to_string());
    };

    let gesucht: u64 = roh
        .trim_start_matches('#')
        .parse()
        .map_err(|_| format!("`{roh}` ist keine Auftragsnummer."))?;

    let connection = config::load()?;

    // Erst schauen, ob es ihn für diesen Rechner überhaupt gibt — sonst
    // greifen wir blind und melden einen Fehler, nachdem schon etwas
    // passiert ist.
    let offen = holen(&connection, false)?;

    let Some(ziel) = offen.iter().find(|a| a.id == gesucht) else {
        return Err(format!(
            "Auftrag #{gesucht} liegt für diesen Rechner nicht (mehr) bereit.\n\
             Was anliegt:  ai-brain-cli auftraege"
        ));
    };

    let task = ziel.task_id;
    let projekt = ziel.projekt.clone();

    // Jetzt greifen. Von hier an gehört der Auftrag diesem Lauf.
    let angenommen = holen(&connection, true)?;

    if !angenommen.iter().any(|a| a.id == gesucht) {
        return Err(format!(
            "Auftrag #{gesucht} war schneller weg als dieser Aufruf — ein anderer Lauf hat ihn."
        ));
    }

    println!("Auftrag #{gesucht} angenommen (Aufgabe #{task}).");
    println!();

    // Von hier übernimmt der gewöhnliche Lauf. `--task` holt die Beschreibung
    // aus AI Brain, alles andere (Rechte, Zeitgrenze, imkr) kommt von den
    // weitergegebenen Schaltern.
    let mut agent_args: Vec<String> = vec!["--task".to_string(), task.to_string()];

    if let Some(p) = projekt {
        agent_args.push("--projekt".to_string());
        agent_args.push(p);
    }

    agent_args.extend(args[1..].iter().cloned());

    let ausgang = crate::agent::starten(&agent_args);

    // Zurückmelden, sonst bleibt der Auftrag für immer „angenommen" — und
    // niemand sieht, ob er etwas geworden ist. Das Melden darf den Ausgang des
    // Laufs nicht verschlucken: Es ergänzt ihn, es ersetzt ihn nicht.
    let (status, text) = match &ausgang {
        Ok(()) => (
            "completed",
            format!("Lauf zu Auftrag #{gesucht} durchgelaufen."),
        ),
        Err(e) => ("failed", e.clone()),
    };

    if let Err(e) = melden(&connection, gesucht, status, &text) {
        eprintln!("Hinweis: Ergebnis von Auftrag #{gesucht} nicht gemeldet ({e}).");
        eprintln!("Nachtragen: ai-brain-cli auftrag-melden {gesucht} {status}");
    }

    ausgang
}

/// Das Ergebnis eines Auftrags nach AI Brain zurückschreiben.
fn melden(
    connection: &Connection,
    auftrag: u64,
    status: &str,
    text: &str,
) -> Result<(), String> {
    let token = mcp::access_token(connection)?;

    mcp::call_tool_text(
        connection,
        &token,
        "complete-dispatch-tool",
        serde_json::json!({
            "dispatch_id": auftrag,
            "status": status,
            "response": text,
        }),
    )
    .map(|_| ())
}

/// `auftrag-melden <id> <completed|failed> [text]` — von Hand nachtragen.
///
/// Gibt es, weil das Melden nach einem langen Lauf scheitern kann (Netz weg,
/// Token abgelaufen). Ohne diesen Weg bliebe der Auftrag „angenommen" stehen,
/// und die Aufgabe hinge in `in_progress` — für jeden Filter unsichtbar.
pub fn nachmelden(args: &[String]) -> Result<(), String> {
    let (Some(roh), Some(status)) = (args.first(), args.get(1)) else {
        return Err(
            "`ai-brain-cli auftrag-melden <nummer> <completed|failed> [text]`".to_string(),
        );
    };

    if status != "completed" && status != "failed" {
        return Err(format!("`{status}` ist kein Status. Möglich: completed, failed."));
    }

    let nummer: u64 = roh
        .trim_start_matches('#')
        .parse()
        .map_err(|_| format!("`{roh}` ist keine Auftragsnummer."))?;

    let connection = config::load()?;
    let text = if args.len() > 2 {
        args[2..].join(" ")
    } else {
        format!("Von Hand als {status} vermerkt.")
    };

    melden(&connection, nummer, status, &text)?;
    println!("Auftrag #{nummer} als {status} gemeldet.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::seit;

    #[test]
    fn seit_liest_den_zeitstempel_und_verschluckt_nichts() {
        assert_eq!(seit(Some("2026-10-14T12:34:56+02:00")), "14.10. 12:34");
        assert_eq!(seit(Some("2026-01-02T09:05:00Z")), "02.01. 09:05");

        // Kein Zeitstempel: ein Strich, kein Absturz und keine Erfindung.
        assert_eq!(seit(None), "—");

        // Was nicht nach ISO aussieht, wird durchgereicht statt zerlegt — eine
        // falsch geratene Zeile ist schlimmer als eine unschöne.
        assert_eq!(seit(Some("irgendwann")), "irgendwann");
        assert_eq!(seit(Some("2026-10T12:00")), "2026-10T12:00");
    }
}
