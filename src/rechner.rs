//! Dieser Rechner als Werkzeug (#7805).
//!
//! **Warum das ein eigener MCP-Server ist.** Die Werkzeuge von AI Brain kommen
//! aus AI Brain — die kennen Projekte, Aufgaben und Server. Was sie NICHT
//! koennen, ist diesen Rechner anfassen: Dienste, Platten, Bildschirm,
//! Tastatur. Genau das war Hermes' Rolle hier, und genau die uebernimmt der
//! Harness. Also liefert der Harness seine eigenen Werkzeuge, auf demselben
//! Weg, auf dem er die von AI Brain durchreicht.
//!
//! **Zwei Server, ein Motor.** Die Maske haengt beide an: `ai-brain` (der
//! Durchgriff auf die Instanz) und `rechner` (dieser hier). Damit hat ein
//! Gespraech beides in derselben Hand — die Daten der Instanz und die Hoheit
//! ueber die Maschine.
//!
//! ## Die Rechte stehen in der Umgebung, nicht im Aufruf
//!
//! Ein Werkzeug, das sich seine Rechte selbst mitgeben laesst, hat keine. Die
//! Maske setzt `AI_BRAIN_RECHNER_RECHTE`, bevor sie den Motor startet; dieser
//! Server liest es beim Start **einmal**. Ein Modell kann die Variable im
//! laufenden Prozess nicht aendern, also auch seine Rechte nicht erweitern.

use std::io::{BufRead, Write};
use std::process::Command;

/// Was ein Gespraech auf diesem Rechner darf.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hoheit {
    /// Nur nachsehen: Zustaende lesen, nichts aendern.
    Lesend,
    /// Auch anfassen: Dienste steuern, Befehle fahren, tippen.
    Steuernd,
}

impl Hoheit {
    pub fn aus_umgebung() -> Self {
        match std::env::var("AI_BRAIN_RECHNER_RECHTE").as_deref() {
            Ok("steuernd") => Hoheit::Steuernd,
            _ => Hoheit::Lesend,
        }
    }
}

/// Ein Werkzeug dieses Servers.
struct Werkzeug {
    name: &'static str,
    beschreibung: &'static str,
    /// Braucht es `Hoheit::Steuernd`?
    aendernd: bool,
    schema: fn() -> serde_json::Value,
}

fn ohne_argumente() -> serde_json::Value {
    serde_json::json!({ "type": "object", "properties": {} })
}

fn schema_dienst_zustand() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "dienste": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Unit-Namen, z. B. [\"ai-brain.service\", \"reverb.service\"]. Ohne Angabe: alle fehlgeschlagenen Units."
            }
        }
    })
}

fn schema_dienst_steuern() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "dienst": { "type": "string", "description": "Unit-Name, z. B. \"reverb.service\"." },
            "aktion": { "type": "string", "enum": ["start", "stop", "restart", "reload"] }
        },
        "required": ["dienst", "aktion"]
    })
}

fn schema_befehl() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "befehl": { "type": "string", "description": "Shell-Befehl. Laeuft als der angemeldete Nutzer." },
            "verzeichnis": { "type": "string", "description": "Arbeitsverzeichnis. Standard: das Heimatverzeichnis." },
            "frist": { "type": "integer", "description": "Abbruch nach so vielen Sekunden. Standard 60, hoechstens 600." }
        },
        "required": ["befehl"]
    })
}

fn schema_bildschirmfoto() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "ausgabe": { "type": "string", "description": "Zieldatei. Standard: eine neue Datei unter /tmp." }
        }
    })
}

fn schema_tippen() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "text": { "type": "string", "description": "Was getippt wird — Zeichen fuer Zeichen in das Fenster mit dem Fokus." }
        },
        "required": ["text"]
    })
}

fn schema_taste() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "taste": { "type": "string", "description": "Eine Taste oder Kombination in wtype-Schreibweise, z. B. \"Return\", \"ctrl+c\"." }
        },
        "required": ["taste"]
    })
}

