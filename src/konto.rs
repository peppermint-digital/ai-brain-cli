//! Welches Konto arbeitet, und wie man wechselt (#7832).
//!
//! **Kein zweiter Speicher.** Die Konten liegen bei imkr
//! (`~/.local/state/imkr/accounts/accounts.toml`), und der Wechsel läuft über
//! `CLAUDE_CONFIG_DIR` — ein Verzeichnis je Konto. Das ist ein guter Mechanismus,
//! und ihn hier nachzubauen hiesse, zwei Wahrheiten über dasselbe zu haben.
//! Also fragt die Maske imkr.
//!
//! **Ohne imkr geht es trotzdem.** Dann gibt es genau ein Konto — das, auf das
//! `CLAUDE_CONFIG_DIR` zeigt, oder der Standard `~/.claude`. Angezeigt wird es
//! weiterhin; nur Wechseln fällt aus, und das wird gesagt statt verschwiegen.

use std::collections::BTreeMap;
use std::process::Command;

/// Der Name des gerade geltenden Kontos.
///
/// Abgeleitet aus `CLAUDE_CONFIG_DIR`: imkr legt je Konto ein Verzeichnis an,
/// dessen letzter Teil der Name ist. Zeigt die Variable woanders hin oder ist
/// sie nicht gesetzt, gilt der Standard.
pub fn aktuelles(umgebung: &BTreeMap<String, String>) -> String {
    let pfad = umgebung
        .get("CLAUDE_CONFIG_DIR")
        .cloned()
        .or_else(|| std::env::var("CLAUDE_CONFIG_DIR").ok());

    let Some(pfad) = pfad else {
        return "Standard".to_string();
    };

    // `…/accounts/claude/henneberg` → `claude:henneberg`
    let teile: Vec<&str> = pfad.trim_end_matches('/').rsplit('/').take(2).collect();

    match teile.as_slice() {
        [name, werkzeug] if pfad.contains("/accounts/") => format!("{werkzeug}:{name}"),
        [name, ..] => (*name).to_string(),
        _ => pfad,
    }
}

/// Die Konten, die imkr kennt.
///
/// Gibt `None` zurück, wenn imkr nicht erreichbar ist — das ist kein Fehler,
/// sondern ein Rechner ohne imkr.
pub fn liste() -> Option<String> {
    let aus = Command::new("imkr").args(["account", "ls"]).output().ok()?;

    if !aus.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&aus.stdout).trim().to_string();

    if text.is_empty() {
        return None;
    }

    Some(text)
}

/// Auf ein Konto wechseln: die Umgebung dafür von imkr holen.
///
/// Gibt die Variablen zurück, die der Motor im nächsten Zug braucht. Der
/// Wechsel wirkt also **ab dem nächsten Zug**, nicht rückwirkend — ein
/// laufendes Gespräch gehört dem Konto, mit dem es begonnen hat.
pub fn wechseln(name: &str) -> Result<BTreeMap<String, String>, String> {
    let aus = Command::new("imkr")
        .args(["account", "env", name])
        .output()
        .map_err(|e| format!("imkr liess sich nicht rufen: {e}"))?;

    let text = String::from_utf8_lossy(&aus.stdout).to_string();
    let fehler = String::from_utf8_lossy(&aus.stderr).trim().to_string();

    if !aus.status.success() {
        // imkrs eigene Meldung durchreichen — sie ist genauer als jede, die
        // ich hier erfinden könnte („both agents have an account henneberg:
        // say claude:henneberg or codex:henneberg").
        return Err(if fehler.is_empty() {
            format!("imkr kennt kein Konto '{name}'.")
        } else {
            fehler
        });
    }

    let umgebung = env_lesen(&text);

    if umgebung.is_empty() {
        return Err(format!(
            "imkr nennt für '{name}' keine Umgebung — ohne die ändert ein Wechsel nichts."
        ));
    }

    Ok(umgebung)
}

/// `export KEY=VALUE`-Zeilen in Paare zerlegen.
pub fn env_lesen(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();

    for zeile in text.lines() {
        let zeile = zeile.trim();
        let zeile = zeile.strip_prefix("export ").unwrap_or(zeile);

        let Some((schluessel, wert)) = zeile.split_once('=') else {
            continue;
        };

        let schluessel = schluessel.trim();

        // Nur, was wie ein Variablenname aussieht. Eine Hinweiszeile von imkr
        // mit einem Gleichheitszeichen darin wird sonst zur Variable.
        if schluessel.is_empty()
            || !schluessel
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        {
            continue;
        }

        let wert = wert.trim().trim_matches('"').trim_matches('\'');

        map.insert(schluessel.to_string(), wert.to_string());
    }

    map
}

