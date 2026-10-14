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
        return Ok(PathBuf::from(xdg).join("ai-brain-cli"));
    }

    // Unter Windows gehört so etwas nach %APPDATA%, nicht in ein verstecktes
    // Verzeichnis im Heimatordner. XDG gibt es dort nicht.
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return Ok(PathBuf::from(appdata).join("ai-brain-cli"));
        }
    }

    Ok(heimat()?.join(".config").join("ai-brain-cli"))
}

pub fn config_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("connection.json"))
}

/// Der Ordner, unter dem bis zur Umbenennung alles lag (#7664).
fn alter_config_dir() -> Result<PathBuf, String> {
    let neu = config_dir()?;
    let name = neu
        .file_name()
        .ok_or_else(|| format!("{} hat keinen Namen.", neu.display()))?;

    if name != "ai-brain-cli" {
        return Err("Unerwarteter Name der Ablage.".to_string());
    }

    Ok(neu.with_file_name("ai-brain-companion"))
}

/// Holt die Ablage des alten Namens herüber — einmalig, beim ersten Lauf (#7664).
///
/// Die Bedingung hängt an der **Verbindung**, nicht am Ordner. Das ist kein
/// Feinschliff: `install.sh` legt die Ablage selbst an und schreibt `instanz`
/// hinein, bevor dieses Programm das erste Mal läuft. Eine Prüfung auf „neuer
/// Ordner existiert nicht" würde nach jedem Installer-Lauf schweigen — und die
/// Zugangsdaten blieben unter dem alten Namen liegen, während das Programm
/// „Dieser Rechner ist noch nicht verbunden" meldet.
///
/// Es zieht der **ganze Inhalt** um, nicht nur `connection.json`: daneben liegen
/// `sync-state.json` (welcher Eintrag in welcher Fassung hier liegt) und
/// `gesicherte-fassungen/`. Bliebe der Abgleichstand zurück, meldete `sync`
/// anschliessend jeden Eintrag als geändert — und `--force` schöbe die Arbeit
/// der Nutzer als `.lokal` weg, obwohl sich nichts getan hat.
///
/// Was schon im neuen Ordner liegt, bleibt liegen: Die `instanz`, die der
/// Installer gerade geschrieben hat, nennt die Instanz, von der der Nutzer
/// eben installiert hat — die ist aktueller als die alte.
pub fn uebernimm_alte_ablage() {
    let (Ok(neu), Ok(alt)) = (config_dir(), alter_config_dir()) else {
        return;
    };

    if neu.join("connection.json").exists() || !alt.join("connection.json").exists() {
        return;
    }

    if let Err(e) = fs::create_dir_all(&neu) {
        eprintln!(
            "ai-brain-cli: {} lässt sich nicht anlegen ({e}).",
            neu.display()
        );
        return;
    }

    let Ok(eintraege) = fs::read_dir(&alt) else {
        eprintln!("ai-brain-cli: {} lässt sich nicht lesen.", alt.display());
        return;
    };

    let mut geholt = 0usize;
    let mut stehengeblieben = 0usize;

    for eintrag in eintraege.flatten() {
        let ziel = neu.join(eintrag.file_name());

        if ziel.exists() {
            stehengeblieben += 1;
            continue;
        }

        match fs::rename(eintrag.path(), &ziel) {
            Ok(()) => geholt += 1,
            Err(e) => {
                stehengeblieben += 1;
                eprintln!(
                    "ai-brain-cli: {} liess sich nicht übernehmen ({e}).",
                    eintrag.path().display()
                );
            }
        }
    }

    // Nur wenn nichts zurückblieb, ist der alte Ordner wirklich erledigt.
    // `remove_dir` (nicht `_all`) ist Absicht: Es darf nur ein LEERES
    // Verzeichnis verschwinden — alles andere wäre ein Löschweg für Daten,
    // die wir gerade nicht holen konnten.
    if stehengeblieben == 0 {
        let _ = fs::remove_dir(&alt);
    }

    eprintln!(
        "ai-brain-cli: Ablage übernommen — {} Eintrag/Einträge aus {} nach {}{}",
        geholt,
        alt.display(),
        neu.display(),
        if stehengeblieben > 0 {
            format!(", {stehengeblieben} blieb(en) stehen")
        } else {
            String::new()
        }
    );
}

