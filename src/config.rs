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

/// Das Heimatverzeichnis des Benutzers.
///
/// Windows setzt `HOME` nicht — dort heißt es `USERPROFILE`. Ohne diesen Zweig
/// bricht das Programm auf Windows vor der ersten Aufgabe ab, mit einer Meldung
/// über eine Variable, die dort niemand kennt.
pub fn heimat() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| "Weder HOME noch USERPROFILE ist gesetzt.".to_string())
}

pub fn config_dir() -> Result<PathBuf, String> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(xdg).join("ai-brain-companion"));
    }

    // Unter Windows gehört so etwas nach %APPDATA%, nicht in ein verstecktes
    // Verzeichnis im Heimatordner. XDG gibt es dort nicht.
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return Ok(PathBuf::from(appdata).join("ai-brain-companion"));
        }
    }

    Ok(heimat()?.join(".config").join("ai-brain-companion"))
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
    Ok(heimat()?.join(".claude.json"))
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

// ----------------------------------------------------------------- codex ----

/// Traegt den MCP-Server in Codex ein — ueber dessen eigenes Werkzeug.
///
/// Codex bringt `codex mcp add` mit und kennt sein TOML-Format selbst. Die
/// `~/.codex/config.toml` von Hand zu schreiben waere die schlechtere Loesung:
/// Sie enthaelt bereits `[features]` und `[projects."…"]`-Abschnitte, und ein
/// selbstgebauter Schreiber muesste die alle heil lassen.
///
/// Fehlt Codex auf dem Rechner, ist das kein Fehler — dann gibt es nichts
/// einzutragen.
pub fn write_codex_config() -> Result<Option<String>, String> {
    if which("codex").is_none() {
        return Ok(None);
    }

    let executable = std::env::current_exe()
        .map_err(|e| format!("Eigener Programmpfad nicht ermittelbar: {e}"))?;

    let pfad = executable.to_string_lossy().to_string();

    // Ein vorhandener Eintrag laesst sich nicht ueberschreiben — erst weg,
    // dann neu. Schlaegt das Entfernen fehl, war keiner da; das ist der
    // Normalfall und kein Grund zur Sorge.
    let _ = std::process::Command::new("codex")
        .args(["mcp", "remove", "ai-brain"])
        .output();

    let ergebnis = std::process::Command::new("codex")
        .args(["mcp", "add", "ai-brain", "--", &pfad, "mcp"])
        .output()
        .map_err(|e| format!("codex liess sich nicht aufrufen: {e}"))?;

    if !ergebnis.status.success() {
        let fehler = String::from_utf8_lossy(&ergebnis.stderr);

        return Err(format!("codex mcp add schlug fehl: {}", fehler.trim()));
    }

    Ok(Some("codex".to_string()))
}

/// Liegt dieses Programm im PATH?
fn which(name: &str) -> Option<PathBuf> {
    let pfad = std::env::var_os("PATH")?;

    std::env::split_paths(&pfad)
        .map(|verzeichnis| verzeichnis.join(name))
        .find(|kandidat| kandidat.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bewusst EIN Test statt mehrerer: Umgebungsvariablen gehören dem ganzen
    /// Prozess, und Rust lässt Tests nebenläufig laufen. Getrennte Tests würden
    /// sich gegenseitig die Variablen unter den Füßen wegziehen und je nach
    /// Laune bestehen oder scheitern.
    #[test]
    fn pfade_finden_auch_ohne_home() {
        let sicherung: Vec<(&str, Option<std::ffi::OsString>)> =
            ["HOME", "USERPROFILE", "XDG_CONFIG_HOME", "APPDATA"]
                .iter()
                .map(|k| (*k, std::env::var_os(k)))
                .collect();

        for (k, _) in &sicherung {
            std::env::remove_var(k);
        }

        // Ohne jede Angabe darf nicht geraten werden.
        assert!(
            heimat().is_err(),
            "ohne HOME und USERPROFILE darf es kein Ergebnis geben"
        );

        // Der Windows-Fall: nur USERPROFILE gesetzt. Genau hier brach das
        // Programm vorher ab, bevor es überhaupt etwas tun konnte.
        std::env::set_var("USERPROFILE", "/tmp/nutzer");
        assert_eq!(heimat().unwrap(), PathBuf::from("/tmp/nutzer"));

        // HOME hat Vorrang, damit sich unter Unix nichts ändert.
        std::env::set_var("HOME", "/tmp/zuhause");
        assert_eq!(heimat().unwrap(), PathBuf::from("/tmp/zuhause"));

        // XDG schlägt alles — das galt vorher schon und muss weiter gelten.
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg");
        assert_eq!(
            config_dir().unwrap(),
            PathBuf::from("/tmp/xdg/ai-brain-companion")
        );

        std::env::remove_var("XDG_CONFIG_HOME");
        assert_eq!(
            config_dir().unwrap(),
            PathBuf::from("/tmp/zuhause/.config/ai-brain-companion")
        );

        for (k, wert) in sicherung {
            match wert {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}
