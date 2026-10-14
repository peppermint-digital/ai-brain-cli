//! Einträge dieses Rechners an- und abwählen.
//!
//! Die Auswahl liegt auf der **Instanz**, nicht hier — pro Gerät, sichtbar auch
//! unter „Meine Geräte". Dieser Befehl ist eine zweite Bedienung derselben
//! Einstellung, keine zweite Einstellung. Würde hier lokal gefiltert, gäbe es
//! drei Wahrheiten (Maske, Instanz, Rechner) und keine gültige.
//!
//! Bedienung bewusst über Nummern statt Pfeiltasten: Das läuft über SSH, im
//! Windows-Terminal und in jedem Terminal gleich, und es kostet keine
//! Abhängigkeit.

use crate::config::{self, Connection};
use crate::mcp;
use std::collections::BTreeSet;
use std::io::{IsTerminal, Write};

const HOLEN: &str = "get-device-selection-tool";
const SETZEN: &str = "set-device-selection-tool";

const MODUS_ALLES: &str = "alles";
const MODUS_AUSWAHL: &str = "auswahl";

struct Eintrag {
    slug: String,
    typ: String,
    name: String,
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mut geraet: Option<String> = None;
    let mut setzen: Option<Vec<String>> = None;
    let mut alles = false;
    let mut nur_zeigen = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--geraet" | "--gerät" => {
                i += 1;
                geraet = args.get(i).cloned();
            }
            "--setzen" => {
                i += 1;
                let liste = args.get(i).ok_or_else(|| {
                    "--setzen braucht eine Liste, z.B. --setzen brain,fertig".to_string()
                })?;
                setzen = Some(
                    liste
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                );
            }
            "--alles" => alles = true,
            "--zeigen" => nur_zeigen = true,
            other => return Err(format!("Unbekannte Option für select: {other}")),
        }
        i += 1;
    }

    let connection = config::load()?;
    let token = mcp::access_token(&connection)?;

    let stand = mcp::call_tool(
        &connection,
        &token,
        HOLEN,
        serde_json::json!({
            "catalogue": true,
            "device": geraet.clone().unwrap_or_default(),
        }),
    )?;

    let katalog = katalog_lesen(&stand);
    let mut gewaehlt: BTreeSet<String> = stand
        .get("slugs")
        .and_then(|s| s.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let mut modus = stand
        .get("mode")
        .and_then(|m| m.as_str())
        .unwrap_or(MODUS_ALLES)
        .to_string();

    let geraetename = stand
        .get("device")
        .and_then(|d| d.as_str())
        .unwrap_or("dieser Rechner")
        .to_string();

    // Nicht-interaktive Wege zuerst: Sie gelten auch ohne Terminal und sind
    // der Weg für Skripte und Agenten.
    if alles {
        return speichern(&connection, &token, &geraet, MODUS_ALLES, &[], &geraetename);
    }

    if let Some(liste) = setzen {
        return speichern(
            &connection,
            &token,
            &geraet,
            MODUS_AUSWAHL,
            &liste,
            &geraetename,
        );
    }

    if nur_zeigen || !std::io::stdin().is_terminal() {
        // Ohne Terminal wird NICHT gefragt — sonst hinge ein Lauf in einer
        // Rückfrage, die niemand sieht, bis das Zeitlimit greift.
        zeigen(&geraetename, &modus, &gewaehlt, &katalog);

        if !nur_zeigen {
            println!();
            println!("Kein Terminal — nichts geändert.");
            println!("Zum Setzen:  select --setzen brain,fertig   oder   select --alles");
        }

        return Ok(());
    }

    loop {
        zeigen(&geraetename, &modus, &gewaehlt, &katalog);
        println!();
        println!("Nummern schalten um · „alles“ · „keine“ · „/suchwort“ · Enter speichert · „q“ bricht ab");
        print!("> ");
        std::io::stdout()
            .flush()
            .map_err(|e| format!("Ausgabe nicht schreibbar: {e}"))?;

        let mut zeile = String::new();
        if std::io::stdin()
            .read_line(&mut zeile)
            .map_err(|e| format!("Eingabe nicht lesbar: {e}"))?
            == 0
        {
            // Eingabe zu Ende (Strg-D) — wie Abbruch behandeln, nicht wie
            // Bestätigung. Was gespeichert wird, sagt jemand ausdrücklich.
            println!();
            println!("Abgebrochen, nichts geändert.");

            return Ok(());
        }

        let eingabe = zeile.trim();

        match eingabe {
            "" => {
                let liste: Vec<String> = gewaehlt.iter().cloned().collect();

                return speichern(&connection, &token, &geraet, &modus, &liste, &geraetename);
            }
            "q" | "Q" | "abbruch" => {
                println!("Abgebrochen, nichts geändert.");

                return Ok(());
            }
            "alles" | "alle" => {
                modus = MODUS_ALLES.to_string();
            }
            "keine" => {
                modus = MODUS_AUSWAHL.to_string();
                gewaehlt.clear();
            }
            suche if suche.starts_with('/') => {
                let wort = suche[1..].trim().to_lowercase();
                let treffer: Vec<&Eintrag> = katalog
                    .iter()
                    .filter(|e| {
                        e.slug.to_lowercase().contains(&wort)
                            || e.name.to_lowercase().contains(&wort)
                    })
                    .collect();

                println!();
                if treffer.is_empty() {
                    println!("Nichts gefunden für „{wort}“.");
                } else {
                    for e in treffer {
                        let nummer = katalog.iter().position(|k| k.slug == e.slug).unwrap_or(0) + 1;
                        println!(
                            "  {nummer:>3} [{}] {} ({})",
                            if gewaehlt.contains(&e.slug) { "x" } else { " " },
                            e.name,
                            e.typ
                        );
                    }
                }
                println!();
            }
            _ => {
                let mut unbekannt: Vec<String> = Vec::new();

                for teil in eingabe.split_whitespace() {
                    match teil.parse::<usize>() {
                        Ok(n) if n >= 1 && n <= katalog.len() => {
                            let slug = katalog[n - 1].slug.clone();
                            if !gewaehlt.remove(&slug) {
                                gewaehlt.insert(slug);
                            }
                            // Sobald jemand einzeln wählt, ist „alles“ nicht
                            // mehr gemeint — sonst hakt man an und nichts ändert
                            // sich, weil ohnehin alles gilt.
                            modus = MODUS_AUSWAHL.to_string();
                        }
                        _ => unbekannt.push(teil.to_string()),
                    }
                }

                if !unbekannt.is_empty() {
                    println!();
                    println!("Keine gültige Nummer: {}", unbekannt.join(", "));
                }
            }
        }
    }
}

