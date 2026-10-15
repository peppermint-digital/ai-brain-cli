//! Der Abholer als Taktgeber (#7814).
//!
//! **Ohne Abholer ist ein Dauerauftrag dasselbe wie ein pausierter Wächter.**
//! AI Brain kennt Takt und Frist, aber niemand holt ab — und das sieht genauso
//! aus wie Ruhe. Genau dieser Fall liess Hermes' Backup-Wächter dreieinhalb
//! Monate untätig in der Liste stehen.
//!
//! ## Nutzer-Units, kein Systemdienst
//!
//! Der Abholer arbeitet mit den Rechten des angemeldeten Menschen und holt
//! seine Aufträge mit dessen Zugang. Ein Systemdienst müsste als `root` laufen
//! und sich die Identität borgen — mehr Rechte für weniger Klarheit. Nutzer-Units
//! brauchen kein `sudo`, und der Rechner hier hat `Linger` schon an (sonst
//! liefen sie nur, solange jemand angemeldet ist; davor wird gewarnt).
//!
//! ## Dass der Taktgeber läuft, prüft die Gegenseite
//!
//! Dieses Modul richtet ihn ein, überwacht sich aber nicht selbst — das wäre
//! derselbe Zirkel wie bei Hermes. Bleibt der Abholer stehen, kommen keine
//! Meldungen, und die Stillfrist in AI Brain schlägt Alarm. Der Wächter über
//! den Wächter sitzt also auf der anderen Seite.

use std::path::PathBuf;
use std::process::Command;

const DIENST: &str = "ai-brain-daueraufgaben.service";
const TIMER: &str = "ai-brain-daueraufgaben.timer";

fn unit_verzeichnis() -> Result<PathBuf, String> {
    let heimat = std::env::var("HOME").map_err(|_| "HOME ist nicht gesetzt.".to_string())?;

    Ok(PathBuf::from(heimat).join(".config/systemd/user"))
}

/// Die Unit-Dateien, wie sie geschrieben werden.
///
/// `Persistent=true` ist wichtig: War der Rechner aus, soll der Abholer nach
/// dem Hochfahren nachholen statt den Takt zu überspringen. Ohne das fiele ein
/// Lauf aus, ohne dass es auffällt — und das ist genau die Sorte Lücke, gegen
/// die Dauerauftraege gebaut sind.
pub fn units(programm: &str, takt: &str) -> (String, String) {
    let dienst = format!(
        "[Unit]\n\
         Description=AI Brain Harness — Dauerauftraege abholen und fahren\n\
         After=network-online.target\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={programm} daueraufgaben --fahren --still\n"
    );

    let timer = format!(
        "[Unit]\n\
         Description=AI Brain Harness — Taktgeber fuer Dauerauftraege\n\
         \n\
         [Timer]\n\
         OnCalendar={takt}\n\
         Persistent=true\n\
         RandomizedDelaySec=30\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n"
    );

    (dienst, timer)
}

/// `taktgeber [--einrichten|--entfernen|--zustand]`
pub fn verwalten(args: &[String]) -> Result<(), String> {
    let hat = |n: &str| args.iter().any(|a| a == n);

    if hat("--entfernen") {
        return entfernen();
    }

    if hat("--einrichten") {
        let takt = args
            .iter()
            .position(|a| a == "--takt")
            .and_then(|i| args.get(i + 1))
            .filter(|w| !w.starts_with("--"))
            .cloned()
            // Alle fünf Minuten: feiner als jeder sinnvolle Takt eines
            // Dauerauftrags, und ein Lauf ohne Fälliges kostet eine Abfrage.
            .unwrap_or_else(|| "*-*-* *:00/5:00".to_string());

        return einrichten(&takt);
    }

    zustand()
}

fn einrichten(takt: &str) -> Result<(), String> {
    let verzeichnis = unit_verzeichnis()?;

    std::fs::create_dir_all(&verzeichnis)
        .map_err(|e| format!("{} lässt sich nicht anlegen: {e}", verzeichnis.display()))?;

    let programm = std::env::current_exe()
        .map_err(|e| format!("Eigener Pfad nicht ermittelbar: {e}"))?
        .display()
        .to_string();

    let (dienst, timer) = units(&programm, takt);

    std::fs::write(verzeichnis.join(DIENST), &dienst)
        .map_err(|e| format!("{DIENST} nicht schreibbar: {e}"))?;
    std::fs::write(verzeichnis.join(TIMER), &timer)
        .map_err(|e| format!("{TIMER} nicht schreibbar: {e}"))?;

    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", TIMER])?;

    println!("Taktgeber eingerichtet.");
    println!();
    println!("  Dienst   {}", verzeichnis.join(DIENST).display());
    println!("  Takt     {takt}");
    println!("  Befehl   {programm} daueraufgaben --fahren --still");
    println!();

    // Ohne Lingering läuft der Timer nur, solange jemand angemeldet ist — und
    // ein Abholer, der mit der Abmeldung stehenbleibt, ist genau die stille
    // Lücke, gegen die Dauerauftraege gebaut sind.
    match lingering() {
        Some(true) => println!("  Lingering ist an — der Taktgeber läuft auch ohne angemeldete Sitzung."),
        Some(false) => {
            println!("  \x1b[33mLingering ist AUS.\x1b[0m Der Taktgeber läuft dann nur, solange");
            println!("  jemand angemeldet ist. Einschalten:");
            println!("    sudo loginctl enable-linger {}", std::env::var("USER").unwrap_or_default());
        }
        None => println!("  Lingering liess sich nicht feststellen — bitte nachsehen."),
    }

    println!();
    println!("Zustand ansehen:  ai-brain-cli taktgeber");

    Ok(())
}

