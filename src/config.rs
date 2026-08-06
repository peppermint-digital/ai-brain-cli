//! Ablage der Gerätezugangsdaten und Eintrag in Claude Code.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct Connection {
    pub base_url: String,
    pub instance_name: String,
    pub device_name: String,
    pub client_id: String,
    pub client_secret: String,
    pub token_endpoint: String,
    pub mcp_url: String,
    pub scope: String,
    pub owner: String,
}

pub fn config_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or_else(|| "Weder XDG_CONFIG_HOME noch HOME gesetzt.".to_string())?;

    Ok(base.join("ai-brain-companion"))
}

pub fn config_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("connection.json"))
}

pub fn save(connection: &Connection) -> Result<PathBuf, String> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir).map_err(|e| format!("{} lässt sich nicht anlegen: {e}", dir.display()))?;

    let path = config_path()?;
    let json = serde_json::to_string_pretty(connection)
        .map_err(|e| format!("Verbindung lässt sich nicht schreiben: {e}"))?;

    fs::write(&path, json).map_err(|e| format!("{} lässt sich nicht schreiben: {e}", path.display()))?;

    // Die Datei enthält ein Client-Secret. Auf Unix eng stellen, damit sie
    // nicht für andere Konten des Rechners lesbar ist. Ein Schlüsselbund wäre
    // besser und steht auf der Liste — aber lieber jetzt 0600 als später
    // vielleicht Keyring.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }

    Ok(path)
}

pub fn load() -> Result<Connection, String> {
    let path = config_path()?;

    let raw = fs::read_to_string(&path).map_err(|_| {
        format!(
            "Dieser Rechner ist noch nicht verbunden.\n\
             Führe zuerst aus:  ai-brain-companion connect <domain>"
        )
    })?;

    serde_json::from_str(&raw).map_err(|e| format!("{} ist beschädigt: {e}", path.display()))
}

pub fn print_status() -> Result<(), String> {
    let connection = load()?;

    println!("Verbunden mit : {}", connection.base_url);
    println!("Instanz       : {}", connection.instance_name);
    println!("Gerät         : {}", connection.device_name);
    println!("Zugerechnet an: {}", connection.owner);
    println!("Client-ID     : {}", connection.client_id);
    println!("MCP-Endpunkt  : {}", connection.mcp_url);

    Ok(())
}

pub fn disconnect() -> Result<(), String> {
    let path = config_path()?;

    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("{} lässt sich nicht löschen: {e}", path.display()))?;
        println!("Lokale Verbindung entfernt.");
    } else {
        println!("Es war keine Verbindung hinterlegt.");
    }

    println!();
    println!("Hinweis: Der Gerätezugang besteht auf der Instanz weiter.");
    println!("Endgültig entziehen kannst du ihn unter „Meine Geräte“.");

    Ok(())
}

fn claude_config_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or_else(|| "HOME ist nicht gesetzt.".to_string())?;

    Ok(PathBuf::from(home).join(".claude.json"))
}

/// Trägt den Companion als MCP-Server in Claude Code ein.
///
/// Bewusst mit Sicherung vorher: `~/.claude.json` gehört Claude Code, enthält
/// weit mehr als unseren Eintrag, und ein kaputtes JSON dort kostet den Nutzer
/// seine gesamte Konfiguration. Wir lesen, ergänzen genau einen Schlüssel und
/// schreiben zurück — alles andere bleibt unangetastet.
pub fn write_claude_config() -> Result<PathBuf, String> {
    let path = claude_config_path()?;

    let mut root: serde_json::Value = if path.exists() {
        let raw = fs::read_to_string(&path)
            .map_err(|e| format!("{} lässt sich nicht lesen: {e}", path.display()))?;

        let backup = path.with_extension("json.vor-companion");
        fs::write(&backup, &raw)
            .map_err(|e| format!("Sicherung {} schlug fehl: {e}", backup.display()))?;

        serde_json::from_str(&raw)
            .map_err(|e| format!("{} enthält kein gültiges JSON: {e}", path.display()))?
    } else {
        serde_json::json!({})
    };

    let executable = std::env::current_exe()
        .map_err(|e| format!("Eigener Programmpfad nicht ermittelbar: {e}"))?;

    root.as_object_mut()
        .ok_or_else(|| format!("{} ist kein JSON-Objekt.", path.display()))?
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "mcpServers ist kein JSON-Objekt.".to_string())?
        .insert(
            "ai-brain".to_string(),
            serde_json::json!({
                "type": "stdio",
                "command": executable.to_string_lossy(),
                "args": ["mcp"],
            }),
        );

    let json = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("Konfiguration lässt sich nicht schreiben: {e}"))?;

    fs::write(&path, json).map_err(|e| format!("{} lässt sich nicht schreiben: {e}", path.display()))?;

    Ok(path)
}