const WERKZEUGE: &[Werkzeug] = &[
    Werkzeug {
        name: "rechner-dienste",
        beschreibung: "Zustand von systemd-Units auf diesem Rechner. Ohne Argument alle fehlgeschlagenen \
                       Units — das ist der schnellste Blick auf \"laeuft hier was nicht\".",
        aendernd: false,
        schema: schema_dienst_zustand,
    },
    Werkzeug {
        name: "rechner-dienst-steuern",
        beschreibung: "Startet, stoppt oder startet eine systemd-Unit neu. Braucht steuernde Rechte.",
        aendernd: true,
        schema: schema_dienst_steuern,
    },
    Werkzeug {
        name: "rechner-datentraeger",
        beschreibung: "Belegung der Dateisysteme (df) und der Zustand gemounteter Netzlaufwerke. \
                       Volle Platten sind die haeufigste Ursache stiller Ausfaelle.",
        aendernd: false,
        schema: ohne_argumente,
    },
    Werkzeug {
        name: "rechner-last",
        beschreibung: "Laufzeit, Last, Speicher und die groessten Verbraucher. Dazu der Zustand der \
                       Grafikkarte, soweit vorhanden — hier teilen sich mehrere Dienste den VRAM.",
        aendernd: false,
        schema: ohne_argumente,
    },
    Werkzeug {
        name: "rechner-befehl",
        beschreibung: "Fuehrt einen Shell-Befehl auf diesem Rechner aus, als der angemeldete Nutzer. \
                       Braucht steuernde Rechte. Fuer alles, was kein eigenes Werkzeug hat.",
        aendernd: true,
        schema: schema_befehl,
    },
    Werkzeug {
        name: "rechner-bildschirmfoto",
        beschreibung: "Nimmt den Bildschirm auf (grim) und liefert den Pfad der Datei. Braucht eine \
                       laufende grafische Sitzung — aus einer reinen Textsitzung heraus gibt es keinen Bildschirm.",
        aendernd: false,
        schema: schema_bildschirmfoto,
    },
    Werkzeug {
        name: "rechner-fenster",
        beschreibung: "Die offenen Fenster der grafischen Sitzung (hyprctl): Titel, Anwendung, Arbeitsflaeche, Fokus.",
        aendernd: false,
        schema: ohne_argumente,
    },
    Werkzeug {
        name: "rechner-tippen",
        beschreibung: "Tippt Text in das Fenster mit dem Fokus (wtype). Braucht steuernde Rechte und eine \
                       grafische Sitzung.",
        aendernd: true,
        schema: schema_tippen,
    },
    Werkzeug {
        name: "rechner-taste",
        beschreibung: "Drueckt eine Taste oder Kombination (wtype). Braucht steuernde Rechte und eine \
                       grafische Sitzung.",
        aendernd: true,
        schema: schema_taste,
    },
];

/// `ai-brain-cli rechner-mcp` — der Werkzeugserver fuer diesen Rechner.
pub fn serve() -> Result<(), String> {
    let hoheit = Hoheit::aus_umgebung();

    eprintln!(
        "ai-brain-cli: Rechner-Werkzeuge bereit ({})",
        match hoheit {
            Hoheit::Lesend => "nur lesend",
            Hoheit::Steuernd => "steuernd",
        }
    );

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for zeile in stdin.lock().lines() {
        let zeile = zeile.map_err(|e| format!("stdin nicht lesbar: {e}"))?;
        let roh = zeile.trim();

        if roh.is_empty() {
            continue;
        }

        let Ok(nachricht) = serde_json::from_str::<serde_json::Value>(roh) else {
            eprintln!("ai-brain-cli: ungueltiges JSON uebersprungen");
            continue;
        };

        // Ohne `id` ist es eine Benachrichtigung — darauf antwortet man nicht.
        let Some(id) = nachricht.get("id").cloned() else {
            continue;
        };

        let methode = nachricht.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let parameter = nachricht.get("params").cloned().unwrap_or(serde_json::json!({}));

        let antwort = match methode {
            "initialize" => erfolg(&id, serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "rechner", "version": env!("CARGO_PKG_VERSION") },
            })),
            "tools/list" => erfolg(&id, serde_json::json!({ "tools": liste(hoheit) })),
            "tools/call" => rufen(&id, &parameter, hoheit),
            "ping" => erfolg(&id, serde_json::json!({})),
            sonst => fehler(&id, -32601, &format!("Methode '{sonst}' kennt dieser Server nicht")),
        };

        writeln!(stdout, "{antwort}").map_err(|e| format!("stdout nicht beschreibbar: {e}"))?;
        stdout.flush().map_err(|e| format!("stdout nicht leerbar: {e}"))?;
    }

    Ok(())
}

