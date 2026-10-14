//! AI Brain CLI
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
//! tot. Genau deshalb ist die CLI nicht nur ein Einrichtungsprogramm.

mod agent;
mod aufgaben;
mod auftrag;
mod config;
mod discovery;
mod freigabe;
mod imkr;
mod frage;
mod mcp;
mod nachricht;
mod pairing;
mod register;
mod select;
mod sync;

use std::process::ExitCode;

const USAGE: &str = "\
AI Brain CLI

Aufrufe:
  ai-brain-cli connect [domain]   Rechner mit einer AI-Brain-Instanz verbinden
                                  (ohne Angabe: die Instanz, von der dieses
                                  Programm kam)
  ai-brain-cli mcp                Als MCP-Server laufen (startet Claude Code selbst)
  ai-brain-cli select             Waehlen, welche Eintraege dieser Rechner bekommt
  ai-brain-cli sync               Skills und Agents aus dem Repository abgleichen
  ai-brain-cli publish <name>     Einen lokalen Eintrag ins Repository geben
  ai-brain-cli agent [...]        Einen Agentenlauf hier starten
  ai-brain-cli agents             Welche Agentenlaeufe hier laufen
  ai-brain-cli aufgaben [...]     Offene Aufgaben eines Projekts (--projekt, --json)
  ai-brain-cli auftraege          Was AI Brain fuer diesen Rechner bereitlegt
  ai-brain-cli auftrag <nummer>   Einen Auftrag annehmen und abarbeiten
  ai-brain-cli auftrag-melden ..  Ergebnis eines Auftrags nachtragen
  ai-brain-cli nachricht ..       Nachrichten zwischen imkrs:
                                  kontakte | senden <person> <text> |
                                  abholen | bestaetigen <nummer>
  ai-brain-cli freigabe [...]     Verzeichnisse fuer Laeufe mit mehr Rechten
  ai-brain-cli logs <kennung>     Was ein Lauf geschrieben hat
  ai-brain-cli stop <kennung>     Einen Lauf beenden
  ai-brain-cli status             Zeigt die aktuelle Verbindung
  ai-brain-cli configure          Traegt den MCP-Server in Claude Code und Codex ein
  ai-brain-cli disconnect         Entfernt die lokale Verbindung

Beispiel:
  ai-brain-cli connect brain.proxy.peppermint-digital.com

Optionen für `sync`:
  --apply               Änderungen wirklich schreiben (ohne: erst fragen)
  --only <name>         Nur diesen einen Eintrag
  --force               Auch überschreiben, wo lokal geändert wurde
                        (die lokale Fassung wird gesichert)
  --aufraeumen          Nicht mehr ausgewählte Einträge entfernen — aber nur solche,
                        die die CLI selbst geschrieben hat und die seither
                        unverändert sind
  --ja                  Nicht fragen, sichere Vorgabe nehmen: anwenden ja, aber
                        nichts überschreiben und nichts entfernen. Dafür bleiben
                        --force und --aufraeumen nötig.

  Am Terminal fragt `sync` vor dem Anwenden und bei jedem strittigen Eintrag
  einzeln (behalten / überschreiben / Unterschied ansehen). Ohne Terminal wird
  NIE gefragt — dann gilt, was an Optionen dasteht.

Optionen für `agent`:
  --task <N>            Aufgabe aus AI Brain (die Beschreibung wird geholt)
  --projekt <slug>      Projekt, in dem gearbeitet wird
  --modell claude|codex Welche Schleife gemietet wird (Vorgabe: claude)
  --dir <Pfad>          Arbeitsverzeichnis (Vorgabe: das aktuelle)
  --fortsetzen          Die letzte Unterhaltung in diesem Verzeichnis weiterfuehren
  --sitzung <id>        Eine bestimmte Unterhaltung weiterfuehren (siehe `logs`)
  --imkr                In einem neuen imkr-Fenster statt als Kindprozess
  --zeitgrenze <Sek>    Nach dieser Zeit abbrechen (Vorgabe 3600, 0 = keine)
  --schreibend          Darf Dateien schreiben, ohne zu fragen
  --vollzugriff         Fragt nichts mehr („God-Mode“)
  --trocken             Nur zeigen, was gestartet wuerde, samt Vorlauf

  --schreibend und --vollzugriff gehen NUR in freigegebenen Verzeichnissen:
    ai-brain-cli freigabe add ~/Development

  Freie Woerter werden der Auftrag: `agent --projekt x \"Changelog schreiben\"`.

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

    // Vor allem anderen: die Ablage des alten Namens herüberholen (#7664).
    // Steht hier und nicht in `config_dir()`, weil das eine reine
    // Pfadfunktion ist — ein Seiteneffekt darin würde in jedem Test feuern.
    config::uebernimm_alte_ablage();

    let result = match args.first().map(String::as_str) {
        Some("connect") => pairing::connect(&args[1..]),
        Some("mcp") => mcp::serve(),
        Some("sync") => sync::run(&args[1..]),
        Some("select") => select::run(&args[1..]),
        Some("publish") => sync::publish(&args[1..]),
        Some("agent") => agent::starten(&args[1..]),
        Some("freigabe") => freigabe::verwalten(&args[1..]),
        Some("agents") => register::anzeigen(&args[1..]),
        Some("aufgaben") => aufgaben::anzeigen(&args[1..]),
        Some("auftraege") => auftrag::anzeigen(&args[1..]),
        Some("auftrag") => auftrag::annehmen(&args[1..]),
        Some("auftrag-melden") => auftrag::nachmelden(&args[1..]),
        Some("nachricht") => nachricht::fuehren(&args[1..]),
        Some("logs") => register::protokoll_zeigen(&args[1..]),
        Some("stop") => register::stoppen(&args[1..]),
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
