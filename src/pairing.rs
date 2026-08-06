//! Kopplung über den Device Authorization Grant (RFC 8628).
//!
//! Der Grant ist hier nur der **Kanal**. Was am Ende herauskommt, ist nicht der
//! Zugangstoken, sondern ein eigener Gerätezugang: eigene Client-ID, eigenes
//! Secret, Besitzer ist der Mensch, der im Browser bestätigt hat. Erst dadurch
//! bleibt die Zurechnung pro Gerät erhalten — ein geteilter Token für alle
//! Companions würde genau die Unterscheidung einebnen, um die es geht.

use crate::config::{self, Connection};
use crate::discovery;
use serde::Deserialize;
use std::io::Write;
use std::thread::sleep;
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: Option<String>,
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_interval() -> u64 {
    5
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct OAuthError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProvisionResponse {
    client_id: String,
    client_secret: String,
    device_name: String,
    owner: Owner,
}

#[derive(Debug, Deserialize)]
struct Owner {
    name: String,
}

pub fn connect(args: &[String]) -> Result<(), String> {
    let mut domain: Option<String> = None;
    let mut device_name: Option<String> = None;
    let mut configure_claude = true;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--name" => {
                i += 1;
                device_name = Some(
                    args.get(i)
                        .ok_or_else(|| "--name braucht einen Wert.".to_string())?
                        .clone(),
                );
            }
            "--no-claude-config" => configure_claude = false,
            other if other.starts_with("--") => {
                return Err(format!("Unbekannte Option: {other}"));
            }
            other => domain = Some(other.to_string()),
        }
        i += 1;
    }

    let domain = domain.ok_or_else(|| {
        "Welche Instanz? Beispiel:\n  ai-brain-companion connect brain.proxy.peppermint-digital.com"
            .to_string()
    })?;

    let base_url = discovery::normalize_domain(&domain);

    println!("Frage {base_url} …");
    let instance = discovery::fetch(&base_url)?;

    let client_id = instance.oauth.companion_client_id.clone().ok_or_else(|| {
        format!(
            "{} kennt noch keinen Companion-Zugang.\n\
             Auf der Instanz einmalig ausführen:  php artisan mcp:companion-client",
            instance.name
        )
    })?;

    let device_name = device_name.unwrap_or_else(default_device_name);

    let device = request_device_code(&instance.oauth.device_authorization_endpoint, &client_id, &instance.mcp.scope)?;

    println!();
    println!("  Dein Code:  {}", device.user_code);
    println!();
    println!("  Bestätige ihn unter:");
    println!(
        "  {}",
        device
            .verification_uri_complete
            .as_deref()
            .unwrap_or(&device.verification_uri)
    );
    println!();
    print!("Warte auf die Bestätigung ");
    let _ = std::io::stdout().flush();

    let access_token = poll_for_token(
        &instance.oauth.token_endpoint,
        &client_id,
        &device.device_code,
        device.interval,
        device.expires_in,
    )?;

    println!();
    println!("Bestätigt. Richte den Gerätezugang ein …");

    let provisioned = provision(&instance.companion.provision_endpoint, &access_token, &device_name)?;

    let connection = Connection {
        base_url: base_url.clone(),
        instance_name: instance.name.clone(),
        device_name: provisioned.device_name.clone(),
        client_id: provisioned.client_id,
        client_secret: provisioned.client_secret,
        token_endpoint: instance.oauth.token_endpoint.clone(),
        mcp_url: instance.mcp.url.clone(),
        scope: instance.mcp.scope.clone(),
        owner: provisioned.owner.name.clone(),
    };

    let stored = config::save(&connection)?;

    println!();
    println!("Fertig.");
    println!("  Gerät          : {}", connection.device_name);
    println!("  Zugerechnet an : {}", connection.owner);
    println!("  Hinterlegt in  : {}", stored.display());

    if configure_claude {
        match config::write_claude_config() {
            Ok(path) => {
                println!("  Claude Code    : Eintrag „ai-brain“ in {}", path.display());

                // Codex bringt `codex mcp add` mit und kennt sein TOML-Format
                // selbst — besser, als die config.toml von Hand zu schreiben.
                // Fehlt Codex, ist das kein Fehler, sondern der Normalfall.
                match config::write_codex_config() {
                    Ok(Some(_)) => println!("  Codex          : Eintrag „ai-brain“ angelegt"),
                    Ok(None) => {}
                    Err(err) => eprintln!("  Codex          : nicht eingetragen ({err})"),
                }

                println!();
                println!("Starte deine Werkzeuge neu, dann stehen die Befehle bereit.");
            }
            Err(err) => {
                // Kein Abbruch: die Kopplung ist gelungen und gespeichert. Nur
                // der bequeme Teil hat gehakt — das darf den Nutzer nicht um
                // das Ergebnis bringen.
                eprintln!();
                eprintln!("Hinweis: Claude-Code-Konfiguration nicht angepasst ({err}).");
                eprintln!("Trag den MCP-Server „ai-brain“ von Hand ein, Befehl: ai-brain-companion mcp");
            }
        }
    }

    println!();
    println!("Widerrufen kannst du das Gerät jederzeit unter:");
    println!("  {}", instance.companion.devices_url);

    Ok(())
}

