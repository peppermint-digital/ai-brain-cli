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
//!
//! MCP-Server (Typ `mcp`) sind Befehle, die von selbst laufen. Sie gehören
//! nie zu „alles" — weder hier noch auf der Instanz. Wer einen will, hakt ihn
//! einzeln an; die Wahl bleibt in `slugs` erhalten, auch im Modus „alles".

use crate::config::{self, Connection};
use crate::mcp;
use std::collections::BTreeSet;
use std::io::{IsTerminal, Write};

const HOLEN: &str = "get-device-selection-tool";
const SETZEN: &str = "set-device-selection-tool";

const MODUS_ALLES: &str = "alles";
const MODUS_AUSWAHL: &str = "auswahl";

const TYP_MCP: &str = "mcp";

struct Eintrag {
    slug: String,
    typ: String,
    name: String,
}

impl Eintrag {
    fn ist_mcp(&self) -> bool {
        self.typ == TYP_MCP
    }

    /// `typ/slug` — die Form, in der die Auswahl hier geführt wird.
    fn schluessel(&self) -> String {
        format!("{}/{}", self.typ, self.slug)
    }
}

/// Was ein Rechner gewählt hat, so genau, wie die Instanz es sagt.
///
/// Neuere Instanzen liefern `items` mit Typ und Slug. Ältere nur `slugs` —
/// dann ist ein Name typübergreifend gemeint, und ein Skill `playwright` ist
/// vom MCP-Server `playwright` nicht zu unterscheiden (#7891). Für diesen Fall
/// bleibt das alte Verhalten, statt etwas zu erraten.
pub struct Auswahl {
    /// `typ/slug`, nur bei `typgenau`.
    schluessel: BTreeSet<String>,
    slugs: BTreeSet<String>,
    typgenau: bool,
}

