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
//! * MCP-Server (Typ `mcp`) sind ebenfalls Befehle. Sie werden nur
//!   eingetragen, wenn dieser Rechner sie **einzeln** gewählt hat — nie über
//!   „alles". Die Regeln dafür stehen in `mcp_eintraege`.
//!
//! ## Wie Änderungen erkannt werden
//!
//! Nicht durch Nachrechnen der Server-Prüfsumme: Zwei Sprachen müssten dann
//! bis auf das letzte Escaping identisch serialisieren, und schon ein
//! Unterschied ließe alles als „geändert" erscheinen.
//!
//! Stattdessen merkt sich die CLI bei jedem Schreiben zwei Dinge: die
//! Prüfsumme, die der Server damals nannte, und einen selbst gebildeten Hash
//! über die Dateien, wie er sie hingelegt hat. Beim nächsten Lauf wird der
//! erste mit dem Server verglichen (hat sich dort etwas getan?) und der zweite
//! mit der Platte (hat hier jemand Hand angelegt?). Beide Vergleiche laufen
//! jeweils gegen den eigenen Wert — nie über die Sprachgrenze.

use crate::config::{self, Connection};
use crate::mcp_eintraege::{self, Lage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Je MCP-Server die Orte, an denen er ausgetragen werden soll: Pfad der
/// Claude-Konfiguration, oder `None` für Codex.
type McpOrte = Vec<(String, Option<PathBuf>)>;

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
    /// Hash über das Verzeichnis — bei MCP-Servern leer, die stehen in `orte`.
    #[serde(default)]
    local_hash: String,
    /// Nur MCP-Server: je Ort (Pfad einer Claude-Konfiguration oder `codex`)
    /// der eigene Hash über den Eintrag, wie der Harness ihn geschrieben hat.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    orte: BTreeMap<String, String>,
}

/// Ein MCP-Server und wie es an jedem Ort um ihn steht.
struct McpPlan {
    item: ManifestItem,
    definition: mcp_eintraege::Definition,
    orte: Vec<McpOrt>,
}

struct McpOrt {
    /// Schlüssel im Abgleichstand: Pfad der Konfiguration oder `codex`.
    ort: String,
    /// `None` bei Codex.
    pfad: Option<PathBuf>,
    vorhanden: Option<serde_json::Value>,
    gewuenscht: serde_json::Value,
    lage: Lage,
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
    /// Liegt lokal, die CLI hat es nie geschrieben, und der Inhalt
    /// weicht ab.
    Fremd,
    /// Liegt lokal, die CLI hat es nie geschrieben — aber der Inhalt ist
    /// identisch. Wird ohne Schreiben in den Stand übernommen.
    Uebernommen,
    /// Kein Dateibestand — wird grundsätzlich nicht angefasst.
    KeinDateibestand,
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mut apply = false;
    let mut force = false;
    let mut aufraeumen = false;
    let mut zugestimmt = false;
    let mut only: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--apply" => apply = true,
            "--force" => force = true,
            "--aufraeumen" => aufraeumen = true,
            // Vorab geantwortet: nicht fragen, sondern die sichere Vorgabe
            // nehmen. Anwenden ja — aber nichts überschreiben und nichts
            // entfernen; dafür bleiben --force und --aufraeumen nötig.
            "--ja" | "--headless" => {
                zugestimmt = true;
                apply = true;
            }
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
    let mut mcp_kandidaten: Vec<ManifestItem> = Vec::new();

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