/// Die Werkzeugliste — aendernde fehlen, wenn die Hoheit nur lesend ist.
///
/// **Bewusst weglassen statt im Aufruf ablehnen.** Ein Werkzeug, das in der
/// Liste steht und dann „darfst du nicht" sagt, laedt zum Probieren ein und
/// kostet jedes Mal einen Zug. Was nicht erlaubt ist, existiert nicht.
fn liste(hoheit: Hoheit) -> Vec<serde_json::Value> {
    WERKZEUGE
        .iter()
        .filter(|w| !w.aendernd || hoheit == Hoheit::Steuernd)
        .map(|w| {
            serde_json::json!({
                "name": w.name,
                "description": w.beschreibung,
                "inputSchema": (w.schema)(),
                "annotations": { "readOnlyHint": !w.aendernd },
            })
        })
        .collect()
}

fn rufen(id: &serde_json::Value, parameter: &serde_json::Value, hoheit: Hoheit) -> String {
    let name = parameter.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let args = parameter.get("arguments").cloned().unwrap_or(serde_json::json!({}));

    let Some(werkzeug) = WERKZEUGE.iter().find(|w| w.name == name) else {
        return fehler(id, -32602, &format!("Kein Werkzeug namens '{name}'"));
    };

    if werkzeug.aendernd && hoheit != Hoheit::Steuernd {
        return inhalt(
            id,
            true,
            &format!(
                "'{name}' aendert etwas und das Gespraech laeuft nur lesend. \
                 Mit steuernden Rechten starten:  brain --steuernd"
            ),
        );
    }

    match ausfuehren(name, &args) {
        Ok(text) => inhalt(id, false, &text),
        Err(text) => inhalt(id, true, &text),
    }
}

/// Ein Werkzeug direkt rufen, ohne MCP und ohne Motor.
///
/// **Wofuer.** Die Befehle der Maske (`/dienste`, `/rechner`) sind Nachfragen,
/// keine Denkaufgaben. Sie durch ein Modell zu schicken kostet Geld und Zeit
/// fuer eine Antwort, die ohnehin feststeht. Dieselben Funktionen, derselbe
/// Riegel — nur ohne den Umweg.
pub fn fuer_maske(name: &str, args: &serde_json::Value) -> Result<String, String> {
    let hoheit = Hoheit::aus_umgebung();

    let Some(werkzeug) = WERKZEUGE.iter().find(|w| w.name == name) else {
        return Err(format!("Kein Werkzeug namens '{name}'"));
    };

    if werkzeug.aendernd && hoheit != Hoheit::Steuernd {
        return Err(format!("'{name}' aendert etwas und dieser Lauf ist nur lesend."));
    }

    ausfuehren(name, args)
}

// ─── Die Werkzeuge selbst ────────────────────────────────────────────────────

/// Name auf Funktion — einmal, damit MCP-Weg und Maske-Weg nicht auseinanderlaufen.
fn ausfuehren(name: &str, args: &serde_json::Value) -> Result<String, String> {
    match name {
        "rechner-dienste" => dienste(args),
        "rechner-dienst-steuern" => dienst_steuern(args),
        "rechner-datentraeger" => datentraeger(),
        "rechner-last" => last(),
        "rechner-befehl" => befehl_fahren(args),
        "rechner-bildschirmfoto" => bildschirmfoto(args),
        "rechner-fenster" => fenster(),
        "rechner-tippen" => tippen(args),
        "rechner-taste" => taste(args),
        sonst => Err(format!("'{sonst}' ist aufgelistet, aber nicht zugeordnet.")),
    }
}

fn dienste(args: &serde_json::Value) -> Result<String, String> {
    let namen: Vec<String> = args
        .get("dienste")
        .and_then(|d| d.as_array())
        .map(|a| a.iter().filter_map(|w| w.as_str().map(str::to_string)).collect())
        .unwrap_or_default();

    if namen.is_empty() {
        let aus = fahren("systemctl", &["--failed", "--no-legend", "--no-pager"], None, 20)?;

        return Ok(if aus.trim().is_empty() {
            "Keine fehlgeschlagene Unit.".to_string()
        } else {
            format!("Fehlgeschlagene Units:\n{aus}")
        });
    }

    let mut zeilen = Vec::new();

    for name in &namen {
        zeilen.push(zustand_einer_unit(name));
    }

    Ok(zeilen.join("\n"))
}

