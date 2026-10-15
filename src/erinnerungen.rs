//! Das Gedaechtnis des Harness, zwischen Rechner und AI Brain (#502).
//!
//! **Das Problem.** Claude Code fuehrt sein Gedaechtnis unter
//! `~/.claude/projects/<pfad>/memory/` — pro Rechner und pro Verzeichnispfad.
//! Gemessen am 15.10.2026: 194 Eintraege in 21 Projekten auf einem Rechner,
//! auf dem Laptop keiner davon. Genau die Sackgasse, wegen der es AI Brain
//! gibt: Was ein Agent lernt, endet sonst an der Maschine.
//!
//! **Die Loesung ist dieselbe wie bei den Skills.** AI Brain ist das Zuhause,
//! die lokalen Dateien sind eine Arbeitskopie. `ziehen` holt, `zurueckgeben`
//! bringt — dieselben Worte und dieselbe Vorsicht wie `sync`, damit niemand
//! ein zweites Modell lernen muss.
//!
//! **Die Datei wird wortgleich uebertragen.** Kennung ist der Dateiname, Art
//! und Beschreibung werden nur FUER DIE ANZEIGE aus dem Vorspann gelesen —
//! gespeichert wird der ganze Text. Felder einzeln abzubilden hiesse, bei
//! jedem neuen Feld etwas still zu verlieren.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::config::{self, Connection};
use crate::mcp;

#[derive(Debug, Deserialize)]
struct Erinnerung {
    name: String,
    #[serde(default)]
    art: String,
    #[serde(default)]
    inhalt: String,
    #[serde(default)]
    projekt: String,
    #[serde(default)]
    pruefsumme: String,
}

#[derive(Debug, Deserialize)]
struct Antwort {
    #[serde(default)]
    erinnerungen: Vec<Erinnerung>,
}

/// Wo Claude Code das Gedaechtnis eines Verzeichnisses fuehrt.
///
/// Die Regel ist seine, nicht unsere: der absolute Pfad mit `/` zu `-`. Sie
/// hier nachzubauen ist unschoen, aber die Alternative waere, Claude Code zu
/// bitten, uns seinen Ort zu verraten — und das kann es nicht.
pub fn gedaechtnis_pfad(verzeichnis: &Path) -> Option<PathBuf> {
    let heimat = std::env::var("HOME").ok()?;
    let schluessel = verzeichnis.to_string_lossy().replace('/', "-");

    Some(
        Path::new(&heimat)
            .join(".claude")
            .join("projects")
            .join(schluessel)
            .join("memory"),
    )
}

/// Art und Beschreibung aus dem Vorspann — nur fuer die Anzeige.
pub fn vorspann(text: &str) -> (String, String) {
    let mut art = String::new();
    let mut beschreibung = String::new();
    let mut im_vorspann = false;

    for (i, zeile) in text.lines().enumerate() {
        if zeile.trim() == "---" {
            if i == 0 {
                im_vorspann = true;
                continue;
            }

            if im_vorspann {
                break;
            }
        }

        if !im_vorspann {
            continue;
        }

        let roh = zeile.trim();

        if let Some(w) = roh.strip_prefix("type:") {
            art = w.trim().to_string();
        } else if let Some(w) = roh.strip_prefix("description:") {
            beschreibung = w.trim().to_string();
        }
    }

    (art, beschreibung)
}

/// Was beim letzten Abgleich auf beiden Seiten stand.
///
/// **Ohne diesen Stand ist jede Abweichung ein Konflikt.** Der erste Entwurf
/// verglich nur hier gegen dort: Sobald eine Datei lokal waechst, sieht das
/// aus wie „beidseitig geaendert", und der Eintrag liesse sich nie wieder
/// zurueckgeben. Gefunden beim ersten echten Nachtrag an MEMORY.md.
fn stand_pfad() -> Result<PathBuf, String> {
    Ok(config::config_dir()?.join("erinnerungen-stand.json"))
}

fn stand_laden() -> BTreeMap<String, String> {
    stand_pfad()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn stand_schreiben(stand: &BTreeMap<String, String>) {
    if let Ok(p) = stand_pfad() {
        if let Ok(text) = serde_json::to_string_pretty(stand) {
            let _ = std::fs::write(p, text);
        }
    }
}

fn pruefsumme(inhalt: &str) -> String {
    use sha2::{Digest, Sha256};

    format!("{:x}", Sha256::digest(inhalt.as_bytes()))
}

/// Was lokal liegt: Kennung → Inhalt.
fn lokal(pfad: &Path) -> BTreeMap<String, String> {
    let mut aus = BTreeMap::new();

    let Ok(eintraege) = std::fs::read_dir(pfad) else {
        return aus;
    };

    for e in eintraege.filter_map(Result::ok) {
        let p = e.path();

        if p.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }

        let Some(name) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };

        if let Ok(text) = std::fs::read_to_string(&p) {
            aus.insert(name.to_string(), text);
        }
    }

    aus
}