        // MCP-Server werden weiter unten geplant — erst muss feststehen, was
        // dieser Rechner ausdrücklich gewählt hat.
        if item.kind == "mcp" {
            mcp_kandidaten.push(item.clone());
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
            // Verzeichnis da, aber die CLI kennt es nicht. Statt es
            // pauschal als fremd zu behandeln, wird nachgesehen: Auf einem
            // Rechner, der die Skills schon von Hand hat, sind sie meist
            // identisch — dann gibt es nichts zu tun ausser sie zu übernehmen.
            //
            // Verglichen werden die Dateien selbst, nicht Prüfsummen. Das ist
            // der einzige Vergleich, der über die Sprachgrenze hinweg
            // verlässlich ist.
            (true, None) => match fetch_item_ueber(&connection, &token, &item.kind, &item.slug) {
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
                    // Laut Stand haben sich beide Seiten bewegt — ein Konflikt
                    // ist das aber erst, wenn der Inhalt wirklich auseinander
                    // laeuft. Wurde der Eintrag auf einem anderen Weg
                    // geschrieben (serverseitig ueber das Install-Werkzeug, von
                    // Hand, durch ein zweites Werkzeug), ist nur die
                    // Buchfuehrung der CLI veraltet; die Dateien sind
                    // laengst dieselben.
                    //
                    // Also erst nachsehen, dann urteilen — sonst meldet jeder
                    // Lauf denselben Konflikt, der keiner ist, und die echten
                    // gehen darin unter. Der Abruf kostet eine Anfrage, faellt
                    // aber nur im seltenen Konfliktfall an.
                    (false, false) => match fetch_item_ueber(&connection, &token, &item.kind, &item.slug) {
                        Ok(detail) => {
                            if files_match(&target, &detail)? {
                                uebernehmen.push((key.clone(), detail, target.clone()));
                                Verdict::Uebernommen
                            } else {
                                Verdict::Konflikt
                            }
                        }
                        // Im Zweifel beim strengeren Urteil bleiben: lieber
                        // einen Konflikt melden, den jemand ansieht, als
                        // ungeprueft zu uebernehmen.
                        Err(err) => {
                            eprintln!("Konnte {}/{} nicht pruefen: {err}", item.kind, item.slug);
                            Verdict::Konflikt
                        }
                    },
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
                    orte: BTreeMap::new(),
                },
            );
        }

        save_state(&state)?;
    }

    // ------------------------------------------------------ MCP-Server ----
    //
    // Was ausdrücklich gewählt ist, fragt der Harness selbst noch einmal bei
    // der Instanz nach, statt sich allein auf das Manifest zu verlassen: Ein
    // Server, der über „alles" hereinrutscht, liefe auf diesem Rechner, ohne
    // dass ihn hier jemand angefordert hat.
    let mcp_bekannt = state.items.keys().any(|k| k.starts_with("mcp/"));
    let mcp_im_manifest = manifest.items.iter().any(|i| i.kind == "mcp");

    let gewaehlt: Option<crate::select::Auswahl> = if mcp_bekannt || mcp_im_manifest {
        match crate::select::ausdruecklich_gewaehlt(&connection, &token) {
            Ok(liste) => Some(liste),
            Err(err) => {
                eprintln!("Auswahl dieses Rechners nicht abrufbar — MCP-Server bleiben unangetastet: {err}");
                None
            }
        }
    } else {
        Some(crate::select::Auswahl::leer())
    };

    let browser = mcp_eintraege::browser_finden();
    let claude_orte = mcp_eintraege::claude_konfigurationen(
        &crate::config::heimat()?,
        std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).as_deref(),
    );
    let codex_da = mcp_eintraege::codex_vorhanden();

    let mut mcp_plan: Vec<McpPlan> = Vec::new();
    let mut mcp_nicht_gewaehlt: Vec<ManifestItem> = Vec::new();
    let mut mcp_fehlgeschlagen = 0usize;

    for item in &mcp_kandidaten {
        // Typgenau: Ein gleichnamiger Skill wählt den Server nicht mit (#7891).
        let ausdruecklich = gewaehlt
            .as_ref()
            .map(|g| g.enthaelt("mcp", &item.slug))
            .unwrap_or(false);

        if !ausdruecklich {
            mcp_nicht_gewaehlt.push(item.clone());
            continue;
        }

        let definition = match fetch_bundle_roh(&connection, &token, &item.kind, &item.slug)
            .and_then(|b| mcp_eintraege::definition_lesen(&b).map_err(|e| format!("mcp/{}: {e}", item.slug)))
        {
            Ok(d) => d,
            Err(err) => {
                eprintln!("Uebersprungen: {err}");
                mcp_fehlgeschlagen += 1;
                continue;
            }
        };

        let key = format!("mcp/{}", item.slug);
        let bekannt = state.items.get(&key);
        let fern_gleich = bekannt.map(|b| b.remote_checksum == item.checksum).unwrap_or(false);

        let mut orte: Vec<McpOrt> = Vec::new();

        if definition.fuer_claude() {
            let gewuenscht = definition.claude_eintrag(browser.as_deref());

            for pfad in &claude_orte {
                let ort = pfad.to_string_lossy().to_string();
                let vorhanden = mcp_eintraege::claude_lesen(pfad, &item.slug)?;
                let lage = mcp_eintraege::urteilen(
                    vorhanden.as_ref(),
                    &gewuenscht,
                    bekannt.and_then(|b| b.orte.get(&ort)).map(String::as_str),
                    fern_gleich,
                );

                orte.push(McpOrt { ort, pfad: Some(pfad.clone()), vorhanden, gewuenscht: gewuenscht.clone(), lage });
            }
        }

        if definition.fuer_codex() && codex_da {
            let gewuenscht = definition.codex_eintrag(browser.as_deref());
            let vorhanden = mcp_eintraege::codex_lesen(&item.slug)?;
            let lage = mcp_eintraege::urteilen(
                vorhanden.as_ref(),
                &gewuenscht,
                bekannt.and_then(|b| b.orte.get(mcp_eintraege::CODEX)).map(String::as_str),
                fern_gleich,
            );

            orte.push(McpOrt {
                ort: mcp_eintraege::CODEX.to_string(),
                pfad: None,
                vorhanden,
                gewuenscht,
                lage,
            });
        }

        mcp_plan.push(McpPlan { item: item.clone(), definition, orte });
    }

    // Wie bei Dateien: Was schon genau so dasteht, wird ohne Schreiben in den
    // Stand übernommen — auch ohne --apply.
    let mut mcp_uebernommen = false;

    for plan in &mcp_plan {
        for ort in plan.orte.iter().filter(|o| o.lage == Lage::Uebernommen) {
            if let Some(vorhanden) = &ort.vorhanden {
                stand_merken(&mut state, &plan.item, &ort.ort, vorhanden);
                mcp_uebernommen = true;
            }
        }
    }

    if mcp_uebernommen {
        save_state(&state)?;
    }

    // Nicht mehr gewählte MCP-Server: nur was der Harness selbst geschrieben
    // hat und was seither unverändert dasteht. Ohne verlässliche Auswahl wird
    // hier gar nichts entschieden.
    let mut mcp_ueberzaehlig: Vec<(String, McpOrte)> = Vec::new();

    if let Some(gewaehlt) = &gewaehlt {
        let aktiv: BTreeSet<String> = manifest
            .items
            .iter()
            .filter(|i| i.kind == "mcp" && gewaehlt.enthaelt("mcp", &i.slug))
            .map(|i| format!("mcp/{}", i.slug))
            .collect();

        for (key, bekannt) in state.items.iter().filter(|(k, _)| k.starts_with("mcp/")) {
            if aktiv.contains(key) {
                continue;
            }

            let slug = &key["mcp/".len()..];
            let mut weg: McpOrte = Vec::new();

            for (ort, hash) in &bekannt.orte {
                let (vorhanden, pfad) = if ort == mcp_eintraege::CODEX {
                    if !codex_da {
                        continue;
                    }
                    (mcp_eintraege::codex_lesen(slug)?, None)
                } else {
                    let pfad = PathBuf::from(ort);
                    (mcp_eintraege::claude_lesen(&pfad, slug)?, Some(pfad))
                };

                if vorhanden.is_none() {
                    continue;
                }

                if mcp_eintraege::entfernbar(vorhanden.as_ref(), hash) {
                    weg.push((ort.clone(), pfad));
                } else {
                    println!();
                    println!("Nicht mehr ausgewaehlt, aber hier geaendert — bleibt liegen: {key} in {ort}");
                }
            }

            mcp_ueberzaehlig.push((key.clone(), weg));
        }
    }

    // Was die CLI einmal geschrieben hat, jetzt aber nicht mehr im
    // Manifest steht: abgewaehlt oder aus dem Repository verschwunden.
    //
    // Entfernt wird NUR, was er selbst angelegt hat UND was seither unveraendert
    // ist. Eigenbauten, veraenderte Fassungen und alles, was er nie geschrieben
    // hat, bleibt liegen — auch mit --aufraeumen. Eine Synchronisation, die
    // fremde Arbeit loeschen kann, ist keine Synchronisation mehr.
    let im_manifest: std::collections::BTreeSet<String> = manifest
        .items
        .iter()
        .map(|i| format!("{}/{}", i.kind, i.slug))
        .collect();

    let mut ueberzaehlig: Vec<(String, PathBuf)> = Vec::new();

    for (key, bekannt) in &state.items {
        if im_manifest.contains(key) || key.starts_with("mcp/") {
            continue;
        }

        let (kind, slug) = match key.split_once('/') {
            Some(paar) => paar,
            None => continue,
        };

        let verzeichnis = claude
            .join(if kind == "agent" { "agents" } else { "skills" })
            .join(slug);

        if !verzeichnis.exists() {
            continue;
        }

        if hash_directory(&verzeichnis)? == bekannt.local_hash {
            ueberzaehlig.push((key.clone(), verzeichnis));
        } else {
            println!();
            println!(
                "Nicht mehr ausgewaehlt, aber hier geaendert — bleibt liegen: {key}"
            );
        }
    }

    if !ueberzaehlig.is_empty() {
        println!();
        println!("Nicht mehr ausgewaehlt ({})", ueberzaehlig.len());

        if !aufraeumen {
            println!("  bleibt liegen — mit --aufraeumen entfernen");
        }

        for (key, _) in ueberzaehlig.iter().take(20) {
            println!("  - {key}");
        }
    }

    let mcp_weg: usize = mcp_ueberzaehlig.iter().map(|(_, w)| w.len()).sum();

    if mcp_weg > 0 {
        println!();
        println!("MCP-Server nicht mehr ausgewaehlt ({mcp_weg} Eintrag/Eintraege)");

        if !aufraeumen {
            println!("  bleibt eingetragen — mit --aufraeumen austragen");
        }

        for (key, weg) in &mcp_ueberzaehlig {
            for (ort, _) in weg {
                println!("  - {key} in {ort}");
            }
        }
    }

    report(&plan);
    report_mcp(&mcp_plan, &mcp_nicht_gewaehlt);

    // Ein ausdrückliches --apply beantwortet die Frage schon; gefragt wird nur,
    // wenn jemand da ist, der antworten kann.
    if !apply && crate::frage::darf_fragen(zugestimmt) {
        let etwas_zu_tun = plan.iter().any(|(_, v, _)| {
            matches!(v, Verdict::Neu | Verdict::Aktualisiert | Verdict::Konflikt | Verdict::Fremd)
        }) || !ueberzaehlig.is_empty()
            || mcp_weg > 0
            || mcp_plan.iter().any(|p| {
                p.orte.iter().any(|o| {
                    matches!(o.lage, Lage::Neu | Lage::Aktualisiert | Lage::Konflikt | Lage::Fremd)
                })
            });

        if etwas_zu_tun {
            println!();
            apply = crate::frage::ja_nein("Anwenden?", false)?;
        }
    }

    if !apply {
        println!();
        println!("Nichts geschrieben. Zum Anwenden:  ai-brain-harness sync --apply");
        return Ok(());
    }

    let mut geschrieben = 0usize;
    let mut uebersprungen = 0usize;
    let mut fehlgeschlagen = 0usize;
    let mut entfernt = 0usize;

    for (item, verdict, target) in &plan {
        let strittig = matches!(verdict, Verdict::Konflikt | Verdict::Fremd);
        let unstrittig = matches!(verdict, Verdict::Neu | Verdict::Aktualisiert);

        if !strittig && !unstrittig {
            continue;
        }

        // Strittig, kein --force, und niemand da zum Fragen: liegen lassen.
        // Im Zweifel nichts anfassen — das ist die Vorgabe, die keinen Schaden
        // anrichtet.
        if strittig && !force && !crate::frage::darf_fragen(zugestimmt) {
            uebersprungen += 1;
            continue;
        }

        let detail = match fetch_item_ueber(&connection, &token, &item.kind, &item.slug) {
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

        // Je Eintrag entscheiden statt pauschal --force: Wer --force setzt, um
        // einen einzigen Eintrag loszuwerden, ueberschreibt im selben Zug alle
        // anderen mit.
        if strittig && !force && !entscheiden(item, verdict, target, &detail)? {
            uebersprungen += 1;
            continue;
        }

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
                orte: BTreeMap::new(),
            },
        );

        geschrieben += 1;
    }

    // MCP-Server: je Ort entscheiden, aber je Server nur EINMAL fragen —
    // drei Claude-Konten sollen nicht dreimal dieselbe Frage auslösen.
    let mut gesichert: BTreeSet<PathBuf> = BTreeSet::new();

    for plan in &mcp_plan {
        let strittig: Vec<&McpOrt> = plan
            .orte
            .iter()
            .filter(|o| matches!(o.lage, Lage::Konflikt | Lage::Fremd))
            .collect();

        let strittige_schreiben = if strittig.is_empty() {
            false
        } else if force {
            true
        } else if !crate::frage::darf_fragen(zugestimmt) {
            false
        } else {
            entscheiden_mcp(plan, &strittig)?
        };

        for ort in &plan.orte {
            let schreiben = match ort.lage {
                Lage::Neu | Lage::Aktualisiert => true,
                Lage::Konflikt | Lage::Fremd => strittige_schreiben,
                _ => false,
            };

            if !schreiben {
                if matches!(ort.lage, Lage::Konflikt | Lage::Fremd) {
                    uebersprungen += 1;
                }
                continue;
            }

            let ergebnis = match &ort.pfad {
                Some(pfad) => mcp_eintraege::claude_schreiben(pfad, &plan.item.slug, Some(&ort.gewuenscht), &mut gesichert),
                None => {
                    // Codex hat keine Datei, die wir sichern koennten — die
                    // alte Angabe wird deshalb daneben abgelegt.
                    if let Some(alt) = &ort.vorhanden {
                        mcp_sicherung_ablegen(&plan.item.slug, alt)?;
                    }
                    mcp_eintraege::codex_schreiben(&plan.item.slug, &plan.definition, browser.as_deref())
                }
            };

            match ergebnis {
                Ok(()) => {
                    // Nachlesen statt annehmen: Codex legt den Eintrag selbst
                    // ab, und nur was wirklich dasteht, taugt als Vergleichswert.
                    let steht = match &ort.pfad {
                        Some(pfad) => mcp_eintraege::claude_lesen(pfad, &plan.item.slug)?,
                        None => mcp_eintraege::codex_lesen(&plan.item.slug)?,
                    };

                    stand_merken(&mut state, &plan.item, &ort.ort, steht.as_ref().unwrap_or(&ort.gewuenscht));
                    geschrieben += 1;
                }
                Err(err) => {
                    eprintln!("Uebersprungen: mcp/{} in {}: {err}", plan.item.slug, ort.ort);
                    fehlgeschlagen += 1;
                }
            }
        }
    }

    fehlgeschlagen += mcp_fehlgeschlagen;

    // Entfernen ist die einzige Handlung, die etwas wegnimmt — dafür wird
    // ausdrücklich gefragt, und die Vorgabe ist Nein.
    if !aufraeumen
        && (!ueberzaehlig.is_empty() || mcp_weg > 0)
        && crate::frage::darf_fragen(zugestimmt)
    {
        println!();
        aufraeumen = crate::frage::ja_nein(
            &format!(
                "{} nicht mehr ausgewaehlte Eintraege entfernen?",
                ueberzaehlig.len() + mcp_weg
            ),
            false,
        )?;
    }

    if aufraeumen {
        for (key, verzeichnis) in &ueberzaehlig {
            fs::remove_dir_all(verzeichnis)
                .map_err(|e| format!("{} nicht entfernbar: {e}", verzeichnis.display()))?;

            state.items.remove(key);
            entfernt += 1;
        }

        for (key, weg) in &mcp_ueberzaehlig {
            let slug = &key["mcp/".len()..];
            let mut alle_weg = true;

            for (ort, pfad) in weg {
                let ergebnis = match pfad {
                    Some(pfad) => mcp_eintraege::claude_schreiben(pfad, slug, None, &mut gesichert),
                    None => mcp_eintraege::codex_entfernen(slug),
                };

                match ergebnis {
                    Ok(()) => {
                        if let Some(bekannt) = state.items.get_mut(key) {
                            bekannt.orte.remove(ort);
                        }
                        entfernt += 1;
                    }
                    Err(err) => {
                        eprintln!("{key} in {ort} nicht austragbar: {err}");
                        alle_weg = false;
                    }
                }
            }

            // Was hier geaendert wurde, bleibt im Stand — sonst gilt es beim
            // naechsten Lauf als fremd, obwohl der Harness es einmal schrieb.
            let leer = state.items.get(key).map(|b| b.orte.is_empty()).unwrap_or(true);

            if alle_weg && leer {
                state.items.remove(key);
            }
        }
    }

    save_state(&state)?;

    println!();
    println!("{geschrieben} geschrieben.");

    if entfernt > 0 {
        println!("{entfernt} entfernt.");
    }

    if fehlgeschlagen > 0 {
        println!("{fehlgeschlagen} nicht abrufbar — siehe Meldungen oben.");
    }

    if uebersprungen > 0 {
        println!(
            "{uebersprungen} übersprungen, weil hier Änderungen liegen. Am Terminal wird je \
             Eintrag gefragt; --force überschreibt sie alle. Die hiesige Fassung wird dabei \
             gesichert."
        );
    }

    Ok(())
}