fn entfernen() -> Result<(), String> {
    let verzeichnis = unit_verzeichnis()?;

    // Erst abschalten, dann löschen. Andersherum bleibt eine Unit aktiviert,
    // deren Datei fehlt, und systemd meckert bei jedem `daemon-reload`.
    let _ = systemctl(&["disable", "--now", TIMER]);

    for datei in [DIENST, TIMER] {
        let pfad = verzeichnis.join(datei);

        if pfad.exists() {
            std::fs::remove_file(&pfad)
                .map_err(|e| format!("{} nicht löschbar: {e}", pfad.display()))?;
        }
    }

    systemctl(&["daemon-reload"])?;

    println!("Taktgeber entfernt.");
    println!();
    println!("Achtung: Ohne Abholer melden die Daueraufträge nicht mehr. AI Brain");
    println!("schlägt nach ihrer Stillfrist Alarm — das ist gewollt, aber es kommt.");
    println!("Wer sie wirklich nicht mehr braucht, schaltet sie dort ab.");

    Ok(())
}

fn zustand() -> Result<(), String> {
    let verzeichnis = unit_verzeichnis()?;

    if !verzeichnis.join(TIMER).exists() {
        println!("Kein Taktgeber eingerichtet.");
        println!();
        println!("Ohne ihn holt niemand die Daueraufträge dieses Rechners ab — und");
        println!("das sieht aus wie Ruhe, bis die Stillfrist zuschlägt.");
        println!();
        println!("Einrichten:  ai-brain-cli taktgeber --einrichten");

        return Ok(());
    }

    let aktiv = ausgabe(&["is-active", TIMER]).unwrap_or_else(|| "unbekannt".into());

    println!("Taktgeber: {aktiv}");
    println!();

    if let Some(naechster) = ausgabe(&["list-timers", "--no-legend", "--no-pager", TIMER]) {
        if !naechster.trim().is_empty() {
            println!("  {}", naechster.trim());
            println!();
        }
    }

    if let Some(letzter) = ausgabe(&[
        "show", DIENST, "-p", "Result", "-p", "ExecMainStatus", "-p", "ActiveEnterTimestamp",
        "--value",
    ]) {
        let werte: Vec<&str> = letzter.lines().map(str::trim).collect();

        println!("  Letzter Lauf   {}", werte.get(2).copied().unwrap_or("—"));
        println!("  Ergebnis       {}", werte.first().copied().unwrap_or("—"));
    }

    if lingering() == Some(false) {
        println!();
        println!("  \x1b[33mLingering ist AUS\x1b[0m — der Taktgeber steht, sobald niemand angemeldet ist.");
    }

    Ok(())
}

fn lingering() -> Option<bool> {
    let nutzer = std::env::var("USER").ok()?;

    let aus = Command::new("loginctl")
        .args(["show-user", &nutzer, "-p", "Linger", "--value"])
        .output()
        .ok()?;

    if !aus.status.success() {
        return None;
    }

    match String::from_utf8_lossy(&aus.stdout).trim() {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

fn systemctl(args: &[&str]) -> Result<(), String> {
    let mut alle = vec!["--user"];
    alle.extend_from_slice(args);

    let aus = Command::new("systemctl")
        .args(&alle)
        .output()
        .map_err(|e| format!("systemctl liess sich nicht starten: {e}"))?;

    if aus.status.success() {
        return Ok(());
    }

    Err(format!(
        "`systemctl {}` scheiterte: {}",
        alle.join(" "),
        String::from_utf8_lossy(&aus.stderr).trim()
    ))
}

fn ausgabe(args: &[&str]) -> Option<String> {
    let mut alle = vec!["--user"];
    alle.extend_from_slice(args);

    let aus = Command::new("systemctl").args(&alle).output().ok()?;

    Some(String::from_utf8_lossy(&aus.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn der_dienst_ruft_den_abholer_still() {
        let (dienst, _) = units("/usr/bin/ai-brain-cli", "*-*-* *:00/5:00");

        assert!(dienst.contains("daueraufgaben --fahren --still"));
        assert!(dienst.contains("Type=oneshot"));
    }

    #[test]
    fn der_timer_holt_versaeumte_laeufe_nach() {
        // Ohne `Persistent=true` faellt der Lauf nach einem Neustart aus —
        // und ein ausgefallener Lauf ist genau die stille Luecke.
        let (_, timer) = units("/x", "*-*-* *:00/5:00");

        assert!(timer.contains("Persistent=true"));
        assert!(timer.contains("OnCalendar=*-*-* *:00/5:00"));
        assert!(timer.contains("WantedBy=timers.target"));
    }

    #[test]
    fn der_takt_landet_im_timer_und_nicht_im_dienst() {
        let (dienst, timer) = units("/x", "*-*-* 04:00:00");

        assert!(timer.contains("04:00:00"));
        assert!(!dienst.contains("04:00:00"));
    }
}
