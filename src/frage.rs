//! Rückfragen — und die Regel, wann keine gestellt wird.
//!
//! Der wichtigste Teil ist nicht das Fragen, sondern das Schweigen: Läuft der
//! ai-brain-cli ohne Terminal (Skript, Cron, Agent), darf **nie** gefragt werden.
//! Eine Frage, die niemand sieht, hält den Lauf auf, bis das Zeitlimit greift —
//! und was dann als Fehler gemeldet wird, sieht aus wie ein Ausfall der
//! Instanz, obwohl nur jemand auf ein „j" gewartet hat.
//!
//! Deshalb liegt die Prüfung hier und nicht an jeder Aufrufstelle: Wer eine
//! Frage stellen will, kommt an {@link darf_fragen} nicht vorbei.

use std::io::{IsTerminal, Write};

/// Darf überhaupt gefragt werden?
///
/// `zugestimmt` ist das `--ja` des Aufrufers: Wer es setzt, hat im Voraus
/// geantwortet und will nicht gefragt werden — auch am Terminal nicht.
pub fn darf_fragen(zugestimmt: bool) -> bool {
    !zugestimmt && std::io::stdin().is_terminal()
}

/// Ja/Nein mit ausdrücklicher Vorgabe.
///
/// Die Vorgabe gilt bei blossem Enter und bei Eingabe-Ende (Strg-D). Sie wird
/// im Text angezeigt, damit niemand raten muss, was Enter bedeutet.
pub fn ja_nein(text: &str, vorgabe: bool) -> Result<bool, String> {
    let hinweis = if vorgabe { "[J/n]" } else { "[j/N]" };

    print!("{text} {hinweis} ");
    std::io::stdout()
        .flush()
        .map_err(|e| format!("Ausgabe nicht schreibbar: {e}"))?;

    let mut zeile = String::new();
    if std::io::stdin()
        .read_line(&mut zeile)
        .map_err(|e| format!("Eingabe nicht lesbar: {e}"))?
        == 0
    {
        println!();

        return Ok(vorgabe);
    }

    Ok(match zeile.trim().to_lowercase().as_str() {
        "" => vorgabe,
        "j" | "ja" | "y" | "yes" => true,
        _ => false,
    })
}

/// Auswahl aus mehreren Möglichkeiten, jede durch einen Buchstaben benannt.
///
/// Bei leerer Eingabe oder Eingabe-Ende gilt die Vorgabe. Eine unbekannte
/// Eingabe wird nicht stillschweigend als Vorgabe gelesen, sondern erneut
/// gefragt — sonst führt ein Vertipper zu etwas, das niemand gewollt hat.
pub fn auswahl(text: &str, moeglichkeiten: &[(char, &str)], vorgabe: char) -> Result<char, String> {
    loop {
        println!("{text}");

        for (taste, beschreibung) in moeglichkeiten {
            let marke = if *taste == vorgabe { "*" } else { " " };
            println!("  {marke} {taste}  {beschreibung}");
        }

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
            println!();

            return Ok(vorgabe);
        }

        let eingabe = zeile.trim().to_lowercase();

        if eingabe.is_empty() {
            return Ok(vorgabe);
        }

        if let Some(zeichen) = eingabe.chars().next() {
            if moeglichkeiten.iter().any(|(t, _)| *t == zeichen) {
                return Ok(zeichen);
            }
        }

        println!("Bitte eine der genannten Tasten.");
        println!();
    }
}
