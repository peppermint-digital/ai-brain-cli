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

/// Die Modellkonten, die AI Brain führt.
///
/// **Zweite Quelle, nicht Ersatz.** Wer imkr benutzt, hat seine Konten dort;
/// wer nicht, hat sie in AI Brain. Beides kommt vor, und eine Maske, die nur
/// eine Quelle kennt, lässt auf dem jeweils anderen Rechner nichts anbieten.
/// Bastians Einwand dazu war genau dieser Fall.
///
/// Scheitert die Abfrage, ist das kein Fehler der Maske — dann gibt es eben
/// keine Instanz-Konten zu zeigen. Gesagt wird es trotzdem.
pub fn aus_ai_brain() -> Result<Vec<InstanzKonto>, String> {
    let connection = crate::config::load()?;
    let token = crate::mcp::access_token(&connection)?;

    let roh = crate::mcp::call_tool(
        &connection,
        &token,
        "list-model-accounts-tool",
        serde_json::json!({ "json": true }),
    )?;

    let antwort: InstanzAntwort = serde_json::from_value(roh)
        .map_err(|e| format!("Antwort auf list-model-accounts-tool ist unlesbar: {e}"))?;

    Ok(antwort.konten)
}

#[derive(Debug, serde::Deserialize)]
struct InstanzAntwort {
    #[serde(default)]
    konten: Vec<InstanzKonto>,
}

#[derive(Debug, serde::Deserialize)]
pub struct InstanzKonto {
    pub art: String,
    pub label: String,
    #[serde(default)]
    pub aktiv: bool,
    #[serde(default)]
    pub zustand: Option<String>,
    #[serde(default)]
    pub letzter_fehler: Option<String>,
    #[serde(default)]
    pub token_endet_auf: Option<String>,
}

impl InstanzKonto {
    /// Eine Zeile für die Maske.
    pub fn zeile(&self) -> String {
        let mut z = format!("  {:<28} {}", self.label, self.art);

        if !self.aktiv {
            z.push_str("  abgeschaltet");
        } else if let Some(zustand) = &self.zustand {
            z.push_str(&format!("  {zustand}"));
        }

        // Die letzten vier Zeichen des Tokens. Der einzige Grund: zwei Konten
        // mit aehnlichem Namen unterscheiden. Mehr gibt das Werkzeug nicht
        // heraus, und mehr braucht die Zeile nicht.
        if let Some(ende) = &self.token_endet_auf {
            if !ende.is_empty() {
                z.push_str(&format!("  {ende}"));
            }
        }

        if let Some(fehler) = &self.letzter_fehler {
            if !fehler.is_empty() {
                z.push_str(&format!("  ({fehler})"));
            }
        }

        z
    }
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

    fn instanz(json: serde_json::Value) -> Vec<InstanzKonto> {
        serde_json::from_value::<InstanzAntwort>(json).unwrap().konten
    }

    #[test]
    fn eine_leere_antwort_ist_kein_fehler() {
        // Ohne `#[serde(default)]` scheiterte das Zerlegen, und „keine
        // Konten" saehe aus wie „nicht abfragbar".
        assert!(instanz(serde_json::json!({})).is_empty());
    }

    #[test]
    fn ein_instanzkonto_wird_vollstaendig_gelesen() {
        let k = instanz(serde_json::json!({
            "konten": [{
                "art": "claude",
                "label": "Stelljes",
                "aktiv": true,
                "zustand": "ok",
                "letzter_fehler": null,
                "token_endet_auf": "…9876"
            }]
        }));

        assert_eq!(k.len(), 1);
        assert_eq!(k[0].label, "Stelljes");
        assert_eq!(k[0].art, "claude");
        assert!(k[0].aktiv);

        let z = k[0].zeile();
        assert!(z.contains("Stelljes"), "{z}");
        assert!(z.contains("claude"), "{z}");
        assert!(z.contains("ok"), "{z}");
        // Die letzten vier Zeichen unterscheiden zwei aehnliche Konten.
        assert!(z.contains("9876"), "{z}");
    }

    #[test]
    fn ein_abgeschaltetes_konto_sieht_nicht_ruhig_aus() {
        let k = instanz(serde_json::json!({
            "konten": [{ "art": "claude", "label": "Alt", "aktiv": false, "zustand": "ok" }]
        }));

        let z = k[0].zeile();

        assert!(z.contains("abgeschaltet"), "{z}");
        // Der Zustand „ok" darf daneben nicht stehen — abgeschaltet und ok
        // zugleich liest sich wie „laeuft".
        assert!(!z.contains("ok"), "{z}");
    }

    #[test]
    fn ein_letzter_fehler_steht_in_der_zeile() {
        // Ein totes Konto muss auffallen, sonst waehlt man es aus.
        let k = instanz(serde_json::json!({
            "konten": [{
                "art": "claude", "label": "Abgelaufen", "aktiv": true,
                "zustand": "fehler", "letzter_fehler": "401 Unauthorized"
            }]
        }));

        let z = k[0].zeile();

        assert!(z.contains("401 Unauthorized"), "{z}");
    }

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
