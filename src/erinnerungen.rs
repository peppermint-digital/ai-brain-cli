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

    let mut neu_hier = Vec::new();
    let mut neu_dort = Vec::new();
    let mut konflikt = Vec::new();
    let mut gleich = 0usize;

    for (name, inhalt) in &hier {
        match dort.get(name) {
            None => neu_dort.push(name.clone()),
            Some(d) if d.pruefsumme == pruefsumme(inhalt) => gleich += 1,
            Some(_) => konflikt.push(name.clone()),
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
    println!("  gleich                : {gleich}");

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
        println!("Beidseitig geändert — wird nicht angefasst:");

        for n in &konflikt {
            println!("  - {n}");
        }
    }

    if !anwenden {
        println!();
        println!("Nichts geschrieben. Mit --apply anwenden.");

        return Ok(());
    }

    let mut gescheitert: Vec<(String, String)> = Vec::new();

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
