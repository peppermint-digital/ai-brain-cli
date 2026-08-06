//! Instanz-Steckbrief lesen.
//!
//! Der Companion kennt nur die Domain — sie steckt in der URL, mit der er
//! installiert wurde. Alles Weitere steht unter `/.well-known/ai-brain`:
//! wo der MCP-Endpunkt liegt, wo gekoppelt wird, und mit welcher öffentlichen
//! Client-ID der Device-Flow zu starten ist.
//!
//! Damit funktioniert der Companion gegen jede selbstgehostete Instanz, ohne
//! dass Pfade einkompiliert wären.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Discovery {
    pub name: String,
    pub product: String,
    pub mcp: Mcp,
    pub oauth: OAuth,
    pub companion: Companion,
}

#[derive(Debug, Deserialize)]
pub struct Mcp {
    pub url: String,
    pub scope: String,
}

#[derive(Debug, Deserialize)]
pub struct OAuth {
    pub token_endpoint: String,
    pub device_authorization_endpoint: String,
    /// Gehört zum Steckbrief-Vertrag, wird aber im Ablauf nicht gebraucht:
    /// Die Antwort auf die Kopplungsanfrage liefert ihre eigene, konkretere
    /// Bestätigungs-URL mit. Das Feld bleibt, damit ein fehlender oder
    /// umbenannter Schlüssel beim Einlesen auffällt statt still zu verschwinden.
    #[allow(dead_code)]
    pub verification_uri: String,
    /// Fehlt, solange auf der Instanz `php artisan mcp:companion-client` nicht
    /// gelaufen ist. Die Instanz meldet das ehrlich als `null`, statt eine
    /// Kennung zu erfinden — wir können dem Nutzer deshalb genau sagen, was
    /// zu tun ist, statt ihn in einen OAuth-Fehler laufen zu lassen.
    pub companion_client_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Companion {
    pub provision_endpoint: String,
    pub devices_url: String,
}

/// Nimmt entgegen, was ein Mensch tippt („brain.example.com",
/// „https://brain.example.com", mit oder ohne Schrägstrich am Ende) und macht
/// eine brauchbare Basis-URL daraus.
pub fn normalize_domain(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');

    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    }
}

pub fn fetch(base_url: &str) -> Result<Discovery, String> {
    let url = format!("{base_url}/.well-known/ai-brain");

    let response = ureq::get(&url)
        .set("Accept", "application/json")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(404, _) => format!(
                "Unter {base_url} antwortet zwar ein Server, aber kein AI Brain \
                 (kein Steckbrief unter /.well-known/ai-brain). Stimmt die Adresse?"
            ),
            other => format!("{base_url} ist nicht erreichbar: {other}"),
        })?;

    let discovery: Discovery = response
        .into_json()
        .map_err(|e| format!("Der Steckbrief von {base_url} ist unlesbar: {e}"))?;

    if discovery.product != "ai-brain" {
        return Err(format!(
            "Unter {base_url} läuft etwas anderes (product = „{}“).",
            discovery.product
        ));
    }

    Ok(discovery)
}
