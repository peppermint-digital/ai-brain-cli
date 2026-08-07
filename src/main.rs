//! AI Brain Companion
//!
//! Verbindet einen Rechner mit einer AI-Brain-Instanz und stellt deren
//! MCP-Server danach für Claude Code bereit.
//!
//! Zwei Aufgaben, zwei Unterbefehle:
//!
//! * `connect <domain>` — koppelt den Rechner. Liest den Instanz-Steckbrief
//!   unter `/.well-known/ai-brain`, startet den Device-Flow, zeigt einen kurzen
//!   Code an und holt nach der Bestätigung im Browser **eigene**
//!   Gerätezugangsdaten ab.
//! * `mcp` — läuft als MCP-Server über stdio. Claude Code startet diesen
//!   Prozess; er hält die Gerätezugangsdaten, besorgt sich damit Token und
//!   reicht die Aufrufe an die Instanz weiter.
//!
//! Warum der zweite Teil nötig ist: Ein Zugangstoken lebt nur eine Stunde,
//! Claude Codes Konfiguration kann aber nur einen festen Wert halten. Ohne
//! einen Prozess, der Token erneuert, wäre die Verbindung nach einer Stunde
//! tot. Genau deshalb ist der Companion nicht nur ein Einrichtungsprogramm.

mod config;
mod discovery;
mod frage;
mod mcp;
mod pairing;
mod select;
mod sync;

use std::process::ExitCode;

const USAGE: &str = "\
AI Brain Companion

Aufrufe:
  ai-brain-companion connect [domain]   Rechner mit einer AI-Brain-Instanz verbinden
                                        (ohne Angabe: die Instanz, von der dieses
                                        Programm kam)
  ai-brain-companion mcp                Als MCP-Server laufen (startet Claude Code selbst)
  ai-brain-companion select             Waehlen, welche Eintraege dieser Rechner bekommt
  ai-brain-companion sync               Skills und Agents aus dem Repository abgleichen
  ai-brain-companion publish <name>     Einen lokalen Eintrag ins Repository geben
  ai-brain-companion status             Zeigt die aktuelle Verbindung
  ai-brain-companion configure          Traegt den MCP-Server in Claude Code und Codex ein
  ai-brain-companion disconnect         Entfernt die lokale Verbindung

Beispiel:
  ai-brain-companion connect brain.proxy.peppermint-digital.com

Optionen für `sync`:
  --apply               Änderungen wirklich schreiben (ohne: erst fragen)
  --only <name>         Nur diesen einen Eintrag
  --force               Auch überschreiben, wo lokal geändert wurde
                        (die lokale Fassung wird gesichert)
  --aufraeumen          Nicht mehr ausgewählte Einträge entfernen — aber nur die,
                        die der Companion selbst geschrieben hat und die seither
                        unverändert sind
  --ja                  Nicht fragen, sichere Vorgabe nehmen: anwenden ja, aber
                        nichts überschreiben und nichts entfernen. Dafür bleiben
                        --force und --aufraeumen nötig.

  Am Terminal fragt `sync` vor dem Anwenden und bei jedem strittigen Eintrag
  einzeln (behalten / überschreiben / Unterschied ansehen). Ohne Terminal wird
  NIE gefragt — dann gilt, was an Optionen dasteht.

Optionen für `select`:
  --zeigen              Nur anzeigen, nichts fragen und nichts aendern
  --setzen a,b,c        Ohne Rueckfrage genau diese Eintraege setzen
  --alles               Ohne Rueckfrage auf alles stellen (auch kuenftige Eintraege)
  --geraet <Name>       Ein anderes eigenes Geraet einstellen (Standard: dieser Rechner)

  Ohne Terminal wird nie gefragt — dann zeigt `select` nur an.

Optionen für `connect`:
  --name <Gerätename>   Abweichender Gerätename (Standard: Hostname)
  --no-claude-config    Claude-Code-Konfiguration nicht anfassen
  --neu                 Auch dann koppeln, wenn dieser Rechner schon verbunden ist.
                        Legt einen ZWEITEN Gerätezugang an — für ein Update ist das
                        nicht nötig, da genügt das Ersetzen des Programms.
";

/// Trägt den MCP-Server nachträglich in die vorhandenen Werkzeuge ein.
///
/// Gebraucht von allen, die mit `--no-claude-config` gekoppelt haben — und
/// beim Umstellen eines Rechners, der vorher auf einem anderen Weg an AI Brain
/// hing. Ohne diesen Befehl bliebe als Ausweg nur, ein zweites Mal zu koppeln,
/// und das legt einen überflüssigen zweiten Gerätezugang an.
///
/// `configure-claude` bleibt als Zweitname bestehen — der Befehl stand so in
/// der README, und wer ihn sich notiert hat, soll nicht ins Leere laufen.
fn configure_tools() -> Result<(), String> {
    // Erst prüfen, ob überhaupt eine Verbindung existiert: sonst schreiben wir
    // einen MCP-Eintrag, der beim Start sofort scheitert.
    let connection = config::load()?;

    println!("Instanz : {}", connection.base_url);
    println!("Gerät   : {}", connection.device_name);
    println!();

    let path = config::write_claude_config()?;
    println!("Claude Code : {}", path.display());

    match config::write_codex_config()? {
        Some(_) => println!("Codex       : Eintrag „ai-brain“ angelegt"),
        None => println!("Codex       : nicht installiert, übersprungen"),
    }

    println!();
    println!("Starte die Werkzeuge neu, damit die Einträge greifen.");

    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let result = match args.first().map(String::as_str) {
        Some("connect") => pairing::connect(&args[1..]),
        Some("mcp") => mcp::serve(),
        Some("sync") => sync::run(&args[1..]),
        Some("select") => select::run(&args[1..]),
        Some("publish") => sync::publish(&args[1..]),
        Some("status") => config::print_status(),
        Some("configure") | Some("configure-claude") => configure_tools(),
        Some("disconnect") => config::disconnect(),
        Some("--help") | Some("-h") | Some("help") | None => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(other) => {
            eprintln!("Unbekannter Befehl: {other}\n");
            print!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // Fehler gehen immer nach stderr: im `mcp`-Modus ist stdout der
            // JSON-RPC-Kanal, dort würde eine Fehlermeldung das Protokoll
            // zerstören und Claude Code mit einem Parse-Fehler abbrechen.
            eprintln!("Fehler: {err}");
            ExitCode::FAILURE
        }
    }
}