fn katalog_lesen(stand: &serde_json::Value) -> Vec<Eintrag> {
    stand
        .get("catalogue")
        .and_then(|c| c.as_array())
        .map(|a| {
            a.iter()
                .map(|e| Eintrag {
                    slug: e
                        .get("slug")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    typ: e
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    name: e
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
                .filter(|e| !e.slug.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn zeigen(geraet: &str, modus: &str, gewaehlt: &BTreeSet<String>, katalog: &[Eintrag]) {
    println!();
    println!("Rechner: {geraet}");

    if modus == MODUS_ALLES {
        println!("Modus  : alles — dieser Rechner bekommt jeden Eintrag, auch künftige");
    } else {
        println!(
            "Modus  : auswahl — genau die angehakten ({})",
            gewaehlt.len()
        );
    }

    let mut letzter_typ = String::new();

    for (i, e) in katalog.iter().enumerate() {
        if e.typ != letzter_typ {
            println!();
            println!("{}", e.typ.to_uppercase());
            letzter_typ = e.typ.clone();
        }

        println!(
            "  {:>3} [{}] {}",
            i + 1,
            if gewaehlt.contains(&e.slug) { "x" } else { " " },
            e.name
        );
    }
}

fn speichern(
    connection: &Connection,
    token: &str,
    geraet: &Option<String>,
    modus: &str,
    slugs: &[String],
    geraetename: &str,
) -> Result<(), String> {
    let ergebnis = mcp::call_tool(
        connection,
        token,
        SETZEN,
        serde_json::json!({
            "mode": modus,
            "slugs": slugs,
            "device": geraet.clone().unwrap_or_default(),
        }),
    )?;

    let anzahl = ergebnis.get("count").and_then(|c| c.as_u64()).unwrap_or(0);

    println!();
    if modus == MODUS_ALLES {
        println!("Gespeichert: {geraetename} bekommt alles.");
    } else {
        println!("Gespeichert: {geraetename} bekommt {anzahl} Einträge.");
    }

    // Ein Tippfehler soll nicht als „abgewählt“ durchgehen.
    if let Some(unbekannt) = ergebnis.get("unbekannt").and_then(|u| u.as_array()) {
        if !unbekannt.is_empty() {
            let namen: Vec<&str> = unbekannt.iter().filter_map(|v| v.as_str()).collect();
            println!("Unbekannt und deshalb NICHT gesetzt: {}", namen.join(", "));
        }
    }

    println!();
    println!("Wirksam wird das beim naechsten Abgleich:  ai-brain-cli sync --apply");

    Ok(())
}