pub fn save(connection: &Connection) -> Result<PathBuf, String> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir)
        .map_err(|e| format!("{} lässt sich nicht anlegen: {e}", dir.display()))?;

    let path = config_path()?;
    let json = serde_json::to_string_pretty(connection)
        .map_err(|e| format!("Verbindung lässt sich nicht schreiben: {e}"))?;

    fs::write(&path, json)
        .map_err(|e| format!("{} lässt sich nicht schreiben: {e}", path.display()))?;

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
        "Dieser Rechner ist noch nicht verbunden.\n\
         Führe zuerst aus:  ai-brain-cli connect <domain>"
            .to_string()
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
        fs::remove_file(&path)
            .map_err(|e| format!("{} lässt sich nicht löschen: {e}", path.display()))?;
        println!("Lokale Verbindung entfernt.");
    } else {
        println!("Es war keine Verbindung hinterlegt.");
    }

    println!();
    println!("Hinweis: Der Gerätezugang besteht auf der Instanz weiter.");
    println!("Endgültig entziehen kannst du ihn unter „Meine Geräte“.");

    Ok(())
}

pub fn claude_config_path() -> Result<PathBuf, String> {
    Ok(heimat()?.join(".claude.json"))
}

/// Trägt die CLI als MCP-Server in Claude Code ein.
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

    fs::write(&path, json)
        .map_err(|e| format!("{} lässt sich nicht schreiben: {e}", path.display()))?;

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
            PathBuf::from("/tmp/xdg/ai-brain-cli")
        );

        std::env::remove_var("XDG_CONFIG_HOME");
        assert_eq!(
            config_dir().unwrap(),
            PathBuf::from("/tmp/zuhause/.config/ai-brain-cli")
        );

        // Die Übernahme vom alten Namen (#7664). Steht in DIESEM Test, nicht in
        // einem eigenen — aus demselben Grund wie alles andere hier: der Umzug
        // haengt an XDG_CONFIG_HOME, und zwei Tests wuerden sich die Variable
        // gegenseitig wegziehen.
        let spielwiese =
            std::env::temp_dir().join(format!("ai-brain-cli-uebernahme-{}", std::process::id()));
        let _ = fs::remove_dir_all(&spielwiese);
        std::env::set_var("XDG_CONFIG_HOME", &spielwiese);

        let alt = spielwiese.join("ai-brain-companion");
        let neu = spielwiese.join("ai-brain-cli");

        let alt_anlegen = || {
            fs::create_dir_all(&alt).unwrap();
            fs::write(alt.join("connection.json"), "{}").unwrap();
            // Der Abgleichstand MUSS mitkommen — bleibt er zurueck, meldet
            // `sync` danach jeden Eintrag als geaendert.
            fs::write(alt.join("sync-state.json"), "ABGLEICH").unwrap();
        };

        // Fall 1: Der Rechner hat nur die alte Ablage.
        alt_anlegen();
        uebernimm_alte_ablage();

        assert!(!alt.exists(), "der leere alte Ordner muss verschwinden");
        assert_eq!(
            fs::read_to_string(neu.join("connection.json")).unwrap(),
            "{}"
        );
        assert_eq!(
            fs::read_to_string(neu.join("sync-state.json")).unwrap(),
            "ABGLEICH",
            "der Abgleichstand muss mit umziehen"
        );

        // Fall 2 — der Fall, der fast durchgerutscht waere: `install.sh` legt
        // die neue Ablage selbst an und schreibt `instanz` hinein, BEVOR dieses
        // Programm das erste Mal laeuft. Eine Pruefung auf „Ordner existiert
        // nicht" wuerde hier schweigen und die Zugangsdaten zurueck lassen.
        let _ = fs::remove_dir_all(&neu);
        fs::create_dir_all(&neu).unwrap();
        fs::write(neu.join("instanz"), "https://neue-instanz.example").unwrap();
        alt_anlegen();
        fs::write(alt.join("instanz"), "https://alte-instanz.example").unwrap();

        uebernimm_alte_ablage();

        assert_eq!(
            fs::read_to_string(neu.join("connection.json")).unwrap(),
            "{}",
            "die Verbindung muss trotz vorhandenem Ordner herueberkommen"
        );
        assert_eq!(
            fs::read_to_string(neu.join("instanz")).unwrap(),
            "https://neue-instanz.example",
            "die vom Installer geschriebene Instanz ist die aktuellere und bleibt"
        );
        assert!(
            alt.exists(),
            "blieb etwas stehen, bleibt auch der alte Ordner — kein Loeschweg"
        );

        // Fall 3: Ist die Verbindung schon drueben, wird nichts mehr angefasst.
        let _ = fs::remove_dir_all(&alt);
        alt_anlegen();
        fs::write(alt.join("connection.json"), "FREMD").unwrap();
        uebernimm_alte_ablage();
        assert_eq!(
            fs::read_to_string(neu.join("connection.json")).unwrap(),
            "{}",
            "eine vorhandene Verbindung darf nicht ueberschrieben werden"
        );

        let _ = fs::remove_dir_all(&spielwiese);

        for (k, wert) in sicherung {
            match wert {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}
