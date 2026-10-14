//! MCP-Server über stdio.
//!
//! Claude Code startet diesen Prozess und spricht JSON-RPC über stdin/stdout.
//! Wir reichen jede Nachricht an den MCP-Endpunkt der Instanz weiter und geben
//! die Antwort zurück — mit einem gültigen Token davor.
//!
//! Das ist der Teil, der den bisherigen Sonderweg ersetzt: Bisher hing dieser
//! Rechner über `php artisan mcp:start brain` am Repo-Verzeichnis, ohne
//! OAuth, ohne Gerät, nicht widerrufbar und nur hier lauffähig. Jetzt geht
//! jeder Rechner denselben Weg über die Domain.
//!
//! Zwei Dinge sind hier heikel:
//!
//! * **stdout gehört dem Protokoll.** Jede Ausgabe, die kein JSON-RPC ist,
//!   bricht die Verbindung. Alle Meldungen gehen deshalb nach stderr.
//! * **Benachrichtigungen haben keine Antwort.** JSON-RPC-Nachrichten ohne
//!   `id` sind Notifications; auf sie darf nichts zurückgeschrieben werden,
//!   sonst wartet die Gegenseite auf eine Antwort, die es nicht gibt.

use crate::config::{self, Connection};
use serde::Deserialize;
use std::io::{BufRead, Read, Write};
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default = "default_expiry")]
    expires_in: u64,
}

fn default_expiry() -> u64 {
    3600
}

struct TokenCache {
    value: String,
    valid_until: Instant,
}

pub fn serve() -> Result<(), String> {
    let connection = config::load()?;

    eprintln!(
        "ai-brain-cli: verbunden mit {} als Gerät „{}“",
        connection.base_url, connection.device_name
    );

    let mut cache: Option<TokenCache> = None;

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| format!("stdin nicht lesbar: {e}"))?;
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        let message: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(e) => {
                eprintln!("ai-brain-cli: ungültiges JSON von Claude Code übersprungen ({e})");
                continue;
            }
        };

        let is_notification = message.get("id").is_none();

        let token = match token(&connection, &mut cache) {
            Ok(token) => token,
            Err(err) => {
                eprintln!("ai-brain-cli: {err}");

                if !is_notification {
                    respond_with_error(&mut stdout, &message, &err)?;
                }

                continue;
            }
        };

        match forward(&connection, &token, trimmed) {
            Ok(Some(body)) => {
                if !is_notification {
                    writeln!(stdout, "{body}").map_err(|e| format!("stdout nicht beschreibbar: {e}"))?;
                    stdout.flush().map_err(|e| format!("stdout nicht leerbar: {e}"))?;
                }
            }
            Ok(None) => {
                // Leere Antwort — bei Notifications der Normalfall.
            }
            Err(err) => {
                eprintln!("ai-brain-cli: {err}");

                if !is_notification {
                    respond_with_error(&mut stdout, &message, &err)?;
                }
            }
        }
    }

    Ok(())
}

/// Einmalig einen Token holen — für Aufrufe ausserhalb der Dauerschleife
/// (etwa die Synchronisation), die keinen Zwischenspeicher brauchen.
pub fn access_token(connection: &Connection) -> Result<String, String> {
    let mut cache = None;

    token(connection, &mut cache)
}

/// Holt einen Token und behält ihn, bis er fast abgelaufen ist.
///
/// Der Puffer von 60 Sekunden ist kein Zierrat: ohne ihn läuft ein Token
/// zwischen Prüfung und Ankunft beim Server ab, und der Aufruf scheitert mit
/// einem 401, für den es keine erkennbare Ursache gibt.
fn token(connection: &Connection, cache: &mut Option<TokenCache>) -> Result<String, String> {
    if let Some(cached) = cache {
        if Instant::now() < cached.valid_until {
            return Ok(cached.value.clone());
        }
    }

    let response = ureq::post(&connection.token_endpoint)
        .set("Accept", "application/json")
        .send_form(&[
            ("grant_type", "client_credentials"),
            ("client_id", &connection.client_id),
            ("client_secret", &connection.client_secret),
            ("scope", &connection.scope),
        ])
        .map_err(|e| match e {
            ureq::Error::Status(401, _) => format!(
                "Der Gerätezugang wurde abgelehnt. Wurde er unter „Meine Geräte“ entzogen? \
                 Dann neu koppeln: ai-brain-cli connect {}",
                connection.base_url
            ),
            other => format!("Token nicht erhältlich: {other}"),
        })?;

    let token: TokenResponse = response
        .into_json()
        .map_err(|e| format!("Token-Antwort ist unlesbar: {e}"))?;

    let lifetime = Duration::from_secs(token.expires_in.saturating_sub(60).max(30));

    *cache = Some(TokenCache {
        value: token.access_token.clone(),
        valid_until: Instant::now() + lifetime,
    });

    Ok(token.access_token)
}