/// Was mit einem strittigen Eintrag geschehen soll — einzeln gefragt.
///
/// Bisher gab es dafür nur `--force`, und das gilt für den ganzen Lauf: Wer
/// einen einzigen Eintrag loswerden wollte, überschrieb alle anderen mit. Die
/// Vorgabe ist deshalb „behalten" — die Antwort, die nichts kaputt macht.
fn entscheiden(
    item: &ManifestItem,
    verdict: &Verdict,
    target: &Path,
    detail: &ItemDetail,
) -> Result<bool, String> {
    let lage = match verdict {
        Verdict::Konflikt => "hier UND auf der Instanz geaendert",
        _ => "liegt hier, stammt aber nicht vom ai-brain-harness",
    };

    println!();
    println!("{} {} — {lage}", item.kind, item.slug);

    loop {
        match crate::frage::auswahl(
            "Was damit tun?",
            &[
                ('b', "behalten — die hiesige Fassung bleibt"),
                (
                    'u',
                    "ueberschreiben — die Fassung der Instanz gilt (hiesige wird gesichert)",
                ),
                ('d', "Unterschied ansehen"),
            ],
            'b',
        )? {
            'b' => return Ok(false),
            'u' => return Ok(true),
            _ => unterschied_zeigen(target, detail)?,
        }
    }
}