/// Zustand EINER Unit, in einer Zeile.
///
/// **Bewusst `show` statt `is-active`.** `is-active` meldet den Zustand ueber
/// seinen Rueckgabewert: 3 heisst inaktiv, 4 heisst „Unit kenne ich nicht".
/// Beides ist eine Antwort, kein Fehler — wer das durchreicht, schreibt
/// „'systemctl' endete mit 4" hin, und das sieht nach einem kaputten Werkzeug
/// aus statt nach einem Dienst, den es nicht gibt. `show` endet immer mit 0 und
/// sagt es im Text.
fn zustand_einer_unit(name: &str) -> String {
    let roh = fahren(
        "systemctl",
        &["show", "-p", "LoadState", "-p", "ActiveState", "-p", "SubState",
          "-p", "ActiveEnterTimestamp", "--value", name],
        None,
        10,
    );

    let Ok(roh) = roh else {
        return format!("{name}: nicht abfragbar");
    };

    let werte: Vec<&str> = roh.lines().map(str::trim).collect();
    let feld = |i: usize| werte.get(i).copied().unwrap_or("");

    let geladen = feld(0);
    let zustand = feld(1);
    let unter = feld(2);
    let seit = feld(3);

    if geladen == "not-found" {
        return format!("{name}: gibt es auf diesem Rechner nicht");
    }

    if geladen == "masked" {
        return format!("{name}: stillgelegt (masked)");
    }

    let mut zeile = format!("{name}: {zustand}");

    // `failed (exit-code)` sagt mehr als `failed` — der Untergrund ist genau
    // das, was man als naechstes wissen will.
    if !unter.is_empty() && unter != zustand {
        zeile.push_str(&format!(" ({unter})"));
    }

    if !seit.is_empty() && seit != "n/a" {
        zeile.push_str(&format!("  seit {seit}"));
    }

    zeile
}

fn dienst_steuern(args: &serde_json::Value) -> Result<String, String> {
    let dienst = pflicht(args, "dienst")?;
    let aktion = pflicht(args, "aktion")?;

    if !["start", "stop", "restart", "reload"].contains(&aktion.as_str()) {
        return Err(format!("Unbekannte Aktion '{aktion}'. Erlaubt: start, stop, restart, reload."));
    }

    // Vor dem Anfassen den Zustand merken — sonst steht hinterher nur „ok" da
    // und niemand weiss, was sich geaendert hat.
    let vorher = fahren("systemctl", &["is-active", &dienst], None, 10)
        .unwrap_or_else(|e| e)
        .trim()
        .to_string();

    let aus = fahren("systemctl", &[&aktion, &dienst], None, 120)?;

    let nachher = fahren("systemctl", &["is-active", &dienst], None, 15)
        .unwrap_or_else(|e| e)
        .trim()
        .to_string();

    Ok(format!(
        "{dienst}: {aktion} ausgefuehrt.\nVorher: {vorher}\nNachher: {nachher}{}",
        if aus.trim().is_empty() {
            String::new()
        } else {
            format!("\n{aus}")
        }
    ))
}

fn datentraeger() -> Result<String, String> {
    let platten = fahren("df", &["-h", "-x", "tmpfs", "-x", "devtmpfs"], None, 20)?;

    // Netzlaufwerke haengen hier oefter, als man denkt — ein haengender Mount
    // sieht in `df` normal aus, bis man ihn anfasst.
    let mounts = fahren(
        "findmnt",
        &["-t", "cifs,nfs,nfs4,fuse.sshfs", "-o", "TARGET,SOURCE,FSTYPE", "-n"],
        None,
        15,
    )
    .unwrap_or_default();

    Ok(format!(
        "{platten}\nNetzlaufwerke:\n{}",
        if mounts.trim().is_empty() {
            "  keine".to_string()
        } else {
            mounts
        }
    ))
}

