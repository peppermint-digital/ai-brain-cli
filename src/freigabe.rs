//! Welche Verzeichnisse ein Agent mit erweiterten Rechten anfassen darf (#7669).
//!
//! **Was hier geregelt ist und was nicht.** Ein gewöhnlicher Lauf braucht keine
//! Freigabe: Er läuft mit den Vorgaben des Werkzeugs und fragt, bevor er etwas
//! tut — damit ist er nicht gefährlicher als ein Mensch, der dort `claude`
//! tippt. Einen solchen Lauf zu verbieten wäre Theater, und ein Riegel, der am
//! Alltag hängenbleibt, wird abgeschaltet.
//!
//! Gesichert wird genau das, was Sicherung braucht: **Läufe, die nicht mehr
//! fragen.** `--schreibend` und `--vollzugriff` gibt es nur in Verzeichnissen,
//! die ausdrücklich freigegeben sind.
//!
//! **Ohne Freigabedatei gibt es keinen Vollzugriff** — nirgends. Das ist der
//! Punkt, an dem die Voreinstellung zählt: Wäre sie „alles erlaubt, bis jemand
//! einschränkt", hätte der Riegel nie existiert.
//!
//! **Pfade werden aufgelöst, nicht verglichen.** `~/Development/../../etc` ist
//! als Zeichenkette etwas anderes als `/etc`, als Verzeichnis aber dasselbe.
//! Wer Freigaben über Zeichenketten prüft, hat keinen Riegel, sondern eine
//! Schreibweise.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Freigaben {
    /// Aufgelöste absolute Pfade. Unterverzeichnisse gelten mit.
    #[serde(default)]
    pub verzeichnisse: Vec<String>,
}

fn pfad() -> Result<PathBuf, String> {
    Ok(crate::config::config_dir()?.join("freigaben.json"))
}

pub fn laden() -> Freigaben {
    // Fehlt die Datei oder ist sie kaputt, gilt „nichts freigegeben". Ein
    // unlesbarer Riegel darf nicht zu einem offenen Riegel werden.
    let Ok(p) = pfad() else {
        return Freigaben::default();
    };

    let Ok(roh) = std::fs::read_to_string(p) else {
        return Freigaben::default();
    };

    serde_json::from_str(&roh).unwrap_or_default()
}

fn speichern(freigaben: &Freigaben) -> Result<(), String> {
    let p = pfad()?;

    if let Some(ordner) = p.parent() {
        std::fs::create_dir_all(ordner)
            .map_err(|e| format!("{} lässt sich nicht anlegen: {e}", ordner.display()))?;
    }

    let json = serde_json::to_string_pretty(freigaben).map_err(|e| format!("{e}"))?;

    std::fs::write(&p, json).map_err(|e| format!("{} lässt sich nicht schreiben: {e}", p.display()))
}

/// Löst einen Pfad auf, soweit er existiert.
///
/// `canonicalize` verlangt, dass der Pfad existiert — das ist hier erwünscht:
/// Ein Verzeichnis freizugeben, das es nicht gibt, wäre ein Versprechen auf
/// etwas Unbekanntes, und später könnte dort etwas anderes stehen.
pub fn aufloesen(roh: &str) -> Result<PathBuf, String> {
    let erweitert = if let Some(rest) = roh.strip_prefix("~/") {
        crate::config::heimat()?.join(rest)
    } else if roh == "~" {
        crate::config::heimat()?
    } else {
        PathBuf::from(roh)
    };

    std::fs::canonicalize(&erweitert)
        .map_err(|e| format!("{} lässt sich nicht auflösen: {e}", erweitert.display()))
}

/// Liegt `verzeichnis` in einem freigegebenen Baum?
///
/// Reine Funktion über eine Liste, damit sie ohne Dateien prüfbar ist.
pub fn ist_frei_in(verzeichnis: &Path, freigegeben: &[String]) -> bool {
    freigegeben
        .iter()
        .map(PathBuf::from)
        .any(|frei| verzeichnis == frei || verzeichnis.starts_with(&frei))
}

pub fn ist_frei(verzeichnis: &Path) -> bool {
    ist_frei_in(verzeichnis, &laden().verzeichnisse)
}

/// `freigabe` / `freigabe add <pfad>` / `freigabe rm <pfad>`
pub fn verwalten(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None | Some("ls") | Some("list") => {
            zeigen();
            Ok(())
        }
        Some("add") | Some("dazu") => hinzufuegen(&args[1..]),
        Some("rm") | Some("weg") => entfernen(&args[1..]),
        Some(sonst) => Err(format!(
            "Unbekannt: `{sonst}`. Möglich: `freigabe`, `freigabe add <pfad>`, `freigabe rm <pfad>`."
        )),
    }
}

fn zeigen() {
    let freigaben = laden();

    if freigaben.verzeichnisse.is_empty() {
        println!("Keine Verzeichnisse freigegeben.");
        println!();
        println!("Ohne Freigabe laufen Agenten mit den Vorgaben ihres Werkzeugs — sie");
        println!("fragen, bevor sie etwas tun. `--schreibend` und `--vollzugriff`");
        println!("brauchen eine Freigabe:");
        println!();
        println!("  ai-brain-cli freigabe add ~/Development");

        return;
    }

    println!("Freigegeben für `--schreibend` und `--vollzugriff` (mit Unterverzeichnissen):");
    println!();

    for v in &freigaben.verzeichnisse {
        let noch_da = Path::new(v).is_dir();
        println!("  {}{}", v, if noch_da { "" } else { "   (gibt es nicht mehr)" });
    }
}