/// Zeigt, worin sich die hiesige Fassung von der der Instanz unterscheidet.
///
/// Bewusst kein vollwertiger Diff: Verglichen wird Datei für Datei, und bei
/// Textdateien werden abweichende Zeilen nach Zeilennummer gegenübergestellt.
/// Das beantwortet die Frage, die vor der Entscheidung steht — „ist das meine
/// Änderung oder etwas Fremdes?" — ohne eine Vergleichsmechanik, deren Ausgabe
/// niemand nachprüfen kann.
fn unterschied_zeigen(target: &Path, detail: &ItemDetail) -> Result<(), String> {
    println!();

    let mut abweichend = 0usize;
    let mut bekannt: BTreeSet<String> = BTreeSet::new();

    for datei in &detail.files {
        bekannt.insert(datei.path.clone());

        let hier = target.join(&datei.path);
        let dort = datei.bytes()?;

        if !hier.exists() {
            println!("  nur auf der Instanz: {}", datei.path);
            abweichend += 1;
            continue;
        }

        let lokal =
            fs::read(&hier).map_err(|e| format!("{} nicht lesbar: {e}", hier.display()))?;

        if lokal == dort {
            continue;
        }

        abweichend += 1;
        println!("  verschieden: {}", datei.path);

        // Binärdateien nicht auf den Schirm kippen — Größe sagt hier mehr.
        match (String::from_utf8(lokal.clone()), String::from_utf8(dort)) {
            (Ok(a), Ok(b)) => zeilen_gegenueberstellen(&a, &b),
            _ => println!("      (keine Textdatei, {} Bytes hier)", lokal.len()),
        }
    }

    // Was nur hier liegt, ist oft der eigentliche Grund für den Konflikt.
    let mut hiesige: Vec<(String, Vec<u8>)> = Vec::new();
    if target.exists() {
        sammle(target, target, &mut hiesige)?;
    }

    for (pfad, _) in hiesige {
        if !bekannt.contains(&pfad) {
            println!("  nur hier: {pfad}");
            abweichend += 1;
        }
    }

    if abweichend == 0 {
        println!("  Kein Unterschied in den Dateien.");
    }

    println!();

    Ok(())
}

