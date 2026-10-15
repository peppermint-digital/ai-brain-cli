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

/// Wie viele unbeaufsichtigte Auftragslaeufe heute schon liefen.
///
/// **Gezaehlt aus dem Laufregister, nicht aus einem eigenen Zaehler.** Ein
/// zweiter Zaehler waere eine zweite Wahrheit — und die driftet, sobald ein
/// Lauf abbricht, bevor er zaehlen konnte.
fn laeufe_heute() -> usize {
    let Ok(laeufe) = crate::register::liste() else {
        return 0;
    };

    // Mitternacht als Unix-Sekunde, ohne Zeitzonen-Bibliothek: der Tag hat
    // 86400 Sekunden, und wir brauchen keine Kalenderlogik, sondern eine
    // Grenze, die sich taeglich zurueckstellt.
    let tagesbeginn = (crate::register::jetzt() / 86_400) * 86_400;

    laeufe
        .iter()
        .filter(|l| l.gestartet >= tagesbeginn)
        .filter(|l| {
            l.auftrag
                .as_deref()
                .map(|a| a.starts_with(MARKE))
                .unwrap_or(false)
        })
        .count()
}

/// Die Marke, an der unbeaufsichtigte Laeufe erkennbar sind.
const MARKE: &str = "[Taktgeber] ";

// Der abschliessende Abstand ist nicht Kosmetik: `laeufe_heute` prueft mit
// `starts_with(MARKE)`, und der Register-Eintrag entsteht als
// "{marke} {text}". Ohne den Abstand zaehlte "[Taktgeber]etwas" mit,
// "[Taktgeber] etwas" aber nicht — die Tagesgrenze waere wirkungslos.
//
// Als Zusicherung beim BAUEN, nicht als Test: ein Test auf eine Konstante
// prueft zur Laufzeit nichts, der Compiler kennt die Antwort schon. So
// scheitert der Bau, nicht erst der Lauf.
const _: () = assert!(MARKE.as_bytes()[MARKE.len() - 1] == b' ');

/// Wie viele Modelllaeufe der Taktgeber am Tag hoechstens anwirft.
///
/// **Warum es eine Grenze braucht.** Ein Dispatch traegt eine Anweisung fuer
/// ein Modell — anders als ein Dauerauftrag, der meist ein Befehl ist.
/// Unbeaufsichtigt Modelle anzuwerfen ist eine Kostenquelle ohne Aufsicht:
/// Legt jemand (oder ein Fehler in einer Automatik) hundert Auftraege an,
/// laufen sie sonst alle. Zwanzig ist reichlich fuer einen Arbeitstag und
/// klein genug, dass ein Irrlaeufer auffaellt, bevor er teuer wird.
const GRENZE_PRO_TAG: usize = 20;

// Null hielte den Taktgeber dauerhaft an, ohne dass es nach einem Fehler
// aussieht; zu grosszuegig hiesse, dass ein Irrlaeufer teuer wird, bevor er
// auffaellt.
const _: () = assert!(GRENZE_PRO_TAG > 0 && GRENZE_PRO_TAG <= 50);

/// `auftraege` — was liegt an?
pub fn anzeigen(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "--fahren") {
        return fahren(args);
    }

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
    println!("Annehmen und starten:  ai-brain-harness auftrag <nummer>");

    Ok(())
}