fn hinzufuegen(args: &[String]) -> Result<(), String> {
    let Some(roh) = args.first() else {
        return Err("Welches Verzeichnis? `ai-brain-cli freigabe add <pfad>`".to_string());
    };

    let pfad = aufloesen(roh)?;

    if !pfad.is_dir() {
        return Err(format!("{} ist kein Verzeichnis.", pfad.display()));
    }

    // Das Heimatverzeichnis oder die Wurzel freizugeben heisst „alles" —
    // einschliesslich `.ssh`, `.config` und der Zugangsdaten dieser CLI. Das
    // darf niemandem nebenbei passieren.
    let heikel = pfad == crate::config::heimat()? || pfad == Path::new("/");

    if heikel && !args.iter().any(|a| a == "--trotzdem") {
        return Err(format!(
            "{} freizugeben heisst: alles, auch ~/.ssh und die Zugangsdaten dieser CLI.\n\
             Gib ein Projektverzeichnis frei statt den ganzen Baum. Wenn du es \
             wirklich willst:  ai-brain-cli freigabe add {} --trotzdem",
            pfad.display(),
            roh
        ));
    }

    let mut freigaben = laden();
    let als_text = pfad.display().to_string();

    if freigaben.verzeichnisse.contains(&als_text) {
        println!("{als_text} war schon freigegeben.");

        return Ok(());
    }

    // Liegt der neue Pfad schon in einem freigegebenen Baum, bringt der
    // Eintrag nichts — und zwei Einträge für dieselbe Sache driften
    // auseinander, sobald einer entfernt wird.
    if ist_frei_in(&pfad, &freigaben.verzeichnisse) {
        println!("{als_text} liegt schon in einer Freigabe — kein zweiter Eintrag nötig.");

        return Ok(());
    }

    freigaben.verzeichnisse.push(als_text.clone());
    freigaben.verzeichnisse.sort();
    speichern(&freigaben)?;

    println!("Freigegeben: {als_text}");

    if heikel {
        println!();
        println!("⚠  Das ist der ganze Baum. `ai-brain-cli freigabe rm` nimmt es zurück.");
    }

    Ok(())
}

fn entfernen(args: &[String]) -> Result<(), String> {
    let Some(roh) = args.first() else {
        return Err("Welches Verzeichnis? `ai-brain-cli freigabe rm <pfad>`".to_string());
    };

    let mut freigaben = laden();

    // Hier NICHT auf Existenz bestehen: Ein Eintrag für ein gelöschtes
    // Verzeichnis muss sich entfernen lassen, sonst bleibt er für immer stehen.
    let gesucht = aufloesen(roh)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| roh.clone());

    let vorher = freigaben.verzeichnisse.len();
    freigaben.verzeichnisse.retain(|v| v != &gesucht);

    if freigaben.verzeichnisse.len() == vorher {
        return Err(format!(
            "{gesucht} war nicht freigegeben. Liste: ai-brain-cli freigabe"
        ));
    }

    speichern(&freigaben)?;
    println!("Freigabe entfernt: {gesucht}");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ist_frei_in_gilt_fuer_unterverzeichnisse_und_nicht_fuer_nachbarn() {
        let frei = vec!["/home/x/Development".to_string()];

        assert!(ist_frei_in(Path::new("/home/x/Development"), &frei));
        assert!(ist_frei_in(Path::new("/home/x/Development/projekt"), &frei));
        assert!(ist_frei_in(
            Path::new("/home/x/Development/projekt/tief/drin"),
            &frei
        ));

        // Der Nachbar mit gemeinsamem Namensanfang darf NICHT mitgelten —
        // würde hier über Zeichenketten verglichen, wäre
        // `/home/x/Development-geheim` frei.
        assert!(!ist_frei_in(Path::new("/home/x/Development-geheim"), &frei));
        assert!(!ist_frei_in(Path::new("/home/x"), &frei));
        assert!(!ist_frei_in(Path::new("/etc"), &frei));

        // Leere Liste heisst: nichts frei. Das ist die Voreinstellung, wenn es
        // keine Freigabedatei gibt.
        assert!(!ist_frei_in(Path::new("/home/x/Development"), &[]));
    }

    #[test]
    fn aufloesen_folgt_punkt_punkt() {
        // Der Fall, der einen Zeichenketten-Vergleich aushebelt: Ein Pfad, der
        // aus einem freigegebenen Baum herausführt, aber so aussieht, als läge
        // er darin.
        let tief = std::env::temp_dir();
        let herum = format!("{}/../../etc", tief.display());

        if let Ok(aufgeloest) = aufloesen(&herum) {
            assert_eq!(
                aufgeloest,
                PathBuf::from("/etc"),
                "`..` muss aufgelöst werden, sonst ist die Freigabe eine Schreibweise"
            );
            assert!(!ist_frei_in(
                &aufgeloest,
                &[tief.display().to_string()]
            ));
        }
    }
}