fn zeilen_gegenueberstellen(hier: &str, dort: &str) {
    let a: Vec<&str> = hier.lines().collect();
    let b: Vec<&str> = dort.lines().collect();

    let mut gezeigt = 0usize;

    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or("");
        let y = b.get(i).copied().unwrap_or("");

        if x == y {
            continue;
        }

        if gezeigt == 0 {
            println!("      abweichende Zeilen  (- hier  /  + Instanz):");
        }

        println!("      {:>5} - {}", i + 1, kuerzen(x));
        println!("      {:>5} + {}", i + 1, kuerzen(y));

        gezeigt += 1;

        if gezeigt >= 8 {
            println!("      … weitere nicht gezeigt");
            break;
        }
    }

    // Wurde oben etwas eingefügt, verschiebt sich alles danach und erscheint
    // als abweichend. Die Zeilenzahlen machen das erkennbar, statt einen
    // falschen Eindruck zu hinterlassen.
    println!("      Zeilen gesamt: hier {}, Instanz {}", a.len(), b.len());
}

fn kuerzen(zeile: &str) -> String {
    let z = zeile.trim_end();

    if z.chars().count() <= 96 {
        return z.to_string();
    }

    format!("{}…", z.chars().take(96).collect::<String>())
}

/// Merkt sich für einen Ort, wie der Eintrag dort jetzt steht.
fn stand_merken(state: &mut SyncState, item: &ManifestItem, ort: &str, wert: &serde_json::Value) {
    let eintrag = state
        .items
        .entry(format!("mcp/{}", item.slug))
        .or_insert_with(|| SyncedItem {
            remote_checksum: item.checksum.clone(),
            local_hash: String::new(),
            orte: BTreeMap::new(),
        });

    eintrag.remote_checksum = item.checksum.clone();
    eintrag.orte.insert(ort.to_string(), mcp_eintraege::eintrag_hash(wert));
}