fn request_device_code(endpoint: &str, client_id: &str, scope: &str) -> Result<DeviceCodeResponse, String> {
    ureq::post(endpoint)
        .set("Accept", "application/json")
        .send_form(&[("client_id", client_id), ("scope", scope)])
        .map_err(|e| format!("Kopplung lässt sich nicht starten: {}", describe(e)))?
        .into_json()
        .map_err(|e| format!("Antwort auf die Kopplungsanfrage ist unlesbar: {e}"))
}

fn poll_for_token(
    endpoint: &str,
    client_id: &str,
    device_code: &str,
    interval: u64,
    expires_in: u64,
) -> Result<String, String> {
    let deadline = Instant::now() + Duration::from_secs(expires_in);
    let mut wait = Duration::from_secs(interval.max(1));

    loop {
        if Instant::now() >= deadline {
            return Err("Der Code ist abgelaufen. Starte die Kopplung noch einmal.".to_string());
        }

        sleep(wait);
        print!(".");
        let _ = std::io::stdout().flush();

        let attempt = ureq::post(endpoint).set("Accept", "application/json").send_form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code),
            ("client_id", client_id),
        ]);

        match attempt {
            Ok(response) => {
                let token: TokenResponse = response
                    .into_json()
                    .map_err(|e| format!("Token-Antwort ist unlesbar: {e}"))?;

                return Ok(token.access_token);
            }
            Err(ureq::Error::Status(_, response)) => {
                let err: OAuthError = response
                    .into_json()
                    .map_err(|e| format!("Fehlerantwort ist unlesbar: {e}"))?;

                match err.error.as_str() {
                    // Der Normalfall, solange niemand bestätigt hat.
                    "authorization_pending" => continue,
                    // Der Server bittet um mehr Geduld — RFC 8628 verlangt,
                    // das Intervall dauerhaft zu erhöhen, nicht nur einmal.
                    "slow_down" => {
                        wait += Duration::from_secs(5);
                        continue;
                    }
                    "access_denied" => {
                        return Err("Die Kopplung wurde im Browser abgelehnt.".to_string());
                    }
                    "expired_token" => {
                        return Err("Der Code ist abgelaufen. Starte die Kopplung noch einmal.".to_string());
                    }
                    other => {
                        let detail = err.error_description.unwrap_or_default();
                        return Err(format!("Kopplung fehlgeschlagen ({other}). {detail}"));
                    }
                }
            }
            Err(transport) => {
                return Err(format!("Verbindung zur Instanz verloren: {transport}"));
            }
        }
    }
}

fn provision(endpoint: &str, access_token: &str, device_name: &str) -> Result<ProvisionResponse, String> {
    ureq::post(endpoint)
        .set("Authorization", &format!("Bearer {access_token}"))
        .set("Accept", "application/json")
        .send_json(serde_json::json!({
            "device_name": device_name,
            "platform": platform_label(),
        }))
        .map_err(|e| format!("Gerätezugang konnte nicht angelegt werden: {}", describe(e)))?
        .into_json()
        .map_err(|e| format!("Antwort der Provisionierung ist unlesbar: {e}"))
}

/// Standard-Gerätename: `konto@rechner`.
///
/// Der blanke Hostname reicht nicht. Zwei Konten auf derselben Maschine
/// erzeugen sonst zwei Einträge mit identischem Namen — und weil dieser Name
/// in der Zeitstatistik als `started_by` erscheint, wäre die Auswertung pro
/// Gerät damit wertlos. Genau das ist beim ersten Feldtest passiert.
fn default_device_name() -> String {
    let host = gethostname::gethostname().to_string_lossy().to_string();

    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();

    if user.is_empty() {
        host
    } else {
        format!("{user}@{host}")
    }
}

fn platform_label() -> String {
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

/// ureq packt den Antworttext bei Statusfehlern in die Response — ohne das
/// Auslesen sieht der Nutzer nur „400 Bad Request" und weiß nichts.
fn describe(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(code, response) => {
            let body = response.into_string().unwrap_or_default();
            let trimmed = body.trim();

            if trimmed.is_empty() {
                format!("HTTP {code}")
            } else {
                format!("HTTP {code} — {trimmed}")
            }
        }
        other => other.to_string(),
    }
}