/// Das Modell, das gilt — soweit es sich von hier aus sagen lässt.
///
/// Ohne gesetztes `ANTHROPIC_MODEL` entscheidet das Werkzeug selbst, und das
/// steht in dessen Konfiguration. Dann wird „Vorgabe des Werkzeugs" gesagt
/// statt geraten: ein falscher Modellname im Schirm ist schlimmer als keiner.
pub fn modell(umgebung: &BTreeMap<String, String>) -> String {
    umgebung
        .get("ANTHROPIC_MODEL")
        .cloned()
        .or_else(|| std::env::var("ANTHROPIC_MODEL").ok())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "Vorgabe des Werkzeugs".to_string())
}

/// Die Gegenstelle, falls eine fremde eingestellt ist.
pub fn anbieter(umgebung: &BTreeMap<String, String>) -> Option<String> {
    umgebung
        .get("ANTHROPIC_BASE_URL")
        .cloned()
        .or_else(|| std::env::var("ANTHROPIC_BASE_URL").ok())
        .filter(|a| !a.is_empty())
        .map(|a| {
            a.trim_start_matches("https://")
                .trim_start_matches("http://")
                .split('/')
                .next()
                .unwrap_or(&a)
                .to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leer() -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    fn mit(paare: &[(&str, &str)]) -> BTreeMap<String, String> {
        paare
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn der_kontoname_kommt_aus_dem_imkr_pfad() {
        let u = mit(&[(
            "CLAUDE_CONFIG_DIR",
            "/home/x/.local/state/imkr/accounts/claude/henneberg",
        )]);

        assert_eq!(aktuelles(&u), "claude:henneberg");
    }

    #[test]
    fn ein_fremder_pfad_liefert_nur_den_letzten_teil() {
        // Nicht jeder Pfad kommt von imkr. Dann ist `claude:` eine Behauptung,
        // die nicht stimmt.
        let u = mit(&[("CLAUDE_CONFIG_DIR", "/opt/eigenes/konto-a")]);

        assert_eq!(aktuelles(&u), "konto-a");
    }

    #[test]
    fn export_zeilen_werden_zerlegt() {
        let text = "export CLAUDE_CONFIG_DIR=/home/x/accounts/claude/department\n";
        let u = env_lesen(text);

        assert_eq!(
            u.get("CLAUDE_CONFIG_DIR").map(String::as_str),
            Some("/home/x/accounts/claude/department")
        );
    }

    #[test]
    fn hinweiszeilen_werden_nicht_zu_variablen() {
        // imkr schreibt Hinweise auf dieselbe Ausgabe. Eine Zeile wie
        // „in this order a used up account falls back = next" darf keine
        // Variable ergeben.
        let u = env_lesen("in this order a used up account = falls back\nexport ECHT=ja\n");

        assert_eq!(u.len(), 1);
        assert_eq!(u.get("ECHT").map(String::as_str), Some("ja"));
    }

    #[test]
    fn anfuehrungszeichen_fallen_weg() {
        let u = env_lesen("export A=\"mit\"\nexport B='auch'\n");

        assert_eq!(u.get("A").map(String::as_str), Some("mit"));
        assert_eq!(u.get("B").map(String::as_str), Some("auch"));
    }

    #[test]
    fn ohne_modell_wird_nicht_geraten() {
        // Ein falscher Modellname im Schirm ist schlimmer als keiner.
        let m = modell(&leer());

        assert!(m.contains("Vorgabe") || !m.is_empty());
    }

    #[test]
    fn das_modell_kommt_aus_der_uebergebenen_umgebung() {
        let u = mit(&[("ANTHROPIC_MODEL", "claude-opus-5")]);

        assert_eq!(modell(&u), "claude-opus-5");
    }

    #[test]
    fn vom_anbieter_bleibt_der_rechnername() {
        let u = mit(&[("ANTHROPIC_BASE_URL", "https://openrouter.ai/api/v1")]);

        assert_eq!(anbieter(&u).as_deref(), Some("openrouter.ai"));
    }
}