/// Legt eine Codex-Angabe, die gleich ersetzt wird, in die gesicherten Fassungen.
fn mcp_sicherung_ablegen(slug: &str, wert: &serde_json::Value) -> Result<(), String> {
    let ziel = backup_dir()?.join("mcp").join(format!("{slug}-codex.json"));

    if let Some(eltern) = ziel.parent() {
        fs::create_dir_all(eltern)
            .map_err(|e| format!("{} lässt sich nicht anlegen: {e}", eltern.display()))?;
    }

    let json = serde_json::to_string_pretty(wert).unwrap_or_default();
    fs::write(&ziel, json).map_err(|e| format!("{} lässt sich nicht schreiben: {e}", ziel.display()))?;
    println!("  Bisherige Codex-Angabe gesichert: {}", ziel.display());

    Ok(())
}

/// Was mit einem strittigen MCP-Server geschehen soll — einmal je Server.
fn entscheiden_mcp(plan: &McpPlan, strittig: &[&McpOrt]) -> Result<bool, String> {
    println!();
    println!(
        "mcp {} — steht an {} Ort(en) anders, als die Instanz ihn vorsieht",
        plan.item.slug,
        strittig.len()
    );

    loop {
        match crate::frage::auswahl(
            "Was damit tun?",
            &[
                ('b', "behalten — die hiesigen Eintraege bleiben"),
                ('u', "ueberschreiben — die Fassung der Instanz gilt (vorher gesichert)"),
                ('d', "Unterschied ansehen"),
            ],
            'b',
        )? {
            'b' => return Ok(false),
            'u' => return Ok(true),
            _ => {
                for ort in strittig {
                    println!();
                    println!("  {}", ort.ort);
                    println!(
                        "    hier    : {}",
                        ort.vorhanden
                            .as_ref()
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "—".to_string())
                    );
                    println!("    Instanz : {}", ort.gewuenscht);
                }
                println!();
            }
        }
    }
}

