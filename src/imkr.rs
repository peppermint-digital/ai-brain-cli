//! Läufe in einem imkr-Pane (#7667).
//!
//! **Warum überhaupt.** Ein beaufsichtigter Lauf (`agent` ohne `--imkr`) ist
//! ein Kindprozess: Die CLI hält ihn, schreibt sein Protokoll und stempelt
//! seinen Herzschlag. Das ist richtig für unbeaufsichtigte Arbeit — aber man
//! kann ihm nicht zusehen und nichts hineinrufen.
//!
//! Läuft der Agent in einem imkr-Pane, bekommt man genau das: zusehen
//! (`imkr read`), hineinrufen (`imkr msg`), Zustand aus den Hooks
//! (`imkr status`), und die Übersicht über alle Rechner (`imkr agents --all`).
//! Das baut die CLI **nicht** nach.
//!
//! **Die Folge für das Register: Es gibt niemanden, der stempelt.** Ein
//! Pane-Lauf hat keinen Aufseher, also auch keinen Herzschlag — die
//! Verschwunden-Regel des Registers würde ihn nach drei Minuten als `fehler`
//! abstempeln, obwohl er arbeitet. Deshalb fragt das Register bei Läufen mit
//! Pane-Adresse **imkr** nach dem Zustand, statt auf einen Stempel zu warten.
//!
//! **imkr bleibt optional.** Ohne imkr fehlt nichts als diese Betriebsart; die
//! CLI ist autark. Und `imkr new` startet keine Sitzung — es braucht eine
//! laufende. Wer ausserhalb von imkr `--imkr` verlangt, bekommt das gesagt,
//! statt in „no imkr server running" zu laufen.

use std::path::Path;
use std::process::{Command, Stdio};

