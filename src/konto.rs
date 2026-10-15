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

/// Ist dieser Rechner überhaupt am Modellanbieter angemeldet?
///
/// **Warum die Maske das wissen muss.** Ohne Anmeldung reicht der Motor eine
/// eigene Meldung durch, die nach einem Programmfehler klingt. Der Mensch
/// davor braucht aber einen Satz, der sagt, was zu tun ist — und das ist
/// `claude login`, nicht ein Fehlerbericht.
///
/// Erkannt an `.credentials.json` im geltenden Konfigurationsordner. Das ist
/// die Datei, die `claude login` anlegt; imkr legt sie je Konto eigens ab.
/// Bewusst nicht am Token in der Umgebung: wer `ANTHROPIC_API_KEY` gesetzt
/// hat, ist auch angemeldet, nur anders — deshalb zählt beides.
pub fn angemeldet(umgebung: &BTreeMap<String, String>) -> bool {
    for name in ["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_AUTH_TOKEN"] {
        let gesetzt = umgebung
            .get(name)
            .cloned()
            .or_else(|| std::env::var(name).ok())
            .filter(|w| !w.trim().is_empty());

        if gesetzt.is_some() {
            return true;
        }
    }

    let Some(ordner) = konfigurationsordner(umgebung) else {
        return false;
    };

    std::path::Path::new(&ordner).join(".credentials.json").is_file()
}

/// Der geltende Konfigurationsordner von Claude Code.
fn konfigurationsordner(umgebung: &BTreeMap<String, String>) -> Option<String> {
    if let Some(pfad) = umgebung
        .get("CLAUDE_CONFIG_DIR")
        .cloned()
        .or_else(|| std::env::var("CLAUDE_CONFIG_DIR").ok())
        .filter(|p| !p.trim().is_empty())
    {
        return Some(pfad);
    }

    std::env::var("HOME").ok().map(|h| format!("{h}/.claude"))
}

/// Was zu tun ist, wenn keine Anmeldung gefunden wurde.
///
/// Zwei Wege, und der Unterschied gehört dazu: `login` öffnet den Browser und
/// ist für einen Menschen am Rechner gedacht; `setup-token` erzeugt ein
/// langlebiges Token und ist der Weg für Rechner ohne Browser.
pub fn anmeldehinweis(umgebung: &BTreeMap<String, String>) -> String {
    let ordner = konfigurationsordner(umgebung).unwrap_or_else(|| "~/.claude".to_string());

    // Aus Zeilen zusammengesetzt, nicht als mehrzeiliges Literal: ein
    // Literal ueber mehrere Zeilen nimmt die Einrueckung des Quelltexts MIT,
    // und die landet auf dem Bildschirm. Beim ersten Echttest standen neun
    // Leerzeichen vor jeder Folgezeile.
    [
        "Dieser Rechner ist nicht am Modellanbieter angemeldet.".to_string(),
        format!("In {ordner} liegt keine `.credentials.json`, und es ist"),
        "kein Token in der Umgebung gesetzt.".to_string(),
        String::new(),
        "Anmelden:       claude login         (öffnet den Browser)".to_string(),
        "Ohne Browser:   claude setup-token   (langlebiges Token, braucht ein Abo)".to_string(),
        String::new(),
        "Ohne Anmeldung laufen weiterhin: /aufgaben, /auftraege, /daueraufgaben,".to_string(),
        "/dienste, /rechner, /erinnerungen. Es fehlt nur das Gespräch.".to_string(),
    ]
    .join("\n")
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
    fn ein_token_in_der_umgebung_gilt_als_angemeldet() {
        // Wer `ANTHROPIC_API_KEY` gesetzt hat, ist angemeldet — nur anders als
        // per `claude login`. Beides zaehlt, sonst meldet die Maske „nicht
        // angemeldet" an jemanden, der gerade arbeitet.
        //
        // **Der Ordner muss leer sein.** Ohne `CLAUDE_CONFIG_DIR` greift der
        // Rueckfall auf `~/.claude` — und auf einem Entwicklerrechner liegt
        // dort eine `.credentials.json`. Der Test war damit gruen, egal ob die
        // Umgebungsvariable geprueft wird. Mit einer Mutationsprobe gefunden:
        // das Entfernen des Umgebungswegs liess ihn unberuehrt.
        let ordner = std::env::temp_dir().join(format!("harness-nur-env-{}", std::process::id()));
        std::fs::create_dir_all(&ordner).unwrap();

        let u = mit(&[
            ("ANTHROPIC_API_KEY", "sk-ant-api03-irgendwas"),
            ("CLAUDE_CONFIG_DIR", &ordner.display().to_string()),
        ]);

        assert!(angemeldet(&u), "ein Token in der Umgebung ist eine Anmeldung");

        let _ = std::fs::remove_dir_all(&ordner);
    }

    #[test]
    fn ein_leeres_token_gilt_nicht_als_angemeldet() {
        // Eine gesetzte, aber leere Variable ist der haeufigste Fall nach einem
        // misslungenen Export — sie darf nicht wie eine Anmeldung aussehen.
        let u = mit(&[("ANTHROPIC_API_KEY", "   ")]);
        let voruebergehend = std::env::temp_dir().join(format!("harness-leer-{}", std::process::id()));
        std::fs::create_dir_all(&voruebergehend).unwrap();

        let mut u2 = u.clone();
        u2.insert("CLAUDE_CONFIG_DIR".into(), voruebergehend.display().to_string());

        assert!(!angemeldet(&u2));

        let _ = std::fs::remove_dir_all(&voruebergehend);
    }

    #[test]
    fn eine_credentials_datei_gilt_als_angemeldet() {
        let ordner = std::env::temp_dir().join(format!("harness-angemeldet-{}", std::process::id()));
        std::fs::create_dir_all(&ordner).unwrap();
        std::fs::write(ordner.join(".credentials.json"), "{}").unwrap();

        let u = mit(&[("CLAUDE_CONFIG_DIR", &ordner.display().to_string())]);

        assert!(angemeldet(&u));

        let _ = std::fs::remove_dir_all(&ordner);
    }

    #[test]
    fn der_hinweis_nennt_beide_wege_und_den_ordner() {
        let ordner = std::env::temp_dir().join("harness-hinweis-pruefung");
        let u = mit(&[("CLAUDE_CONFIG_DIR", &ordner.display().to_string())]);

        let h = anmeldehinweis(&u);

        assert!(h.contains("claude login"), "{h}");
        // Der Weg ohne Browser gehoert dazu — genau fuer Rechner, die keinen
        // haben.
        assert!(h.contains("setup-token"), "{h}");
        assert!(h.contains(&ordner.display().to_string()), "{h}");
        // Und was trotzdem geht, damit die Maske nicht wertlos wirkt.
        assert!(h.contains("/dienste"), "{h}");
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