fn report_mcp(plan: &[McpPlan], nicht_gewaehlt: &[ManifestItem]) {
    let zeilen: Vec<(String, &str)> = plan
        .iter()
        .flat_map(|p| {
            p.orte
                .iter()
                .filter(|o| o.lage != Lage::Unveraendert)
                .map(move |o| {
                    let lage = match o.lage {
                        Lage::Neu => "neu",
                        Lage::Aktualisiert => "aktualisiert",
                        Lage::LokalGeaendert => "hier geändert — bleibt",
                        Lage::Konflikt => "Konflikt — ohne --force nicht angefasst",
                        Lage::Fremd => "fremder Eintrag gleichen Namens — ohne --force nicht angefasst",
                        Lage::Uebernommen => "stand schon richtig da",
                        Lage::Unveraendert => "unverändert",
                    };
                    (format!("{} in {}", p.item.slug, o.ort), lage)
                })
        })
        .collect();

    if !zeilen.is_empty() {
        println!();
        println!("MCP-Server ({})", zeilen.len());

        for (wo, lage) in &zeilen {
            println!("  - {wo}: {lage}");
        }
    }

    let unveraendert: usize = plan
        .iter()
        .map(|p| p.orte.iter().filter(|o| o.lage == Lage::Unveraendert).count())
        .sum();

    if unveraendert > 0 {
        println!();
        println!("MCP-Server unverändert: {unveraendert} Eintrag/Einträge");
    }

    if !nicht_gewaehlt.is_empty() {
        println!();
        println!("MCP-Server nicht einzeln gewählt ({})", nicht_gewaehlt.len());
        println!("  werden nicht eingetragen — MCP-Server nur über `select`, nie über „alles“");

        for item in nicht_gewaehlt.iter().take(20) {
            println!("  - {}", item.name);
        }
    }
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
        "Hooks sind Befehle in der settings.json — die richtet die CLI nicht selbst ein",
    );

    let unveraendert = plan.iter().filter(|(_, v, _)| *v == Verdict::Unveraendert).count();

    if unveraendert > 0 {
        println!();
        println!("Unverändert: {unveraendert}");
    }
}

/// Holt einen Eintrag über MCP.
///
/// **Nur noch dieser Weg** — siehe {@link fetch_manifest}. Der Rückfall auf
/// `GET /api/v1/repository/items/{kind}/{slug}` ist am 14.10.2026 entfallen.
fn fetch_item_ueber(
    connection: &Connection,
    token: &str,
    kind: &str,
    slug: &str,
) -> Result<ItemDetail, String> {
    let wert = fetch_bundle_roh(connection, token, kind, slug)?;

    serde_json::from_value(wert).map_err(|e| format!("{kind}/{slug} ist unlesbar: {e}"))
}

/// Das Bundle eines Eintrags, ungedeutet — MCP-Server lesen es anders als
/// Dateibestände.
fn fetch_bundle_roh(
    connection: &Connection,
    token: &str,
    kind: &str,
    slug: &str,
) -> Result<serde_json::Value, String> {
    let wert = crate::mcp::call_tool(
        connection,
        token,
        "get-repository-bundle-tool",
        serde_json::json!({ "type": kind, "slug": slug }),
    )?;

    // Ein fehlender Eintrag kommt als Befund, nicht als Ausnahme — der Lauf
    // soll daran nicht abbrechen, sondern ihn überspringen.
    if let Some(fehler) = wert.get("error").and_then(|e| e.as_str()) {
        return Err(format!("{kind}/{slug}: {fehler}"));
    }

    Ok(wert)
}