impl Auswahl {
    fn lesen(stand: &serde_json::Value) -> Self {
        let slugs = stand
            .get("slugs")
            .and_then(|s| s.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        let Some(items) = stand.get("items").and_then(|i| i.as_array()) else {
            return Auswahl {
                schluessel: BTreeSet::new(),
                slugs,
                typgenau: false,
            };
        };

        let schluessel = items
            .iter()
            .filter_map(|i| {
                let typ = i.get("type")?.as_str()?;
                let slug = i.get("slug")?.as_str()?;
                Some(format!("{typ}/{slug}"))
            })
            .collect();

        Auswahl {
            schluessel,
            slugs,
            typgenau: true,
        }
    }

    /// Nichts gewählt — für Rechner, die noch keinen MCP-Server kennen.
    pub fn leer() -> Self {
        Auswahl {
            schluessel: BTreeSet::new(),
            slugs: BTreeSet::new(),
            typgenau: true,
        }
    }

    /// Ist genau dieser Eintrag gewählt?
    pub fn enthaelt(&self, typ: &str, slug: &str) -> bool {
        if self.typgenau {
            self.schluessel.contains(&format!("{typ}/{slug}"))
        } else {
            self.slugs.contains(slug)
        }
    }

    /// Die Auswahl als `typ/slug`, gemessen am Katalog. Ohne Typangabe der
    /// Instanz zählt ein Slug für jeden Eintrag, der so heißt — wie bisher.
    fn als_schluessel(&self, katalog: &[Eintrag]) -> BTreeSet<String> {
        if self.typgenau {
            return self.schluessel.clone();
        }

        katalog
            .iter()
            .filter(|e| self.slugs.contains(&e.slug))
            .map(Eintrag::schluessel)
            .collect()
    }
}

/// Was dieser Rechner AUSDRÜCKLICH gewählt hat — unabhängig vom Modus.
///
/// Für `sync`: MCP-Server werden nur eingetragen, wenn sie hier stehen. Der
/// Modus „alles" zählt dafür nicht.
pub fn ausdruecklich_gewaehlt(connection: &Connection, token: &str) -> Result<Auswahl, String> {
    let stand = mcp::call_tool(
        connection,
        token,
        HOLEN,
        serde_json::json!({ "catalogue": false, "device": "" }),
    )?;

    Ok(Auswahl::lesen(&stand))
}

fn ist_mcp_schluessel(schluessel: &str) -> bool {
    schluessel
        .split_once('/')
        .map(|(typ, _)| typ == TYP_MCP)
        .unwrap_or(false)
}

/// Die Liste, die gespeichert wird. Im Modus „alles" nur die MCP-Server —
/// alles andere ist dort ohnehin dabei, die Server aber nie von selbst.
///
/// Kennt die Instanz Typen (`typgenau`), geht `typ/slug` hinaus, sonst nur
/// der Slug: Eine ältere Instanz fände `skill/brain` nicht und meldete den
/// Eintrag als unbekannt.
fn zu_speichern(modus: &str, gewaehlt: &BTreeSet<String>, typgenau: bool) -> Vec<String> {
    let behalten = gewaehlt
        .iter()
        .filter(|s| modus != MODUS_ALLES || ist_mcp_schluessel(s));

    if typgenau {
        return behalten.cloned().collect();
    }

    let slugs: BTreeSet<String> = behalten
        .map(|s| s.split_once('/').map(|(_, slug)| slug).unwrap_or(s).to_string())
        .collect();

    slugs.into_iter().collect()
}

/// Ein Name aus `--dazu`/`--weg` als `typ/slug`.
///
/// `typ/slug` wird genommen, wie es ist, sofern der Katalog den Eintrag
/// kennt. Ein blanker Slug muss eindeutig sein — sonst hätte ein Tippfehler
/// oder ein gleichnamiger Skill still den MCP-Server mitgewählt.
fn aufloesen(name: &str, katalog: &[Eintrag]) -> Result<String, String> {
    if name.contains('/') {
        return if katalog.iter().any(|e| e.schluessel() == name) {
            Ok(name.to_string())
        } else {
            Err(format!("{name}: kein solcher Eintrag im Repository"))
        };
    }

    let treffer: Vec<String> = katalog
        .iter()
        .filter(|e| e.slug == name)
        .map(Eintrag::schluessel)
        .collect();

    match treffer.len() {
        0 => Err(format!("{name}: kein solcher Eintrag im Repository")),
        1 => Ok(treffer[0].clone()),
        _ => Err(format!(
            "{name}: mehrdeutig, bitte mit Typ angeben: {}",
            treffer.join(", ")
        )),
    }
}

/// `--dazu`/`--weg` auf die Auswahl anwenden. Der Modus bleibt, wie er ist.
///
/// Gibt zurück, ob sich etwas geändert hat, und je Name eine Zeile dazu.
fn aendern(
    modus: &str,
    gewaehlt: &mut BTreeSet<String>,
    dazu: &[String],
    weg: &[String],
) -> (bool, Vec<String>) {
    let mut geaendert = false;
    let mut meldungen = Vec::new();

    for s in dazu {
        if modus == MODUS_ALLES && !ist_mcp_schluessel(s) {
            meldungen.push(format!("  {s}: ist im Modus „alles“ ohnehin dabei"));
        } else if gewaehlt.insert(s.clone()) {
            geaendert = true;
            meldungen.push(format!("+ {s}"));
        } else {
            meldungen.push(format!("  {s}: war schon gewählt"));
        }
    }

    for s in weg {
        if modus == MODUS_ALLES && !ist_mcp_schluessel(s) {
            meldungen.push(format!(
                "  {s}: im Modus „alles“ nicht einzeln abwählbar (dafür select am Terminal)"
            ));
        } else if gewaehlt.remove(s) {
            geaendert = true;
            meldungen.push(format!("- {s}"));
        } else {
            meldungen.push(format!("  {s}: war nicht gewählt"));
        }
    }

    (geaendert, meldungen)
}

fn liste_lesen(args: &[String], i: usize, option: &str) -> Result<Vec<String>, String> {
    let liste = args
        .get(i)
        .ok_or_else(|| format!("{option} braucht eine Liste, z.B. {option} brain,mcp/playwright"))?;

    Ok(liste
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mut geraet: Option<String> = None;
    let mut setzen: Option<Vec<String>> = None;
    let mut alles = false;
    let mut nur_zeigen = false;
    let mut dazu: Vec<String> = Vec::new();
    let mut weg: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--geraet" | "--gerät" => {
                i += 1;
                geraet = args.get(i).cloned();
            }
            "--setzen" => {
                i += 1;
                setzen = Some(liste_lesen(args, i, "--setzen")?);
            }
            "--dazu" => {
                i += 1;
                dazu.extend(liste_lesen(args, i, "--dazu")?);
            }
            "--weg" => {
                i += 1;
                weg.extend(liste_lesen(args, i, "--weg")?);
            }
            "--alles" => alles = true,
            "--zeigen" => nur_zeigen = true,
            other => return Err(format!("Unbekannte Option für select: {other}")),
        }
        i += 1;
    }

    let aendert_einzeln = !dazu.is_empty() || !weg.is_empty();
    if aendert_einzeln && (alles || setzen.is_some()) {
        return Err("--dazu/--weg ändern die bestehende Auswahl; nicht zusammen mit --setzen oder --alles.".to_string());
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
    let auswahl = Auswahl::lesen(&stand);
    let typgenau = auswahl.typgenau;
    let mut gewaehlt: BTreeSet<String> = auswahl.als_schluessel(&katalog);

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
        // Gewählte MCP-Server bleiben gewählt — „alles" schaltet sie weder
        // ein noch aus.
        let liste = zu_speichern(MODUS_ALLES, &gewaehlt, typgenau);

        return speichern(&connection, &token, &geraet, MODUS_ALLES, &liste, &geraetename);
    }

    if aendert_einzeln {
        // Erst alles auflösen, dann ändern: Ein unbekannter oder mehrdeutiger
        // Name bricht ab, bevor irgendetwas gespeichert ist.
        let mut fehler = Vec::new();
        let mut aufgeloest = |namen: &[String]| -> Vec<String> {
            namen
                .iter()
                .filter_map(|n| aufloesen(n, &katalog).map_err(|e| fehler.push(e)).ok())
                .collect()
        };
        let dazu = aufgeloest(&dazu);
        let weg = aufgeloest(&weg);

        if !fehler.is_empty() {
            for f in &fehler {
                eprintln!("{f}");
            }
            return Err("Nichts gespeichert.".to_string());
        }

        let (geaendert, meldungen) = aendern(&modus, &mut gewaehlt, &dazu, &weg);
        for m in &meldungen {
            println!("{m}");
        }

        if !geaendert {
            println!("Nichts geändert.");
            return Ok(());
        }

        let liste = zu_speichern(&modus, &gewaehlt, typgenau);

        return speichern(&connection, &token, &geraet, &modus, &liste, &geraetename);
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
            println!("Ergänzen:    select --dazu mcp/playwright   ·   Entfernen: select --weg mcp/playwright");
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
                let liste = zu_speichern(&modus, &gewaehlt, typgenau);

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
                        let nummer = katalog
                            .iter()
                            .position(|k| k.schluessel() == e.schluessel())
                            .unwrap_or(0)
                            + 1;
                        println!(
                            "  {nummer:>3} [{}] {} ({})",
                            if gewaehlt.contains(&e.schluessel()) { "x" } else { " " },
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
                            let eintrag = &katalog[n - 1];
                            let schluessel = eintrag.schluessel();
                            if !gewaehlt.remove(&schluessel) {
                                gewaehlt.insert(schluessel);
                            }
                            // Sobald jemand einzeln wählt, ist „alles“ nicht
                            // mehr gemeint — sonst hakt man an und nichts ändert
                            // sich, weil ohnehin alles gilt. MCP-Server sind
                            // davon ausgenommen: Die gehören nie zu „alles“,
                            // einen anzuhaken heisst also nicht, den Rest
                            // abzuwählen.
                            if !eintrag.ist_mcp() {
                                modus = MODUS_AUSWAHL.to_string();
                            }
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
            if e.ist_mcp() {
                println!("MCP-SERVER — nur einzeln, „alles“ nimmt sie nie mit");
            } else {
                println!("{}", e.typ.to_uppercase());
            }
            letzter_typ = e.typ.clone();
        }

        println!(
            "  {:>3} [{}] {}",
            i + 1,
            if gewaehlt.contains(&e.schluessel()) { "x" } else { " " },
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
    println!("Wirksam wird das beim naechsten Abgleich:  ai-brain-harness sync --apply");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eintrag(slug: &str, typ: &str) -> Eintrag {
        Eintrag {
            slug: slug.to_string(),
            typ: typ.to_string(),
            name: slug.to_string(),
        }
    }

    fn menge(werte: &[&str]) -> BTreeSet<String> {
        werte.iter().map(|s| s.to_string()).collect()
    }

    fn katalog() -> Vec<Eintrag> {
        vec![
            eintrag("brain", "skill"),
            eintrag("fertig", "skill"),
            eintrag("playwright", "mcp"),
            eintrag("playwright", "skill"),
            eintrag("chrome-devtools", "mcp"),
        ]
    }

    #[test]
    fn alles_speichert_nur_die_gewaehlten_mcp_server() {
        let gewaehlt = menge(&["skill/brain", "mcp/playwright"]);

        assert_eq!(zu_speichern(MODUS_ALLES, &gewaehlt, true), vec!["mcp/playwright"]);

        // Im Modus „auswahl" gilt die Liste, wie sie ist.
        assert_eq!(
            zu_speichern(MODUS_AUSWAHL, &gewaehlt, true),
            vec!["mcp/playwright", "skill/brain"]
        );

        // Nichts gewählt: „alles" nimmt keinen Server mit.
        assert!(zu_speichern(MODUS_ALLES, &BTreeSet::new(), true).is_empty());
    }

    #[test]
    fn alte_instanz_bekommt_nur_slugs() {
        let gewaehlt = menge(&["skill/brain", "mcp/playwright", "skill/playwright"]);

        assert_eq!(
            zu_speichern(MODUS_AUSWAHL, &gewaehlt, false),
            vec!["brain", "playwright"]
        );
        assert_eq!(zu_speichern(MODUS_ALLES, &gewaehlt, false), vec!["playwright"]);
    }

    #[test]
    fn aufloesen_eindeutig_mehrdeutig_unbekannt() {
        let k = katalog();

        assert_eq!(aufloesen("brain", &k).unwrap(), "skill/brain");
        assert_eq!(aufloesen("chrome-devtools", &k).unwrap(), "mcp/chrome-devtools");
        assert_eq!(aufloesen("mcp/playwright", &k).unwrap(), "mcp/playwright");

        let mehrdeutig = aufloesen("playwright", &k).unwrap_err();
        assert!(mehrdeutig.contains("mehrdeutig"));
        assert!(mehrdeutig.contains("mcp/playwright") && mehrdeutig.contains("skill/playwright"));

        assert!(aufloesen("gibtsnicht", &k).is_err());
        assert!(aufloesen("agent/brain", &k).is_err());
    }

    #[test]
    fn dazu_und_weg_im_modus_alles() {
        let mut gewaehlt = menge(&["mcp/playwright"]);

        let (geaendert, meldungen) = aendern(
            MODUS_ALLES,
            &mut gewaehlt,
            &["mcp/chrome-devtools".to_string(), "skill/brain".to_string()],
            &["mcp/playwright".to_string()],
        );

        assert!(geaendert);
        assert_eq!(gewaehlt, menge(&["mcp/chrome-devtools"]));
        // Ein Skill wird im Modus „alles" nicht einzeln geführt — Hinweis, kein Eintrag.
        assert!(meldungen.iter().any(|m| m.contains("skill/brain") && m.contains("ohnehin dabei")));
        // Gespeichert wird weiter im Modus „alles", nur mit den Servern.
        assert_eq!(zu_speichern(MODUS_ALLES, &gewaehlt, true), vec!["mcp/chrome-devtools"]);
    }

    #[test]
    fn dazu_und_weg_im_modus_auswahl() {
        let mut gewaehlt = menge(&["skill/brain", "skill/fertig"]);

        let (geaendert, _) = aendern(
            MODUS_AUSWAHL,
            &mut gewaehlt,
            &["mcp/playwright".to_string()],
            &["skill/fertig".to_string()],
        );

        assert!(geaendert);
        assert_eq!(gewaehlt, menge(&["mcp/playwright", "skill/brain"]));

        // Was schon gewählt bzw. nicht gewählt ist, ändert nichts.
        let (geaendert, meldungen) = aendern(
            MODUS_AUSWAHL,
            &mut gewaehlt,
            &["skill/brain".to_string()],
            &["skill/fertig".to_string()],
        );
        assert!(!geaendert);
        assert_eq!(meldungen.len(), 2);
    }

    #[test]
    fn typgenaue_auswahl_trennt_skill_und_server() {
        let stand = serde_json::json!({
            "mode": "alles",
            "slugs": ["playwright"],
            "items": [{ "type": "skill", "slug": "playwright" }],
        });
        let auswahl = Auswahl::lesen(&stand);

        assert!(auswahl.enthaelt("skill", "playwright"));
        // Der gleichnamige Skill wählt den Server NICHT mit.
        assert!(!auswahl.enthaelt("mcp", "playwright"));
    }

    #[test]
    fn ohne_items_gilt_der_slug_typuebergreifend() {
        let stand = serde_json::json!({ "mode": "alles", "slugs": ["playwright"] });
        let auswahl = Auswahl::lesen(&stand);

        assert!(auswahl.enthaelt("mcp", "playwright"));
        assert!(auswahl.enthaelt("skill", "playwright"));
        assert_eq!(
            auswahl.als_schluessel(&katalog()),
            menge(&["mcp/playwright", "skill/playwright"])
        );
    }
}
