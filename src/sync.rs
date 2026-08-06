//! Repository-Synchronisation.
//!
//! Holt Skills und Agents aus dem Repository der Instanz und legt sie unter
//! `~/.claude/skills/` bzw. `~/.claude/agents/` ab.
//!
//! ## Die Regeln
//!
//! * Nur im Repository, nicht lokal → wird installiert.
//! * Nur lokal → bleibt unangetastet. Nicht löschen, nicht hochladen. Ein
//!   Skill, den jemand für sich gebaut hat, geht die geteilte Ablage nichts an.
//! * In beidem, gleich → nichts zu tun.
//! * In beidem, verschieden → **nicht** still überschreiben. Anzeigen, und beim
//!   Überschreiben die lokale Fassung als `.lokal` danebenlegen.
//! * Hooks werden **nie** eingerichtet. Ein Hook ist kein Dateibestand, sondern
//!   ein Befehl in der `settings.json`, der von selbst ausgeführt wird. „Ziehen
//!   ist harmlos, weil es nur ergänzt" gilt für Dateien — hier nicht.
//!
//! ## Wie Änderungen erkannt werden
//!
//! Nicht durch Nachrechnen der Server-Prüfsumme: Zwei Sprachen müssten dann
//! bis auf das letzte Escaping identisch serialisieren, und schon ein
//! Unterschied ließe alles als „geändert" erscheinen.
//!
//! Stattdessen merkt sich der Companion bei jedem Schreiben zwei Dinge: die
//! Prüfsumme, die der Server damals nannte, und einen selbst gebildeten Hash
//! über die Dateien, wie er sie hingelegt hat. Beim nächsten Lauf wird der
//! erste mit dem Server verglichen (hat sich dort etwas getan?) und der zweite
//! mit der Platte (hat hier jemand Hand angelegt?). Beide Vergleiche laufen
//! jeweils gegen den eigenen Wert — nie über die Sprachgrenze.

