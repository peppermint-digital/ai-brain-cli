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
use std::collections::{BTreeMap, BTreeSet};
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
                    // Laut Stand haben sich beide Seiten bewegt — ein Konflikt
                    // ist das aber erst, wenn der Inhalt wirklich auseinander
                    // laeuft. Wurde der Eintrag auf einem anderen Weg
                    // geschrieben (serverseitig ueber das Install-Werkzeug, von
                    // Hand, durch ein zweites Werkzeug), ist nur die
                    // Buchfuehrung des Companions veraltet; die Dateien sind
                    // laengst dieselben.
                    //
                    // Also erst nachsehen, dann urteilen — sonst meldet jeder
                    // Lauf denselben Konflikt, der keiner ist, und die echten
                    // gehen darin unter. Der Abruf kostet eine Anfrage, faellt
                    // aber nur im seltenen Konfliktfall an.
                    (false, false) => match fetch_item(&connection, &token, &item.kind, &item.slug) {
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
                },
            );
        }

        save_state(&state)?;
    }

    // Was der Companion einmal geschrieben hat, jetzt aber nicht mehr im
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
        if im_manifest.contains(key) {
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

    report(&plan);

    // Ein ausdrückliches --apply beantwortet die Frage schon; gefragt wird nur,
    // wenn jemand da ist, der antworten kann.
    if !apply && crate::frage::darf_fragen(zugestimmt) {
        let etwas_zu_tun = plan.iter().any(|(_, v, _)| {
            matches!(v, Verdict::Neu | Verdict::Aktualisiert | Verdict::Konflikt | Verdict::Fremd)
        }) || !ueberzaehlig.is_empty();

        if etwas_zu_tun {
            println!();
            apply = crate::frage::ja_nein("Anwenden?", false)?;
        }
    }

    if !apply {
        println!();
        println!("Nichts geschrieben. Zum Anwenden:  ai-brain-companion sync --apply");
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
            },
        );

        geschrieben += 1;
    }

    // Entfernen ist die einzige Handlung, die etwas wegnimmt — dafür wird
    // ausdrücklich gefragt, und die Vorgabe ist Nein.
    if !aufraeumen && !ueberzaehlig.is_empty() && crate::frage::darf_fragen(zugestimmt) {
        println!();
        aufraeumen = crate::frage::ja_nein(
            &format!(
                "{} nicht mehr ausgewaehlte Eintraege entfernen?",
                ueberzaehlig.len()
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
        _ => "liegt hier, stammt aber nicht vom Companion",
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
        "Welcher Eintrag? Beispiel:\n  ai-brain-companion publish mein-skill".to_string()
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
    let url = format!("{}/api/v1/repository/items", connection.base_url);

    let antwort: serde_json::Value = ureq::post(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/json")
        .send_json(serde_json::json!({
            "type": kind,
            "slug": name,
            "name": name,
            "content": inhalt,
            "files": anhang,
            "change_reason": grund.unwrap_or_else(|| format!("Vom Rechner {} zurueckgegeben", connection.device_name)),
        }))
        .map_err(|e| match e {
            ureq::Error::Status(code, resp) => {
                let body = resp.into_string().unwrap_or_default();
                format!("Abgelehnt (HTTP {code}): {}", body.trim())
            }
            other => format!("Nicht uebertragbar: {other}"),
        })?
        .into_json()
        .map_err(|e| format!("Antwort unlesbar: {e}"))?;

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