fn holen(connection: &Connection, projekt: Option<&str>) -> Result<Vec<Erinnerung>, String> {
    let token = mcp::access_token(connection)?;
    let mut nutzlast = serde_json::json!({});

    if let Some(p) = projekt {
        nutzlast["projekt"] = serde_json::json!(p);
    }

    let roh = mcp::call_tool(connection, &token, "list-harness-memories-tool", nutzlast)?;

    let antwort: Antwort = serde_json::from_value(roh)
        .map_err(|e| format!("Antwort auf list-harness-memories-tool ist unlesbar: {e}"))?;

    Ok(antwort.erinnerungen)
}

fn schicken(
    connection: &Connection,
    name: &str,
    inhalt: &str,
    projekt: Option<&str>,
) -> Result<String, String> {
    let token = mcp::access_token(connection)?;
    let (art, beschreibung) = vorspann(inhalt);

    let mut nutzlast = serde_json::json!({
        "name": name,
        "inhalt": inhalt,
        "art": if art.is_empty() { "reference".to_string() } else { art },
    });

    if !beschreibung.is_empty() {
        nutzlast["beschreibung"] = serde_json::json!(beschreibung);
    }

    if let Some(p) = projekt {
        nutzlast["projekt"] = serde_json::json!(p);
    }

    let roh = mcp::call_tool(connection, &token, "sync-harness-memory-tool", nutzlast)?;

    Ok(roh
        .get("status")
        .and_then(|s| s.as_str())
        .unwrap_or("übertragen")
        .to_string())
}