fn last() -> Result<String, String> {
    let mut teile = vec![
        format!("Laufzeit: {}", fahren("uptime", &["-p"], None, 10).unwrap_or_default().trim()),
        format!("Last: {}", fahren("cat", &["/proc/loadavg"], None, 10).unwrap_or_default().trim()),
        fahren("free", &["-h"], None, 10).unwrap_or_default(),
    ];

    if let Ok(gpu) = fahren(
        "nvidia-smi",
        &["--query-gpu=name,memory.used,memory.total,utilization.gpu", "--format=csv,noheader"],
        None,
        20,
    ) {
        teile.push(format!("Grafikkarte: {}", gpu.trim()));
    }

    Ok(teile.join("\n"))
}

fn befehl_fahren(args: &serde_json::Value) -> Result<String, String> {
    let befehl = pflicht(args, "befehl")?;

    let frist = args
        .get("frist")
        .and_then(|f| f.as_u64())
        .unwrap_or(60)
        .clamp(1, 600);

    let verzeichnis = args
        .get("verzeichnis")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| std::env::var("HOME").ok());

    fahren("bash", &["-lc", &befehl], verzeichnis.as_deref(), frist)
}

fn bildschirmfoto(args: &serde_json::Value) -> Result<String, String> {
    grafisch()?;

    let ziel = args
        .get("ausgabe")
        .and_then(|a| a.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| {
            // Keine Uhr und kein Zufall hier — die laufende Nummer des
            // Prozesses genuegt, um zwei Aufnahmen zu unterscheiden.
            format!("/tmp/harness-bildschirm-{}.png", std::process::id())
        });

    fahren("grim", &[&ziel], None, 30)?;

    let groesse = std::fs::metadata(&ziel).map(|m| m.len()).unwrap_or(0);

    if groesse == 0 {
        return Err(format!(
            "grim hat {ziel} angelegt, aber die Datei ist leer — meist bedeutet das, \
             dass die Aufnahme keiner grafischen Sitzung zugeordnet werden konnte."
        ));
    }

    Ok(format!("Bildschirmfoto: {ziel} ({groesse} Bytes)"))
}

fn fenster() -> Result<String, String> {
    grafisch()?;

    let roh = fahren("hyprctl", &["-j", "clients"], None, 15)?;

    let Ok(liste) = serde_json::from_str::<Vec<serde_json::Value>>(&roh) else {
        return Ok(roh);
    };

    let mut zeilen = Vec::new();

    for f in &liste {
        let titel = f.get("title").and_then(|t| t.as_str()).unwrap_or("");
        let klasse = f.get("class").and_then(|k| k.as_str()).unwrap_or("");
        let flaeche = f
            .get("workspace")
            .and_then(|w| w.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("");

        zeilen.push(format!("[{flaeche}] {klasse}: {titel}"));
    }

    Ok(if zeilen.is_empty() {
        "Kein offenes Fenster.".to_string()
    } else {
        zeilen.join("\n")
    })
}

fn tippen(args: &serde_json::Value) -> Result<String, String> {
    grafisch()?;

    let text = pflicht(args, "text")?;

    fahren("wtype", &[&text], None, 60)?;

    Ok(format!("{} Zeichen getippt.", text.chars().count()))
}

fn taste(args: &serde_json::Value) -> Result<String, String> {
    grafisch()?;

    let taste = pflicht(args, "taste")?;

    // wtype trennt Modifikatoren mit `-M`; eine Kombination wie „ctrl+c"
    // zerfaellt hier in ihre Teile.
    let mut argumente: Vec<String> = Vec::new();
    let teile: Vec<&str> = taste.split('+').map(str::trim).collect();

    if let Some((letzte, modifikatoren)) = teile.split_last() {
        for m in modifikatoren {
            argumente.push("-M".into());
            argumente.push((*m).to_string());
        }

        argumente.push("-k".into());
        argumente.push((*letzte).to_string());

        for m in modifikatoren.iter().rev() {
            argumente.push("-m".into());
            argumente.push((*m).to_string());
        }
    }

    let verweise: Vec<&str> = argumente.iter().map(String::as_str).collect();

    fahren("wtype", &verweise, None, 30)?;

    Ok(format!("'{taste}' gedrueckt."))
}

// ─── Handwerk ────────────────────────────────────────────────────────────────

/// Gibt es ueberhaupt eine grafische Sitzung, die man anfassen koennte?
///
/// **Die Pruefung gehoert vor den Aufruf, nicht danach.** Ohne sie meldet
/// `grim` „failed to retrieve wayland globals" und `wtype` „Compositor does not
/// support the virtual keyboard protocol" — zwei Meldungen, die nach einem
/// fehlenden Programm klingen, obwohl nur die Sitzung fehlt. Auf diesem Rechner
/// ist das der Normalfall, wenn der Harness aus einer Textsitzung laeuft.
fn grafisch() -> Result<(), String> {
    if std::env::var("WAYLAND_DISPLAY").is_ok() || std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
        return Ok(());
    }

    Err("Keine grafische Sitzung erreichbar: weder WAYLAND_DISPLAY noch HYPRLAND_INSTANCE_SIGNATURE \
         ist gesetzt. Dieser Lauf haengt an einer Textsitzung — Bildschirm, Tastatur und Fenster \
         gehoeren zu einer Wayland-Sitzung, die von hier aus nicht sichtbar ist."
        .to_string())
}