/// `auftrag <id> [--imkr] [--vollzugriff] …` — annehmen und arbeiten.
///
/// Der Auftrag wird **vor** dem Start angenommen. Andersherum könnte ein
/// zweiter Lauf ihn in der Zwischenzeit greifen, und zwei Agenten arbeiteten
/// an derselben Aufgabe.
pub fn annehmen(args: &[String]) -> Result<(), String> {
    let Some(roh) = args.first() else {
        return Err("Welcher Auftrag? `ai-brain-harness auftrag <nummer>` (Liste: `auftraege`)".to_string());
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
             Was anliegt:  ai-brain-harness auftraege"
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
        eprintln!("Nachtragen: ai-brain-harness auftrag-melden {gesucht} {status}");
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
            "`ai-brain-harness auftrag-melden <nummer> <completed|failed> [text]`".to_string(),
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

/// `auftraege --fahren` — unbeaufsichtigt abholen und abarbeiten.
///
/// **Die Lücke, die das schliesst.** Dauerauftraege holt ein Taktgeber alle
/// fuenf Minuten ab; ein Dispatch aus der AI-Brain-Oberflaeche wartete
/// dagegen UNBEGRENZT — bis jemand `auftraege` tippt. Wer in der Oberflaeche
/// eine Aufgabe an einen Rechner schickt, hat damit ein Versprechen ohne
/// Frist.
///
/// **Warum keine Parallelitaet.** Die Auftraege werden der Reihe nach
/// gefahren, nicht gleichzeitig. Zwei Modelllaeufe auf demselben Rechner
/// streiten sich um Kontingent und Aufmerksamkeit, und der zweite Fehlschlag
/// verdeckt den ersten.
pub fn fahren(args: &[String]) -> Result<(), String> {
    let still = args.iter().any(|a| a == "--still");
    let connection = config::load()?;
    let offen = holen(&connection, false)?;

    if offen.is_empty() {
        if !still {
            println!("Keine Auftraege fuer diesen Rechner.");
        }

        return Ok(());
    }

    let bereits = laeufe_heute();

    if bereits >= GRENZE_PRO_TAG {
        // Nicht still verschlucken: eine erreichte Grenze ist ein Befund.
        // Wer sie nicht sieht, haelt die Auftraege fuer vergessen.
        return Err(format!(
            "Tagesgrenze erreicht: {bereits} von {GRENZE_PRO_TAG} unbeaufsichtigten Laeufen.\n             {} Auftrag/Auftraege bleiben liegen. Von Hand fahren:  ai-brain-harness auftrag <nummer>",
            offen.len()
        ));
    }

    let frei = GRENZE_PRO_TAG - bereits;
    let zu_fahren: Vec<&Auftrag> = offen.iter().take(frei).collect();

    if !still {
        println!(
            "{} Auftrag/Auftraege, {} heute schon gelaufen (Grenze {GRENZE_PRO_TAG}).",
            offen.len(),
            bereits
        );
        println!();
    }

    let mut gefahren = 0usize;
    let mut gescheitert = 0usize;

    for auftrag in &zu_fahren {
        if !still {
            println!("  #{} {}", auftrag.id, auftrag.titel.as_deref().unwrap_or("(ohne Titel)"));
        }

        // Ueber denselben Weg wie von Hand: `annehmen` greift atomar, startet
        // den Lauf und meldet zurueck. Eine zweite Umsetzung desselben
        // Ablaufs waere eine zweite Stelle, an der das Melden vergessen
        // werden kann.
        let mut eigene: Vec<String> = vec![auftrag.id.to_string()];

        // Die Marke macht den Lauf im Register als unbeaufsichtigt erkennbar —
        // daran zaehlt die Tagesgrenze.
        eigene.push("--marke".to_string());
        eigene.push(MARKE.trim().to_string());

        eigene.extend(
            args.iter()
                .filter(|a| *a != "--fahren" && *a != "--still")
                .cloned(),
        );

        match annehmen(&eigene) {
            Ok(()) => gefahren += 1,
            Err(e) => {
                gescheitert += 1;
                eprintln!("  #{}: {e}", auftrag.id);
            }
        }
    }

    let liegen = offen.len().saturating_sub(zu_fahren.len());

    if !still || gescheitert > 0 || liegen > 0 {
        println!();
        println!("{gefahren} gefahren, {gescheitert} gescheitert, {liegen} liegen (Tagesgrenze).");
    }

    if gescheitert > 0 {
        return Err(format!("{gescheitert} Auftrag/Auftraege sind gescheitert."));
    }

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
