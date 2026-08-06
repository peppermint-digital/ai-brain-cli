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
mod mcp;
mod pairing;

use std::process::ExitCode;

const USAGE: &str = "\
AI Brain Companion

Aufrufe:
  ai-brain-companion connect <domain>   Rechner mit einer AI-Brain-Instanz verbinden
  ai-brain-companion mcp                Als MCP-Server laufen (startet Claude Code selbst)
  ai-brain-companion status             Zeigt die aktuelle Verbindung
  ai-brain-companion disconnect         Entfernt die lokale Verbindung

Beispiel:
  ai-brain-companion connect brain.proxy.peppermint-digital.com

Optionen für `connect`:
  --name <Gerätename>   Abweichender Gerätename (Standard: Hostname)
  --no-claude-config    Claude-Code-Konfiguration nicht anfassen
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let result = match args.first().map(String::as_str) {
        Some("connect") => pairing::connect(&args[1..]),
        Some("mcp") => mcp::serve(),
        Some("status") => config::print_status(),
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