fn pflicht(args: &serde_json::Value, name: &str) -> Result<String, String> {
    args.get(name)
        .and_then(|w| w.as_str())
        .map(str::to_string)
        .filter(|w| !w.trim().is_empty())
        .ok_or_else(|| format!("'{name}' fehlt."))
}

/// Ein Programm fahren und seine Ausgabe einsammeln.
///
/// Die Frist laeuft ueber `timeout`, nicht ueber einen eigenen Zeitgeber: ein
/// Kindprozess, der nicht endet, haengt sonst den ganzen Werkzeugserver — und
/// damit das Gespraech.
fn fahren(programm: &str, args: &[&str], verzeichnis: Option<&str>, frist: u64) -> Result<String, String> {
    let mut befehl = Command::new("timeout");
    befehl.arg(format!("{frist}s"));
    befehl.arg(programm);
    befehl.args(args);

    if let Some(dir) = verzeichnis {
        // Erst nachsehen, dann setzen. Ein fehlendes Arbeitsverzeichnis laesst
        // `spawn` mit „No such file or directory" scheitern — und die Meldung
        // zeigt dann auf das Programm, das in Wahrheit nie gesucht wurde.
        if !std::path::Path::new(dir).is_dir() {
            return Err(format!("Arbeitsverzeichnis '{dir}' gibt es nicht."));
        }

        befehl.current_dir(dir);
    }

    let ausgabe = befehl
        .output()
        .map_err(|e| format!("'{programm}' liess sich nicht starten: {e}"))?;

    let aus = String::from_utf8_lossy(&ausgabe.stdout).to_string();
    let err = String::from_utf8_lossy(&ausgabe.stderr).to_string();

    if ausgabe.status.success() {
        return Ok(if aus.trim().is_empty() && !err.trim().is_empty() {
            err
        } else {
            aus
        });
    }

    // 124 ist die Marke von `timeout` — die gehoert benannt, sonst sucht man
    // den Fehler im Programm statt in der Frist.
    if ausgabe.status.code() == Some(124) {
        return Err(format!("'{programm}' lief laenger als {frist}s und wurde abgebrochen."));
    }

    Err(format!(
        "'{programm}' endete mit {}{}{}",
        ausgabe
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "Signal".to_string()),
        if err.trim().is_empty() { "" } else { ":\n" },
        err.trim()
    ))
}

// ─── JSON-RPC ────────────────────────────────────────────────────────────────

fn erfolg(id: &serde_json::Value, ergebnis: serde_json::Value) -> String {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": ergebnis }).to_string()
}

fn inhalt(id: &serde_json::Value, fehlerhaft: bool, text: &str) -> String {
    erfolg(
        id,
        serde_json::json!({
            "content": [{ "type": "text", "text": text }],
            "isError": fehlerhaft,
        }),
    )
}