use crate::config::{self, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct Manifest {
    items: Vec<ManifestItem>,
    layout: BTreeMap<String, Layout>,
}

#[derive(Debug, Deserialize, Clone)]
struct ManifestItem {
    #[serde(rename = "type")]
    kind: String,
    slug: String,
    name: String,
    checksum: String,
}

#[derive(Debug, Deserialize, Clone)]
struct Layout {
    kind: String,
    #[serde(default)]
    directory: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ItemDetail {
    checksum: String,
    files: Vec<ItemFile>,
}

#[derive(Debug, Deserialize)]
struct ItemFile {
    path: String,
    content: String,
    /// `utf-8` oder `base64`. Nicht jede Datei im Repository ist Text — kami
    /// bringt eine 9 MB grosse ZIP mit. Ohne Kennzeichnung schriebe man deren
    /// Base64-Text als Datei und haette Datenmuell auf der Platte.
    #[serde(default = "default_encoding")]
    encoding: String,
}

fn default_encoding() -> String {
    "utf-8".to_string()
}

impl ItemFile {
    fn bytes(&self) -> Result<Vec<u8>, String> {
        match self.encoding.as_str() {
            "base64" => decode_base64(&self.content)
                .ok_or_else(|| format!("{} ist als base64 gekennzeichnet, laesst sich aber nicht dekodieren", self.path)),
            _ => Ok(self.content.as_bytes().to_vec()),
        }
    }
}

/// Base64 ohne zusaetzliche Abhaengigkeit — die Aufgabe ist klein genug.
fn decode_base64(input: &str) -> Option<Vec<u8>> {
    const TABELLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut wert = [255u8; 256];
    for (i, z) in TABELLE.iter().enumerate() {
        wert[*z as usize] = i as u8;
    }

    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut puffer = 0u32;
    let mut bits = 0u32;

    for z in input.bytes() {
        if z == b'=' || z == b'\n' || z == b'\r' {
            continue;
        }

        let v = wert[z as usize];

        if v == 255 {
            return None;
        }

        puffer = (puffer << 6) | v as u32;
        bits += 6;

        if bits >= 8 {
            bits -= 8;
            out.push((puffer >> bits) as u8);
        }
    }

    Some(out)
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SyncState {
    /// Schlüssel: `typ/slug`
    #[serde(default)]
    items: BTreeMap<String, SyncedItem>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SyncedItem {
    remote_checksum: String,
    local_hash: String,
}

/// Was mit einem Eintrag zu tun ist.
#[derive(Debug, PartialEq)]
enum Verdict {
    /// Liegt hier noch nicht.
    Neu,
    /// Instanz hat eine neuere Fassung, lokal wurde nichts angefasst.
    Aktualisiert,
    /// Lokal geändert, Instanz unverändert — bleibt in Ruhe.
    LokalGeaendert,
    /// Beide Seiten haben sich bewegt.
    Konflikt,
    /// Identisch.
    Unveraendert,
    /// Liegt lokal, der Companion hat es nie geschrieben, und der Inhalt
    /// weicht ab.
    Fremd,
    /// Liegt lokal, der Companion hat es nie geschrieben — aber der Inhalt ist
    /// identisch. Wird ohne Schreiben in den Stand übernommen.
    Uebernommen,
    /// Kein Dateibestand — wird grundsätzlich nicht angefasst.
    KeinDateibestand,
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mut apply = false;
    let mut force = false;
    let mut only: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--apply" => apply = true,
            "--force" => force = true,
            "--only" => {
                i += 1;
                only = Some(
                    args.get(i)
                        .ok_or_else(|| "--only braucht einen Namen.".to_string())?
                        .clone(),
                );
            }
            other => return Err(format!("Unbekannte Option: {other}")),
        }
        i += 1;
    }

    let connection = config::load()?;
    let token = crate::mcp::access_token(&connection)?;

    let manifest = fetch_manifest(&connection, &token)?;
    let mut state = load_state()?;
    let claude = claude_dir()?;

    let mut plan: Vec<(ManifestItem, Verdict, PathBuf)> = Vec::new();
    // Einträge, die schon richtig auf der Platte liegen: nur in den Stand
    // aufnehmen, nicht schreiben.
    let mut uebernehmen: Vec<(String, ItemDetail, PathBuf)> = Vec::new();

    for item in &manifest.items {
        if let Some(filter) = &only {
            if &item.slug != filter {
                continue;
            }
        }

        // Ein Eintrag ohne Slug hat kein Ziel — sein Pfad waere das
        // Elternverzeichnis. Ueberspringen statt daran zu scheitern: ein
        // einzelner fehlerhafter Datensatz auf der Instanz darf nicht jeden
        // Rechner lahmlegen (erlebt mit Item 189, Bug #542).
        if item.slug.trim().is_empty() {
            eprintln!(
                "Uebersprungen: ein Eintrag vom Typ {} hat keinen Namen — auf der Instanz nachtragen.",
                item.kind
            );
            continue;
        }

        let layout = manifest.layout.get(&item.kind);

        // Alles, was keine Dateien sind — heute nur Hooks. Bewusst hart:
        // lieber einen Typ zu viel aussparen als einen Befehl einrichten, den
        // niemand angefordert hat.
        if layout.map(|l| l.kind.as_str()) != Some("directory") {
            plan.push((item.clone(), Verdict::KeinDateibestand, PathBuf::new()));
            continue;
        }

        let directory = layout
            .and_then(|l| l.directory.clone())
            .unwrap_or_else(|| format!("{}s", item.kind));

        let target = claude.join(&directory).join(&item.slug);
        let key = format!("{}/{}", item.kind, item.slug);

        let verdict = match (target.exists(), state.items.get(&key)) {
            (false, _) => Verdict::Neu,
            // Verzeichnis da, aber der Companion kennt es nicht. Statt es
            // pauschal als fremd zu behandeln, wird nachgesehen: Auf einem
            // Rechner, der die Skills schon von Hand hat, sind sie meist
            // identisch — dann gibt es nichts zu tun ausser sie zu übernehmen.
            //
            // Verglichen werden die Dateien selbst, nicht Prüfsummen. Das ist
            // der einzige Vergleich, der über die Sprachgrenze hinweg
            // verlässlich ist.
            (true, None) => match fetch_item(&connection, &token, &item.kind, &item.slug) {
                Ok(detail) => {
                    if files_match(&target, &detail)? {
                        uebernehmen.push((key.clone(), detail, target.clone()));
                        Verdict::Uebernommen
                    } else {
                        Verdict::Fremd
                    }
                }
                // Laesst sich der Eintrag nicht holen, ist das kein Grund den
                // ganzen Lauf abzubrechen. Im Zweifel gilt: nicht anfassen.
                Err(err) => {
                    eprintln!("Konnte {}/{} nicht pruefen: {err}", item.kind, item.slug);
                    Verdict::Fremd
                }
            },
            (true, Some(known)) => {
                let lokal_gleich = hash_directory(&target)? == known.local_hash;
                let fern_gleich = item.checksum == known.remote_checksum;

                match (fern_gleich, lokal_gleich) {
                    (true, true) => Verdict::Unveraendert,
                    (false, true) => Verdict::Aktualisiert,
                    (true, false) => Verdict::LokalGeaendert,
                    (false, false) => Verdict::Konflikt,
                }
            }
        };

        plan.push((item.clone(), verdict, target));
    }

    // Übernehmen kostet nichts und nimmt niemandem etwas weg — das passiert
    // auch ohne --apply, sonst meldet jeder Lauf dieselben Einträge erneut.
    if !uebernehmen.is_empty() {
        for (key, detail, target) in &uebernehmen {
            state.items.insert(
                key.clone(),
                SyncedItem {
                    remote_checksum: detail.checksum.clone(),
                    local_hash: hash_directory(target)?,
                },
            );
        }

        save_state(&state)?;
    }

    report(&plan);

    if !apply {
        println!();
        println!("Nichts geschrieben. Zum Anwenden:  ai-brain-companion sync --apply");
        return Ok(());
    }

    let mut geschrieben = 0usize;
    let mut uebersprungen = 0usize;
    let mut fehlgeschlagen = 0usize;

    for (item, verdict, target) in &plan {
        let schreiben = match verdict {
            Verdict::Neu | Verdict::Aktualisiert => true,
            Verdict::Konflikt | Verdict::Fremd => force,
            _ => false,
        };

        if !schreiben {
            if matches!(verdict, Verdict::Konflikt | Verdict::Fremd) {
                uebersprungen += 1;
            }
            continue;
        }

        let detail = match fetch_item(&connection, &token, &item.kind, &item.slug) {
            Ok(detail) => detail,
            // Ein Eintrag, der sich nicht holen laesst, darf nicht die
            // restlichen 160 verhindern. Genau das ist passiert: skill/kami
            // antwortete mit 500, und der Lauf endete mittendrin.
            Err(err) => {
                eprintln!("Uebersprungen: {err}");
                fehlgeschlagen += 1;
                continue;
            }
        };

        // Vor dem Überschreiben die lokale Fassung sichern. Eine Entscheidung,
        // die sich rückgängig machen lässt, ist nie die falsche.
        //
        // Die Sicherung liegt bewusst NICHT neben dem Original: Claude Code
        // durchsucht ~/.claude/skills/ und lud ein dort abgelegtes
        // `pm-task.lokal` prompt als zweiten, gleichnamigen Skill. Eine
        // Sicherung, die das System verwirrt, ist keine.
        if target.exists() && matches!(verdict, Verdict::Konflikt | Verdict::Fremd) {
            let backup = backup_dir()?.join(&item.kind).join(&item.slug);

            if backup.exists() {
                fs::remove_dir_all(&backup).ok();
            }

            copy_directory(target, &backup)?;
            println!("  Lokale Fassung gesichert: {}", backup.display());
        }

        write_item(target, &detail)?;

        state.items.insert(
            format!("{}/{}", item.kind, item.slug),
            SyncedItem {
                remote_checksum: detail.checksum.clone(),
                local_hash: hash_directory(target)?,
            },
        );

        geschrieben += 1;
    }

    save_state(&state)?;

    println!();
    println!("{geschrieben} geschrieben.");

    if fehlgeschlagen > 0 {
        println!("{fehlgeschlagen} nicht abrufbar — siehe Meldungen oben.");
    }

    if uebersprungen > 0 {
        println!(
            "{uebersprungen} übersprungen, weil hier Änderungen liegen. Mit --force überschreiben \
             (die lokale Fassung wird dabei als .lokal gesichert)."
        );
    }

    Ok(())
}

fn report(plan: &[(ManifestItem, Verdict, PathBuf)]) {
    let zeige = |verdict: &Verdict, titel: &str, hinweis: &str| {
        let treffer: Vec<&(ManifestItem, Verdict, PathBuf)> =
            plan.iter().filter(|(_, v, _)| v == verdict).collect();

        if treffer.is_empty() {
            return;
        }

        println!();
        println!("{titel} ({})", treffer.len());

        if !hinweis.is_empty() {
            println!("  {hinweis}");
        }

        for (item, _, _) in treffer.iter().take(20) {
            println!("  - {} {}", item.kind, item.name);
        }

        if treffer.len() > 20 {
            println!("  … und {} weitere", treffer.len() - 20);
        }
    };

    zeige(&Verdict::Neu, "Neu", "");
    zeige(&Verdict::Aktualisiert, "Aktualisiert", "");
    zeige(
        &Verdict::Konflikt,
        "Konflikt",
        "hier UND dort geändert — wird ohne --force nicht angefasst",
    );
    zeige(
        &Verdict::Uebernommen,
        "Lag schon richtig hier",
        "inhaltlich identisch — ab jetzt bekannt, nichts geschrieben",
    );
    zeige(
        &Verdict::Fremd,
        "Liegt hier in anderer Fassung",
        "wird ohne --force nicht angefasst",
    );
    zeige(
        &Verdict::LokalGeaendert,
        "Lokal geändert",
        "bleibt so, die Instanz hat sich nicht bewegt",
    );
    zeige(
        &Verdict::KeinDateibestand,
        "Nicht als Datei ablegbar",
        "Hooks sind Befehle in der settings.json — die richtet der Companion nicht selbst ein",
    );

    let unveraendert = plan.iter().filter(|(_, v, _)| *v == Verdict::Unveraendert).count();

    if unveraendert > 0 {
        println!();
        println!("Unverändert: {unveraendert}");
    }
}

fn fetch_manifest(connection: &Connection, token: &str) -> Result<Manifest, String> {
    let url = format!("{}/api/v1/repository/manifest", connection.base_url);

    ureq::get(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/json")
        .call()
        .map_err(|e| format!("Liste nicht abrufbar: {e}"))?
        .into_json()
        .map_err(|e| format!("Liste ist unlesbar: {e}"))
}

fn fetch_item(connection: &Connection, token: &str, kind: &str, slug: &str) -> Result<ItemDetail, String> {
    let url = format!("{}/api/v1/repository/items/{kind}/{slug}", connection.base_url);

    ureq::get(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/json")
        .call()
        .map_err(|e| format!("{kind}/{slug} nicht abrufbar: {e}"))?
        .into_json()
        .map_err(|e| format!("{kind}/{slug} ist unlesbar: {e}"))
}

fn write_item(target: &Path, detail: &ItemDetail) -> Result<(), String> {
    for file in &detail.files {
        // Pfade aus der Instanz landen im Dateisystem. Ein `..` darin wuerde
        // ausserhalb des Zielverzeichnisses schreiben — das ist keine Theorie,
        // sondern der klassische Weg, ueber eine Synchronisation fremde Dateien
        // zu ueberschreiben.
        if file.path.contains("..") || file.path.starts_with('/') {
            return Err(format!("Verdaechtiger Pfad abgelehnt: {}", file.path));
        }

        let full = target.join(&file.path);

        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("{} lässt sich nicht anlegen: {e}", parent.display()))?;
        }

        fs::write(&full, file.bytes()?)
            .map_err(|e| format!("{} lässt sich nicht schreiben: {e}", full.display()))?;
    }

    Ok(())
}

fn copy_directory(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("{} lässt sich nicht anlegen: {e}", to.display()))?;

    for entry in fs::read_dir(from).map_err(|e| format!("{} nicht lesbar: {e}", from.display()))? {
        let entry = entry.map_err(|e| format!("Eintrag nicht lesbar: {e}"))?;
        let ziel = to.join(entry.file_name());

        if entry.path().is_dir() {
            copy_directory(&entry.path(), &ziel)?;
        } else {
            fs::copy(entry.path(), &ziel)
                .map_err(|e| format!("{} nicht kopierbar: {e}", entry.path().display()))?;
        }
    }

    Ok(())
}

/// Stimmt das, was auf der Platte liegt, mit dem Eintrag der Instanz überein?
///
/// Verglichen werden die Dateien der Instanz. Zusätzliche lokale Dateien
/// (`.DS_Store`, Notizen, was auch immer) machen den Eintrag NICHT
/// unterschiedlich — sie wären sonst der Grund, warum auf jedem zweiten
/// Rechner ein Konflikt gemeldet wird, den niemand verursacht hat.
fn files_match(target: &Path, detail: &ItemDetail) -> Result<bool, String> {
    for file in &detail.files {
        let full = target.join(&file.path);

        if !full.exists() {
            return Ok(false);
        }

        let vorhanden = fs::read(&full).map_err(|e| format!("{} nicht lesbar: {e}", full.display()))?;

        if vorhanden != file.bytes()? {
            return Ok(false);
        }
    }

    Ok(true)
}

/// Hash über alle Dateien eines Verzeichnisses — Pfade eingeschlossen, sortiert.
///
/// Wird ausschließlich mit sich selbst verglichen, nie mit der Server-Seite.
fn hash_directory(dir: &Path) -> Result<String, String> {
    let mut dateien: Vec<(String, Vec<u8>)> = Vec::new();
    sammle(dir, dir, &mut dateien)?;
    dateien.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();

    for (pfad, inhalt) in dateien {
        hasher.update(pfad.as_bytes());
        hasher.update([0u8]);
        hasher.update(&inhalt);
        hasher.update([0u8]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn sammle(wurzel: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir).map_err(|e| format!("{} nicht lesbar: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("Eintrag nicht lesbar: {e}"))?;
        let pfad = entry.path();

        if pfad.is_dir() {
            sammle(wurzel, &pfad, out)?;
            continue;
        }

        let relativ = pfad
            .strip_prefix(wurzel)
            .map_err(|_| "Pfad ausserhalb des Verzeichnisses".to_string())?
            .to_string_lossy()
            .to_string();

        let inhalt = fs::read(&pfad).map_err(|e| format!("{} nicht lesbar: {e}", pfad.display()))?;

        out.push((relativ, inhalt));
    }

    Ok(())
}

fn claude_dir() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or_else(|| "HOME ist nicht gesetzt.".to_string())?;

    Ok(PathBuf::from(home).join(".claude"))
}

/// Wo überschriebene lokale Fassungen landen.
///
/// Ausserhalb von ~/.claude/, damit Claude Code sie nicht als Skills einliest.
fn backup_dir() -> Result<PathBuf, String> {
    Ok(config::config_dir()?.join("gesicherte-fassungen"))
}

fn state_path() -> Result<PathBuf, String> {
    Ok(config::config_dir()?.join("sync-state.json"))
}

fn load_state() -> Result<SyncState, String> {
    let path = state_path()?;

    if !path.exists() {
        return Ok(SyncState::default());
    }

    let raw = fs::read_to_string(&path).map_err(|e| format!("{} nicht lesbar: {e}", path.display()))?;

    serde_json::from_str(&raw).map_err(|e| format!("{} ist beschädigt: {e}", path.display()))
}

fn save_state(state: &SyncState) -> Result<(), String> {
    let path = state_path()?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{} lässt sich nicht anlegen: {e}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(state).map_err(|e| format!("Zustand nicht schreibbar: {e}"))?;

    fs::write(&path, json).map_err(|e| format!("{} lässt sich nicht schreiben: {e}", path.display()))
}