/// Ist imkr auf diesem Rechner aufrufbar?
pub fn verfuegbar() -> bool {
    Command::new("imkr")
        .arg("version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Die Sitzung, in der dieses Programm läuft — falls es in imkr läuft.
///
/// `imkr new` wirkt immer auf eine Sitzung. Von aussen gibt es keine, und imkr
/// startet auch keine: Ein Pane in einer Sitzung, die es nicht gibt, ist kein
/// sinnvoller Wunsch.
pub fn sitzung() -> Option<String> {
    std::env::var("IMKR_SESSION").ok().filter(|s| !s.is_empty())
}

/// Startet ein Programm in einem neuen imkr-Fenster und gibt die Adresse zurück.
///
/// `--window 0` ist Absicht: ein neues **Fenster**, nicht ein Teilen des
/// Fensters, in dem gerade jemand arbeitet.
///
/// `imkr new` schreibt die Pane-Nummer auf die Ausgabe. Gelesen wird die letzte
/// Zeile, die nur aus einer Zahl besteht — nicht blind die ganze Ausgabe, damit
/// ein künftiger Hinweis davor nichts kaputt macht.
pub fn pane_starten(
    verzeichnis: &Path,
    programm: &str,
    args: &[String],
) -> Result<String, String> {
    let sitzung = sitzung().ok_or_else(|| {
        "`--imkr` geht nur aus einer imkr-Sitzung heraus (IMKR_SESSION ist nicht gesetzt).\n\
         `imkr new` startet keine Sitzung, es braucht eine laufende.\n\
         Ohne `--imkr` läuft derselbe Auftrag als beaufsichtigter Lauf mit Protokoll."
            .to_string()
    })?;

    let mut befehl = Command::new("imkr");
    befehl
        .arg("new")
        .arg("--window")
        .arg("0")
        .arg("--dir")
        .arg(verzeichnis)
        .arg("--")
        .arg(programm)
        .args(args);

    let ausgang = befehl
        .output()
        .map_err(|e| format!("`imkr new` liess sich nicht starten: {e}"))?;

    if !ausgang.status.success() {
        return Err(format!(
            "`imkr new` scheiterte: {}",
            String::from_utf8_lossy(&ausgang.stderr).trim()
        ));
    }

    let pane = pane_aus_ausgabe(&String::from_utf8_lossy(&ausgang.stdout)).ok_or_else(|| {
        format!(
            "`imkr new` hat keine Pane-Nummer genannt: {}",
            String::from_utf8_lossy(&ausgang.stdout).trim()
        )
    })?;

    Ok(format!("{sitzung}/{pane}"))
}

/// Die Pane-Nummer aus der Ausgabe von `imkr new`.
pub fn pane_aus_ausgabe(ausgabe: &str) -> Option<u32> {
    ausgabe
        .lines()
        .rev()
        .find_map(|z| z.trim().parse::<u32>().ok())
}

/// Zustand eines Panes, wie imkr ihn sieht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneZustand {
    Arbeitet,
    Wartet,
    Ruht,
    Weg,
}

/// Übersetzt imkrs Zustandswörter.
///
/// `idle` und `done` werden zu `Ruht`: Für einen Auftrag heisst „der Agent
/// arbeitet nicht mehr" genau, dass sein Zug vorbei ist. Das Pane lebt
/// weiter — der Mensch kann dort weiterreden, und das ist der Sinn dieser
/// Betriebsart.
pub fn zustand_aus_wort(wort: &str) -> PaneZustand {
    match wort {
        "working" => PaneZustand::Arbeitet,
        "blocked" => PaneZustand::Wartet,
        "idle" | "done" | "clear" => PaneZustand::Ruht,
        _ => PaneZustand::Weg,
    }
}

/// Findet den Zustand einer Adresse in der JSON-Ausgabe von `imkr agents`.
///
/// Reine Funktion, damit sie ohne laufendes imkr prüfbar ist — und weil hier
/// die eigentliche Fehlerquelle sitzt: Adressen haben die Form
/// `rechner:sitzung/pane`, gespeichert ist `sitzung/pane`.
pub fn zustand_aus_json(json: &str, adresse: &str) -> PaneZustand {
    let Ok(liste) = serde_json::from_str::<Vec<serde_json::Value>>(json) else {
        return PaneZustand::Weg;
    };

    for eintrag in liste {
        let Some(voll) = eintrag.get("address").and_then(|a| a.as_str()) else {
            continue;
        };

        // `codingmachine:cli-tool/2` endet auf `cli-tool/2`. Verglichen wird
        // auf den Schrägstrich genau, damit `tool/2` nicht auf `cli-tool/2`
        // passt — dieselbe Falle wie bei den Verzeichnis-Freigaben.
        let passt = voll == adresse
            || voll
                .strip_suffix(adresse)
                .is_some_and(|rest| rest.ends_with(':'));

        if passt {
            return eintrag
                .get("state")
                .and_then(|s| s.as_str())
                .map(zustand_aus_wort)
                .unwrap_or(PaneZustand::Weg);
        }
    }

    PaneZustand::Weg
}

/// Fragt imkr nach dem Zustand eines Panes.
///
/// **Ohne `--all`, und das ist wichtig.** Gespeichert wird die lokale Adresse
/// `sitzung/pane` ohne Rechnernamen; der Vergleich ist deshalb ein
/// Suffix-Vergleich. Enthielte die Liste auch fremde Rechner, könnte ein
/// gleichnamiges Pane dort den Zustand liefern. Wer hier `--all` ergänzt, muss
/// vorher die volle Adresse speichern — ein Test hält diese Grenze fest.
pub fn zustand(adresse: &str) -> PaneZustand {
    let Ok(ausgang) = Command::new("imkr").args(["agents", "--json"]).output() else {
        return PaneZustand::Weg;
    };

    if !ausgang.status.success() {
        return PaneZustand::Weg;
    }

    zustand_aus_json(&String::from_utf8_lossy(&ausgang.stdout), adresse)
}

/// Was auf dem Schirm eines Panes steht.
pub fn schirm(adresse: &str, zeilen: usize) -> Result<String, String> {
    let ausgang = Command::new("imkr")
        .args(["read", adresse, "--lines", &zeilen.to_string()])
        .output()
        .map_err(|e| format!("`imkr read` liess sich nicht starten: {e}"))?;

    if !ausgang.status.success() {
        return Err(format!(
            "`imkr read {adresse}` scheiterte: {}",
            String::from_utf8_lossy(&ausgang.stderr).trim()
        ));
    }

    Ok(String::from_utf8_lossy(&ausgang.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_nummer_aus_der_ausgabe() {
        assert_eq!(pane_aus_ausgabe("2\n"), Some(2));
        assert_eq!(pane_aus_ausgabe("2"), Some(2));

        // Ein Hinweis davor darf nicht stören — gelesen wird von hinten.
        assert_eq!(pane_aus_ausgabe("Hinweis: irgendwas\n7\n"), Some(7));

        assert_eq!(pane_aus_ausgabe(""), None);
        assert_eq!(pane_aus_ausgabe("kein Pane\n"), None);
    }

    #[test]
    fn zustand_wird_ueber_die_adresse_gefunden() {
        let json = r#"[
            {"address":"codingmachine:ai-brain/0","state":"working"},
            {"address":"codingmachine:cli-tool/2","state":"blocked"},
            {"address":"anderer:cli-tool/9","state":"idle"}
        ]"#;

        assert_eq!(zustand_aus_json(json, "cli-tool/2"), PaneZustand::Wartet);
        assert_eq!(zustand_aus_json(json, "ai-brain/0"), PaneZustand::Arbeitet);

        // Festgehalten, weil es eine Grenze ist: Eine gespeicherte Adresse ohne
        // Rechnernamen passt auch auf ein gleichnamiges Pane auf einem ANDEREN
        // Rechner. Unbedenklich ist das nur, weil `zustand()` `imkr agents`
        // OHNE `--all` fragt — die Liste enthält dann nur diesen Rechner.
        // Wer hier je `--all` einbaut, muss vorher die volle Adresse speichern.
        assert_eq!(
            zustand_aus_json(json, "cli-tool/9"),
            PaneZustand::Ruht,
            "Suffix-Vergleich trifft auch fremde Rechner — gilt nur ohne --all"
        );

        // Volle Adresse geht auch.
        assert_eq!(
            zustand_aus_json(json, "codingmachine:cli-tool/2"),
            PaneZustand::Wartet
        );

        // Die Falle: ein Teilstück darf nicht passen. `tool/2` ist nicht
        // `cli-tool/2` — sonst zeigte ein Lauf den Zustand eines fremden Panes.
        assert_eq!(zustand_aus_json(json, "tool/2"), PaneZustand::Weg);

        // Unbekanntes und Unsinn: weg, nicht geraten.
        assert_eq!(zustand_aus_json(json, "gibt-es-nicht/0"), PaneZustand::Weg);
        assert_eq!(zustand_aus_json("kein json", "cli-tool/2"), PaneZustand::Weg);
        assert_eq!(zustand_aus_json("[]", "cli-tool/2"), PaneZustand::Weg);
    }

    #[test]
    fn zustandswoerter() {
        assert_eq!(zustand_aus_wort("working"), PaneZustand::Arbeitet);
        assert_eq!(zustand_aus_wort("blocked"), PaneZustand::Wartet);
        assert_eq!(zustand_aus_wort("idle"), PaneZustand::Ruht);
        assert_eq!(zustand_aus_wort("done"), PaneZustand::Ruht);
        assert_eq!(zustand_aus_wort("etwas-neues"), PaneZustand::Weg);
    }
}