/// Ruft ein Werkzeug der Instanz auf und gibt dessen Nutzlast als JSON zurück.
///
/// Der `mcp`-Unterbefehl reicht fremde Anfragen nur durch; hier formt der
/// ai-brain-cli selbst eine. Beides läuft über denselben Kanal und dieselbe
/// Anmeldung — es gibt genau einen Weg zur Instanz, und der ist MCP.
///
/// Ein `initialize` ist nicht nötig: Der HTTP-Weg ist zustandslos, jede
/// Anfrage trägt ihre Anmeldung selbst.
pub fn call_tool(
    connection: &Connection,
    token: &str,
    name: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    })
    .to_string();

    let body = forward(connection, token, &payload)?
        .ok_or_else(|| format!("Die Instanz hat auf {name} nichts geantwortet."))?;

    let antwort: serde_json::Value = serde_json::from_str(&nutzlast(&body))
        .map_err(|e| format!("Antwort auf {name} ist unlesbar: {e}"))?;

    if let Some(fehler) = antwort.get("error") {
        let grund = fehler
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("kein Grund genannt");

        return Err(format!("Die Instanz lehnt {name} ab: {grund}"));
    }

    let text = antwort
        .pointer("/result/content/0/text")
        .and_then(|t| t.as_str())
        .ok_or_else(|| format!("Antwort auf {name} hat keinen Inhalt."))?;

    // Ein abgelehnter Aufruf kommt als gewöhnliche Antwort mit `isError` —
    // der Text ist dann eine Meldung, kein JSON. Ohne diese Abzweigung
    // scheiterte er als "unlesbares JSON" und die eigentliche Begründung
    // (etwa: das Gerät gehört jemand anderem) ginge verloren.
    if antwort.pointer("/result/isError").and_then(|f| f.as_bool()) == Some(true) {
        return Err(text.to_string());
    }

    serde_json::from_str(text).map_err(|e| format!("Inhalt von {name} ist kein JSON: {e} — {text}"))
}

/// Zieht aus der Antwort das JSON heraus.
///
/// Die Instanz darf laut Protokoll auch als Ereignisstrom antworten
/// (`text/event-stream`). Dann steht die Nutzlast hinter `data:`, und der Rest
/// sind Rahmenzeilen, an denen ein JSON-Parser scheitert.
fn nutzlast(body: &str) -> String {
    if !body.lines().any(|z| z.starts_with("data:")) {
        return body.trim().to_string();
    }

    body.lines()
        .filter_map(|z| z.strip_prefix("data:"))
        .map(str::trim)
        .filter(|z| !z.is_empty())
        .last()
        .unwrap_or("")
        .to_string()
}

fn forward(connection: &Connection, token: &str, payload: &str) -> Result<Option<String>, String> {
    let response = ureq::post(&connection.mcp_url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .set("Accept", "application/json, text/event-stream")
        .send_string(payload)
        .map_err(|e| match e {
            ureq::Error::Status(code, resp) => {
                let body = resp.into_string().unwrap_or_default();
                format!("Instanz antwortet mit HTTP {code}: {}", body.trim())
            }
            other => format!("Instanz nicht erreichbar: {other}"),
        })?;

    // NICHT `into_string()`: Das deckelt bei 10 MB und bricht mit „response too
    // big" ab. Ein Eintrag mit Binäranhang liegt darüber — `kami` bringt eine
    // 9 MB grosse ZIP mit, die als Base64 auf rund 12 MB wächst. Der Deckel
    // träfe damit ausgerechnet den Fall, für den die Base64-Kodierung überhaupt
    // eingeführt wurde.
    //
    // Die Obergrenze bleibt, aber grosszügig: ohne jede Grenze könnte eine
    // fehlgeleitete Antwort den Arbeitsspeicher füllen.
    const OBERGRENZE: u64 = 256 * 1024 * 1024;

    let mut body = String::new();
    response
        .into_reader()
        .take(OBERGRENZE)
        .read_to_string(&mut body)
        .map_err(|e| format!("Antwort der Instanz ist unlesbar: {e}"))?;

    let trimmed = body.trim();

    if trimmed.is_empty() {
        return Ok(None);
    }

    Ok(Some(trimmed.to_string()))
}

fn respond_with_error(
    stdout: &mut std::io::Stdout,
    request: &serde_json::Value,
    message: &str,
) -> Result<(), String> {
    let error = serde_json::json!({
        "jsonrpc": "2.0",
        "id": request.get("id").cloned().unwrap_or(serde_json::Value::Null),
        "error": {
            "code": -32000,
            "message": message,
        }
    });

    writeln!(stdout, "{error}").map_err(|e| format!("stdout nicht beschreibbar: {e}"))?;
    stdout.flush().map_err(|e| format!("stdout nicht leerbar: {e}"))?;

    Ok(())
}