/// `erinnerungen [--projekt X] [--verzeichnis P] [--apply]`
pub fn abgleichen(args: &[String]) -> Result<(), String> {
    let wert = |n: &str| -> Option<String> {
        args.iter()
            .position(|a| a == n)
            .and_then(|i| args.get(i + 1))
            .filter(|w| !w.starts_with("--"))
            .cloned()
    };

    let anwenden = args.iter().any(|a| a == "--apply");

    // Zwei ausdrueckliche Richtungen statt eines mehrdeutigen `--force`: Bei
    // einem Konflikt muss jemand SAGEN, welche Seite gilt. Ein Schalter, der
    // „ueberschreiben" heisst, laesst offen, was womit — und genau daraus
    // entsteht der Verlust, den er verhindern soll.
    let hier_gewinnt = args.iter().any(|a| a == "--hier-gewinnt");
    let dort_gewinnt = args.iter().any(|a| a == "--dort-gewinnt");

    if hier_gewinnt && dort_gewinnt {
        return Err("`--hier-gewinnt` und `--dort-gewinnt` zugleich geht nicht.".to_string());
    }

    let verzeichnis = match wert("--verzeichnis") {
        Some(p) => crate::freigabe::aufloesen(&p)?,
        None => std::env::current_dir().map_err(|e| format!("Arbeitsverzeichnis unlesbar: {e}"))?,
    };

    let projekt = wert("--projekt").or_else(|| crate::chat::projekt_aus_claude_md(&verzeichnis));

    let pfad = gedaechtnis_pfad(&verzeichnis)
        .ok_or_else(|| "HOME ist nicht gesetzt — der Ort des Gedächtnisses ist unbekannt.".to_string())?;

    let connection = config::load()?;
    let hier = lokal(&pfad);
    let dort: BTreeMap<String, Erinnerung> = holen(&connection, projekt.as_deref())?
        .into_iter()
        .map(|e| (e.name.clone(), e))
        .collect();

    let mut neu_hier: Vec<String> = Vec::new();
    let mut neu_dort: Vec<String> = Vec::new();
    let mut konflikt = Vec::new();
    let mut gleich: Vec<String> = Vec::new();

    let stand = stand_laden();

    // Drei Seiten, nicht zwei: hier, dort, und was beim letzten Mal galt. Nur
    // so laesst sich „ich habe geaendert" von „die andere Seite ist aelter"
    // unterscheiden.
    for (name, inhalt) in &hier {
        let meine = pruefsumme(inhalt);

        match dort.get(name) {
            None => neu_dort.push(name.clone()),
            Some(d) if d.pruefsumme == meine => gleich.push(name.clone()),
            Some(d) => match stand.get(name) {
                // Dort steht noch der Stand von damals, hier nicht mehr:
                // hier wurde geaendert.
                Some(alt) if *alt == d.pruefsumme => neu_dort.push(name.clone()),
                // Hier steht der Stand von damals, dort nicht mehr:
                // dort wurde geaendert.
                Some(alt) if *alt == meine => neu_hier.push(name.clone()),
                // Beide weichen vom letzten Stand ab — oder es gibt keinen.
                _ => konflikt.push(name.clone()),
            },
        }
    }

    for name in dort.keys() {
        if !hier.contains_key(name) {
            neu_hier.push(name.clone());
        }
    }

    println!(
        "Gedächtnis {} · {}",
        projekt.as_deref().unwrap_or("(ohne Projekt)"),
        pfad.display()
    );
    println!();
    println!("  hier, noch nicht dort : {}", neu_dort.len());
    println!("  dort, noch nicht hier : {}", neu_hier.len());
    println!("  beidseitig geändert   : {}", konflikt.len());
    println!("  gleich                : {}", gleich.len());

    if !neu_hier.is_empty() {
        println!();
        println!("Kommt von AI Brain:");

        for n in &neu_hier {
            let Some(e) = dort.get(n) else { continue };
            let wo = if e.projekt.is_empty() { "überall" } else { &e.projekt };

            println!("  + {n}  \x1b[2m{} · {wo}\x1b[0m", e.art);
        }
    }

    if !konflikt.is_empty() {
        println!();

        if hier_gewinnt {
            println!("Beidseitig geändert — diese Seite gilt, geht nach AI Brain:");
        } else if dort_gewinnt {
            println!("Beidseitig geändert — AI Brain gilt, wird hier überschrieben:");
        } else {
            println!("Beidseitig geändert — wird nicht angefasst:");
        }

        for n in &konflikt {
            println!("  - {n}");
        }

        if !hier_gewinnt && !dort_gewinnt {
            println!();
            println!("  Entscheiden mit --hier-gewinnt oder --dort-gewinnt.");
        }
    }

    if !anwenden {
        println!();
        println!("Nichts geschrieben. Mit --apply anwenden.");

        return Ok(());
    }

    let mut gescheitert: Vec<(String, String)> = Vec::new();

    if dort_gewinnt {
        // Die hiesige Fassung wird gesichert, bevor sie weicht — dieselbe
        // Vorsicht wie bei `sync --force`.
        for n in &konflikt {
            if let Some(inhalt) = hier.get(n) {
                let sicherung = pfad.join(format!("{n}.vorher.md"));
                let _ = std::fs::write(sicherung, inhalt);
            }
        }

        neu_hier.extend(konflikt.iter().cloned());
    }

    if hier_gewinnt {
        neu_dort.extend(konflikt.iter().cloned());
    }

    std::fs::create_dir_all(&pfad)
        .map_err(|e| format!("{} lässt sich nicht anlegen: {e}", pfad.display()))?;

    for name in &neu_hier {
        let Some(e) = dort.get(name) else { continue };
        let ziel = pfad.join(format!("{name}.md"));

        std::fs::write(&ziel, &e.inhalt)
            .map_err(|err| format!("{} nicht schreibbar: {err}", ziel.display()))?;
    }

    for name in &neu_dort {
        let Some(inhalt) = hier.get(name) else { continue };
        // Ein Eintrag OHNE Projektbezug gehoert dorthin, wo er ueberall gilt —
        // das entscheidet die Art, nicht das Verzeichnis. `user` und
        // `feedback` gelten fuer die Person, nicht fuer ein Projekt.
        let (art, _) = vorspann(inhalt);
        let ziel_projekt = match art.as_str() {
            "user" | "feedback" => None,
            _ => projekt.as_deref(),
        };

        // Ein Eintrag, der nicht durchgeht, darf die uebrigen nicht mitnehmen.
        // Beim ersten Lauf war MEMORY.md zu gross — weil es alphabetisch zuerst
        // kommt, brach der ganze Abgleich ab und keine einzige der 138
        // Erinnerungen kam an.
        if let Err(e) = schicken(&connection, name, inhalt, ziel_projekt) {
            gescheitert.push((name.clone(), e));
        }
    }

    // Den Stand erst NACH dem Schreiben fortschreiben, und nur fuer das, was
    // wirklich durchging — sonst merkt sich der Rechner einen Abgleich, den es
    // nicht gab, und haelt den naechsten Unterschied faelschlich fuer einen
    // Konflikt.
    let mut neuer_stand = stand.clone();

    // AUCH das, was schon gleich war. Der erste Entwurf merkte sich nur das
    // Uebertragene — damit hatte die Mehrheit nie einen Stand, und die naechste
    // lokale Aenderung sah wieder wie ein Konflikt aus. Gefunden mit einer
    // Probe: eine Zeile angehaengt, und der Eintrag galt als beidseitig
    // geaendert, obwohl nur hier etwas passiert war.
    for name in &gleich {
        if let Some(inhalt) = hier.get(name) {
            neuer_stand.insert(name.clone(), pruefsumme(inhalt));
        }
    }

    for name in &neu_hier {
        if let Some(e) = dort.get(name) {
            neuer_stand.insert(name.clone(), e.pruefsumme.clone());
        }
    }

    let fehlgeschlagen: Vec<&String> = gescheitert.iter().map(|(n, _)| n).collect();

    for name in &neu_dort {
        if fehlgeschlagen.contains(&name) {
            continue;
        }

        if let Some(inhalt) = hier.get(name) {
            neuer_stand.insert(name.clone(), pruefsumme(inhalt));
        }
    }

    stand_schreiben(&neuer_stand);

    println!();
    println!(
        "{} geholt, {} zurückgegeben.",
        neu_hier.len(),
        neu_dort.len() - gescheitert.len()
    );

    if !gescheitert.is_empty() {
        println!();
        println!("Nicht übertragen ({}):", gescheitert.len());

        for (name, grund) in &gescheitert {
            println!("  - {name}: {grund}");
        }

        return Err(format!(
            "{} von {} Einträgen sind nicht angekommen — siehe oben.",
            gescheitert.len(),
            neu_dort.len()
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{gedaechtnis_pfad, vorspann};
    use std::path::Path;

    #[test]
    fn der_ort_folgt_claudes_regel() {
        std::env::set_var("HOME", "/home/x");

        assert_eq!(
            gedaechtnis_pfad(Path::new("/home/x/Development/ai-brain")),
            Some(Path::new("/home/x/.claude/projects/-home-x-Development-ai-brain/memory").to_path_buf())
        );
    }

    #[test]
    fn vorspann_liest_art_und_beschreibung() {
        let t = "---\nname: Titel\ndescription: eine Zeile\ntype: feedback\n---\n\nText\n";

        assert_eq!(vorspann(t), ("feedback".into(), "eine Zeile".into()));
    }

    #[test]
    fn was_hinter_dem_vorspann_steht_zaehlt_nicht() {
        // Sonst wuerde ein Codebeispiel im Text die Art ueberschreiben — und
        // der Eintrag landete in der falschen Reichweite.
        let t = "---\ntype: user\n---\n\ntype: feedback\ndescription: aus dem Text\n";

        assert_eq!(vorspann(t), ("user".into(), String::new()));
    }

    #[test]
    fn ohne_vorspann_bleibt_es_leer() {
        assert_eq!(vorspann("# Nur Text\n"), (String::new(), String::new()));
    }
}

#[cfg(test)]
mod dreiseitig {
    use std::collections::BTreeMap;

    /// Die Entscheidung aus den drei Pruefsummen, ohne Platte und Netz.
    ///
    /// Eigene Funktion, damit die Regel pruefbar ist: Der erste Entwurf
    /// verglich nur hier gegen dort und erklaerte jede lokale Aenderung zum
    /// Konflikt — der Index liess sich danach nie wieder zurueckgeben.
    fn entscheidung(hier: &str, dort: Option<&str>, zuletzt: Option<&str>) -> &'static str {
        match dort {
            None => "zurueckgeben",
            Some(d) if d == hier => "gleich",
            Some(d) => match zuletzt {
                Some(z) if z == d => "zurueckgeben",
                Some(z) if z == hier => "holen",
                _ => "konflikt",
            },
        }
    }

    #[test]
    fn nur_hier_geaendert_heisst_zurueckgeben() {
        assert_eq!(entscheidung("neu", Some("alt"), Some("alt")), "zurueckgeben");
    }

    #[test]
    fn nur_dort_geaendert_heisst_holen() {
        assert_eq!(entscheidung("alt", Some("neu"), Some("alt")), "holen");
    }

    #[test]
    fn beidseitig_geaendert_bleibt_ein_konflikt() {
        assert_eq!(entscheidung("meins", Some("deins"), Some("alt")), "konflikt");
    }

    #[test]
    fn ohne_gemerkten_stand_lieber_konflikt_als_ueberschreiben() {
        // Der erste Abgleich auf einem Rechner kennt keinen Stand. Dann ist
        // Nichtstun richtig — eine falsche Richtung waere hier teuer.
        assert_eq!(entscheidung("meins", Some("deins"), None), "konflikt");
    }

    #[test]
    fn gleich_bleibt_gleich_egal_was_zuletzt_galt() {
        assert_eq!(entscheidung("x", Some("x"), Some("ganz alt")), "gleich");
        assert_eq!(entscheidung("x", Some("x"), None), "gleich");
    }

    #[test]
    fn was_es_dort_nicht_gibt_geht_hin() {
        let leer: BTreeMap<String, String> = BTreeMap::new();

        assert!(leer.is_empty());
        assert_eq!(entscheidung("neu", None, None), "zurueckgeben");
    }
}