fn fehler(id: &serde_json::Value, code: i32, nachricht: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": nachricht },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lesende_hoheit_verbirgt_die_aendernden_werkzeuge() {
        let lesend = liste(Hoheit::Lesend);
        let steuernd = liste(Hoheit::Steuernd);

        let namen: Vec<&str> = lesend
            .iter()
            .map(|w| w["name"].as_str().unwrap())
            .collect();

        assert!(!namen.contains(&"rechner-befehl"));
        assert!(!namen.contains(&"rechner-tippen"));
        assert!(!namen.contains(&"rechner-dienst-steuern"));
        assert!(namen.contains(&"rechner-dienste"));

        // Die Gegenprobe: steuernd ist wirklich mehr, nicht nur anders.
        assert!(steuernd.len() > lesend.len());
    }

    #[test]
    fn jedes_werkzeug_traegt_den_passenden_lesehinweis() {
        for w in liste(Hoheit::Steuernd) {
            let name = w["name"].as_str().unwrap();
            let nur_lesend = w["annotations"]["readOnlyHint"].as_bool().unwrap();
            let aendernd = WERKZEUGE.iter().find(|x| x.name == name).unwrap().aendernd;

            assert_eq!(nur_lesend, !aendernd, "{name} traegt den falschen Hinweis");
        }
    }

    #[test]
    fn ein_aenderndes_werkzeug_wird_ohne_hoheit_abgewiesen() {
        let antwort = rufen(
            &serde_json::json!(1),
            &serde_json::json!({ "name": "rechner-befehl", "arguments": { "befehl": "echo hallo" } }),
            Hoheit::Lesend,
        );

        let v: serde_json::Value = serde_json::from_str(&antwort).unwrap();

        assert_eq!(v["result"]["isError"], serde_json::json!(true));
        assert!(v["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("nur lesend"));
    }

    #[test]
    fn ein_unbekanntes_werkzeug_wird_benannt() {
        let antwort = rufen(
            &serde_json::json!(7),
            &serde_json::json!({ "name": "rechner-zaubern" }),
            Hoheit::Steuernd,
        );

        assert!(antwort.contains("rechner-zaubern"));
        assert!(antwort.contains("error"));
    }

    #[test]
    fn die_hoheit_kommt_nur_aus_der_umgebung_und_ist_sonst_lesend() {
        // Ohne gesetzte Variable gilt die engere Stufe — ein Tippfehler darf
        // nicht zu mehr Rechten fuehren.
        assert_eq!(Hoheit::aus_umgebung(), Hoheit::Lesend);
    }

    #[test]
    fn eine_frist_ueberschreitung_wird_als_solche_gemeldet() {
        let fehler = fahren("sleep", &["5"], None, 1).unwrap_err();

        assert!(fehler.contains("laenger als 1s"), "{fehler}");
    }

    #[test]
    fn ein_befehl_liefert_seine_ausgabe() {
        // Verzeichnis ausdruecklich mitgeben: `HOME` ist prozessweit, und
        // Nachbartests in diesem Programm setzen es auf erfundene Pfade.
        let aus = befehl_fahren(&serde_json::json!({
            "befehl": "echo hallo-harness",
            "verzeichnis": "/tmp",
        }))
        .unwrap();

        assert!(aus.contains("hallo-harness"), "{aus}");
    }

    #[test]
    fn eine_unbekannte_unit_wird_benannt_statt_als_programmfehler_gemeldet() {
        let zeile = zustand_einer_unit("gibt-es-garantiert-nicht-7805.service");

        assert!(zeile.contains("gibt es auf diesem Rechner nicht"), "{zeile}");
        // Scharf: kein Rueckgabewert, kein Programmname.
        assert!(!zeile.contains("endete mit"), "{zeile}");
        assert!(!zeile.contains("systemctl"), "{zeile}");
    }

    #[test]
    fn eine_laufende_unit_wird_als_aktiv_mit_zeitpunkt_gemeldet() {
        // `init.scope` existiert auf jedem systemd-Rechner und laeuft immer.
        let zeile = zustand_einer_unit("init.scope");

        assert!(zeile.contains("active"), "{zeile}");
        assert!(zeile.contains("seit"), "{zeile}");
    }

    #[test]
    fn ein_fehlendes_arbeitsverzeichnis_wird_benannt_statt_das_programm() {
        let fehler = befehl_fahren(&serde_json::json!({
            "befehl": "echo egal",
            "verzeichnis": "/gibt/es/nicht",
        }))
        .unwrap_err();

        assert!(fehler.contains("Arbeitsverzeichnis"), "{fehler}");
        assert!(fehler.contains("/gibt/es/nicht"), "{fehler}");
    }

    #[test]
    fn ein_fehlendes_pflichtfeld_wird_benannt() {
        assert!(befehl_fahren(&serde_json::json!({})).unwrap_err().contains("befehl"));
    }
}