/// Holt das Manifest über MCP.
///
/// **Es gibt nur noch diesen Weg.** Bis zum 14.10.2026 stand hier ein Rückfall
/// auf `GET /api/v1/repository/manifest`, für Instanzen, die den MCP-Abgleich
/// noch nicht kannten (#3857). Der Rückfall hat zuletzt Schaden angerichtet:
/// Während die Octane-Worker der Instanz mit gemischten Fassungen liefen,
/// scheiterte der MCP-Aufruf zeitweise, der Abgleich wich still auf REST aus —
/// und die Zugriffszahlen auf der alten Route sahen danach aus, als brauche sie
/// noch jemand. Ein Rückfall, der eine Stoerung ueberdeckt, verlaengert sie.
///
/// Scheitert MCP, soll der Lauf scheitern und es sagen.
fn fetch_manifest(connection: &Connection, token: &str) -> Result<Manifest, String> {
    let wert = crate::mcp::call_tool(
        connection,
        token,
        "get-repository-manifest-tool",
        serde_json::json!({}),
    )?;

    serde_json::from_value(wert).map_err(|e| format!("Manifest über MCP ist unlesbar: {e}"))
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
    // Claude Code legt sein `.claude` auch unter Windows in den Heimatordner —
    // nicht nach %APPDATA%. Deshalb hier `heimat()` und nicht `config_dir()`.
    Ok(crate::config::heimat()?.join(".claude"))
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

// ---------------------------------------------------------------- geben ----

/// Einen lokal gebauten Eintrag ins Repository geben.
///
/// Bewusst ein eigener Befehl und kein Nebeneffekt von `sync`: Ein Skill, den
/// jemand fuer sich gebaut hat — mit Notizen, Pfaden oder Zugangswegen darin —
/// landet sonst ungefragt in einer Ablage, die alle lesen koennen.
pub fn publish(args: &[String]) -> Result<(), String> {
    let mut name: Option<String> = None;
    let mut grund: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--grund" => {
                i += 1;
                grund = Some(
                    args.get(i)
                        .ok_or_else(|| "--grund braucht einen Text.".to_string())?
                        .clone(),
                );
            }
            other if other.starts_with("--") => return Err(format!("Unbekannte Option: {other}")),
            other => name = Some(other.to_string()),
        }
        i += 1;
    }

    let name = name.ok_or_else(|| {
        "Welcher Eintrag? Beispiel:\n  ai-brain-harness publish mein-skill".to_string()
    })?;

    let connection = config::load()?;
    let claude = claude_dir()?;

    // Suchen statt raten lassen: der Nutzer tippt den Namen, nicht den Typ.
    let (kind, verzeichnis, haupt) = if claude.join("skills").join(&name).exists() {
        ("skill", claude.join("skills").join(&name), "SKILL.md")
    } else if claude.join("agents").join(&name).exists() {
        ("agent", claude.join("agents").join(&name), "AGENTS.md")
    } else {
        return Err(format!(
            "„{name}“ liegt weder unter ~/.claude/skills/ noch unter ~/.claude/agents/."
        ));
    };

    let hauptdatei = verzeichnis.join(haupt);

    let inhalt = fs::read_to_string(&hauptdatei)
        .map_err(|e| format!("{} nicht lesbar: {e}", hauptdatei.display()))?;

    let mut dateien: Vec<(String, Vec<u8>)> = Vec::new();
    sammle(&verzeichnis, &verzeichnis, &mut dateien)?;
    dateien.sort_by(|a, b| a.0.cmp(&b.0));

    let mut anhang = Vec::new();
    let mut uebersprungen = Vec::new();

    for (pfad, roh) in &dateien {
        if pfad == haupt {
            continue;
        }

        // Binaerdateien laesst dieser Weg bewusst aus: der Endpunkt nimmt Text
        // entgegen, und stillschweigend etwas Kaputtes hochzuladen waere
        // schlimmer, als es zu melden.
        match String::from_utf8(roh.clone()) {
            Ok(text) => anhang.push(serde_json::json!({ "path": pfad, "content": text })),
            Err(_) => uebersprungen.push(pfad.clone()),
        }
    }

    let token = crate::mcp::access_token(&connection)?;

    // Seit 15.10.2026 ueber MCP (#7762). Hier stand der letzte REST-Aufruf des
    // Komm-Layers; er blieb, weil `publish-repository-item-tool` weniger kann
    // als dieser Weg braucht. Jetzt gibt es `sync-repository-item-tool`: Upsert
    // nach (type, slug), Anhaenge werden ersetzt, Begruendung landet in der
    // Versionshistorie.
    let antwort = crate::mcp::call_tool(
        &connection,
        &token,
        "sync-repository-item-tool",
        serde_json::json!({
            "type": kind,
            "slug": name,
            "name": name,
            "content": inhalt,
            "files": anhang,
            "change_reason": grund.unwrap_or_else(|| format!("Vom Rechner {} zurueckgegeben", connection.device_name)),
        }),
    )?;

    let status = antwort
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("uebertragen");

    println!("{name} ({kind}) — {status}.");
    println!("  {} Datei(en) uebertragen", anhang.len() + 1);

    if !uebersprungen.is_empty() {
        println!();
        println!("Nicht uebertragen, weil keine Textdateien:");

        for pfad in &uebersprungen {
            println!("  - {pfad}");
        }
    }

    println!();
    println!("Im Repository sichtbar unter {}/command-center/repository", connection.base_url);

    Ok(())
}
