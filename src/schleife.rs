//! Der eigene Agentenloop.
//!
//! **Warum ueberhaupt.** Bis hierhin mietet der Harness fremde Loops: `claude`
//! und `codex`. Das heisst, er kann nur, was die koennen, und wer ihn benutzen
//! will, muss erst die Befehlszeile eines Wettbewerbers installieren. Fuer
//! OpenRouter, Ollama und alles OpenAI-kompatible gibt es gar keinen Mietweg.
//!
//! **Was er bewusst NICHT ist.** Kein Ersatz fuer `claude`. Kompaktierung,
//! Unteragenten, Hooks und der Plan-Modus bleiben dort, und `--durchreichen`
//! bleibt der Weg dahin. Dieser Loop ist der Weg zu allem anderen.
//!
//! ## Der Ablauf
//!
//! ```text
//! Frage + Werkzeugliste  ->  Anbieter
//!                              |
//!              Text ohne Griff  |  Griffe
//!                   (fertig) <--+--> ausfuehren, Ergebnis anhaengen, erneut
//! ```
//!
//! ## Was dabei gemessen wurde (15.10.2026)
//!
//! | Befund | Folge im Code |
//! |---|---|
//! | `llama3.1:8b` greift, `qwen2.5:14b` nicht — auf demselben Ollama | `Werkzeugfaehig` als Eigenschaft des **Anbietereintrags**, dreiwertig |
//! | `llama3.1:8b` erfand `{"Laufzeit":"true"}` fuer ein Schema **ohne** Felder | [`argumente_filtern`] verwirft Unbekanntes, sichtbar |
//! | OpenAI-Weg liefert Argumente als Zeichenkette, nativer Weg als Objekt | `anbieter::argumente_lesen` nimmt beide |

use crate::anbieter::{self, Anbieter, Griff, Nachricht, Werkzeugfaehig};
use crate::rechner::{self, Hoheit};

/// Hoechstzahl Modellzuege in einem Lauf.
///
/// Ein Loop ohne Grenze ist ein Loop, der im Fehlerfall Geld verbrennt: greift
/// das Modell immer wieder dasselbe Werkzeug, laeuft er bis zum Abbruch durch
/// den Menschen — und bei einem unbeaufsichtigten Lauf gibt es keinen.
const ZUEGE_GRENZE: usize = 25;
const _: () = assert!(ZUEGE_GRENZE > 1 && ZUEGE_GRENZE <= 100);

/// Woher ein Werkzeug kommt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quelle {
    /// Aus AI Brain, ueber HTTP.
    AiBrain,
    /// Von diesem Rechner, im selben Prozess.
    Rechner,
}

/// Was dem Modell angeboten wird, und wer es ausfuehrt.
pub struct Katalog {
    pub werkzeuge: Vec<serde_json::Value>,
    herkunft: std::collections::BTreeMap<String, Quelle>,
}

impl Katalog {
    pub fn quelle(&self, name: &str) -> Option<Quelle> {
        self.herkunft.get(name).copied()
    }

    pub fn anzahl(&self) -> (usize, usize) {
        let rechner = self
            .herkunft
            .values()
            .filter(|q| **q == Quelle::Rechner)
            .count();

        (self.herkunft.len() - rechner, rechner)
    }
}

/// Baut den Katalog aus beiden Quellen.
///
/// Die Reihenfolge ist Absicht: der Rechner **zuletzt**. Traegt AI Brain
/// einmal ein Werkzeug mit demselben Namen, gewinnt das naehere — und ein
/// `rechner-befehl`, der heimlich ueber HTTP geht, waere die unangenehmere
/// Ueberraschung.
pub fn katalog(hoheit: Hoheit, mit_ai_brain: bool) -> Result<Katalog, String> {
    let mut werkzeuge: Vec<serde_json::Value> = Vec::new();
    let mut herkunft = std::collections::BTreeMap::new();

    if mit_ai_brain {
        let verbindung = crate::config::load()?;
        let token = crate::mcp::access_token(&verbindung)?;

        for w in crate::mcp::tools_list(&verbindung, &token)? {
            if let Some(name) = w.get("name").and_then(|n| n.as_str()) {
                herkunft.insert(name.to_string(), Quelle::AiBrain);
                werkzeuge.push(w);
            }
        }
    }

    for w in rechner::liste(hoheit) {
        if let Some(name) = w.get("name").and_then(|n| n.as_str()) {
            let name = name.to_string();

            if herkunft.insert(name.clone(), Quelle::Rechner) == Some(Quelle::AiBrain) {
                werkzeuge.retain(|v| v.get("name").and_then(|n| n.as_str()) != Some(&name));
            }

            werkzeuge.push(w);
        }
    }

    Ok(Katalog {
        werkzeuge,
        herkunft,
    })
}

/// Wirft Argumente weg, die im Schema nicht vorkommen.
///
/// **Warum das kein Schoenheitsfehler ist.** Gemessen am 15.10.2026 schickte
/// `llama3.1:8b` fuer ein Werkzeug mit **leerem** Schema die Argumente
/// `{"Laufzeit":"true"}` — frei erfunden. Durchgereicht trifft so etwas
/// serverseitige Pruefungen, und die Meldung lautet dann „ungueltiges
/// Argument", als haette der Harness etwas falsch gebaut.
///
/// Verworfenes wird **genannt**, nicht stillschweigend geschluckt: ein Modell,
/// das regelmaessig Felder erfindet, ist ein Befund ueber das Modell.
pub fn argumente_filtern(
    schema: &serde_json::Value,
    argumente: &serde_json::Value,
) -> (serde_json::Value, Vec<String>) {
    let Some(gegeben) = argumente.as_object() else {
        return (serde_json::json!({}), Vec::new());
    };

    // Kein `properties` im Schema heisst nicht „keine Felder erlaubt", sondern
    // „das Schema sagt nichts dazu". Dann wird nicht gefiltert — sonst wuerfe
    // ein knapp beschriebenes Werkzeug jedes richtige Argument weg.
    let Some(erlaubt) = schema.get("properties").and_then(|p| p.as_object()) else {
        return (argumente.clone(), Vec::new());
    };

    let mut behalten = serde_json::Map::new();
    let mut verworfen = Vec::new();

    for (schluessel, wert) in gegeben {
        if erlaubt.contains_key(schluessel) {
            behalten.insert(schluessel.clone(), wert.clone());
        } else {
            verworfen.push(schluessel.clone());
        }
    }

    (serde_json::Value::Object(behalten), verworfen)
}

fn schema_von(katalog: &Katalog, name: &str) -> serde_json::Value {
    katalog
        .werkzeuge
        .iter()
        .find(|w| w.get("name").and_then(|n| n.as_str()) == Some(name))
        .and_then(|w| w.get("inputSchema").cloned())
        .unwrap_or_else(|| serde_json::json!({}))
}

/// Fuehrt einen Griff aus und liefert den Text fuer das Modell.
fn greifen(katalog: &Katalog, griff: &Griff) -> (String, bool) {
    let Some(quelle) = katalog.quelle(&griff.name) else {
        return (
            format!(
                "Es gibt kein Werkzeug namens '{}'. Angeboten sind: {}",
                griff.name,
                katalog
                    .herkunft
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            true,
        );
    };

    let schema = schema_von(katalog, &griff.name);
    let (argumente, verworfen) = argumente_filtern(&schema, &griff.argumente);

    if !verworfen.is_empty() {
        eprintln!(
            "   ⚠ '{}' bekam erfundene Argumente, verworfen: {}",
            griff.name,
            verworfen.join(", ")
        );
    }

    let ergebnis = match quelle {
        Quelle::Rechner => rechner::fuer_maske(&griff.name, &argumente),
        Quelle::AiBrain => crate::config::load().and_then(|verbindung| {
            let token = crate::mcp::access_token(&verbindung)?;
            crate::mcp::call_tool_text(&verbindung, &token, &griff.name, argumente)
        }),
    };

    match ergebnis {
        Ok(text) => (text, false),
        Err(grund) => (grund, true),
    }
}

/// Einen Zug beim Anbieter fahren.
fn zug(
    wer: &Anbieter,
    verlauf: &[Nachricht],
    werkzeuge: &[serde_json::Value],
    auftrag: &str,
) -> Result<anbieter::Antwort, String> {
    let nutzlast = anbieter::nutzlast(wer, verlauf, werkzeuge, auftrag);
    let adresse = anbieter::adresse(wer);

    let mut anfrage = ureq::post(&adresse);

    for (kopf, inhalt) in anbieter::kopfzeilen(wer)? {
        anfrage = anfrage.set(&kopf, &inhalt);
    }

    let antwort = anfrage
        .send_string(&nutzlast.to_string())
        .map_err(|e| match e {
            ureq::Error::Status(code, resp) => {
                let koerper = resp.into_string().unwrap_or_default();
                format!("{} antwortet mit HTTP {code}: {}", wer.name, koerper.trim())
            }
            anders => format!("{} ist nicht erreichbar: {anders}", wer.name),
        })?;

    let text = antwort
        .into_string()
        .map_err(|e| format!("Antwort von {} ist unlesbar: {e}", wer.name))?;

    let roh: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("Antwort von {} ist kein JSON: {e} — {text}", wer.name))?;

    anbieter::antwort_lesen(wer.form, &roh)
}

const AUFTRAG: &str = "Du bist der Harness von AI Brain und laeufst auf dem Rechner des Nutzers. \
Du hast Werkzeuge fuer diesen Rechner (Praefix 'rechner-') und fuer AI Brain. \
Antworte auf Deutsch, knapp und sachlich. \
Wenn du etwas nachsehen kannst, sieh nach statt zu vermuten. \
Nenne keine Zahlen, die du nicht aus einem Werkzeug hast.";

/// Was ein abgeschlossener Lauf geliefert hat.
pub struct Ergebnis {
    pub text: String,
    pub zuege: usize,
    pub griffe: usize,
    pub marken: (u64, u64),
}

/// Der Loop.
pub fn fahren(
    wer: &Anbieter,
    katalog: &Katalog,
    frage: &str,
    laut: bool,
) -> Result<Ergebnis, String> {
    if wer.werkzeuge == Werkzeugfaehig::Nein && !katalog.werkzeuge.is_empty() {
        return Err(format!(
            "Anbieter '{}' ist als werkzeugunfaehig gemessen ({}). Ein Lauf mit Werkzeugen \
             laeuft dort ins Leere. Entweder ein anderes Modell, oder '--ohne-werkzeuge'.",
            wer.name, wer.modell
        ));
    }

    if let Some(fenster) = wer.kontext {
        let geschaetzt = anbieter::marken_schaetzen(&anbieter::nutzlast(
            wer,
            &[Nachricht::Nutzer(frage.to_string())],
            &katalog.werkzeuge,
            AUFTRAG,
        ));

        if geschaetzt > fenster {
            let (brain, rechner) = katalog.anzahl();

            return Err(format!(
                "Der Werkzeugkatalog passt nicht in das gemessene Kontextfenster von '{}': \
                 rund {geschaetzt} Marken bei {fenster} Platz ({brain} Werkzeuge aus AI Brain, \
                 {rechner} von diesem Rechner).\n\n\
                 Der Server wuerde den Rest **stillschweigend abschneiden** — das Modell saehe \
                 einen halben Katalog und griffe ins Leere.\n\n\
                 Wege: '--ohne-ai-brain' (nur Rechnerwerkzeuge), ein Anbieter mit groesserem \
                 Fenster, oder bei Ollama serverseitig OLLAMA_CONTEXT_LENGTH heraufsetzen \
                 (kostet VRAM) und danach neu messen.",
                wer.name
            ));
        }
    }

    let mut verlauf = vec![Nachricht::Nutzer(frage.to_string())];
    let mut marken = (0_u64, 0_u64);
    let mut griffe_gesamt = 0_usize;

    for nr in 1..=ZUEGE_GRENZE {
        let mut antwort = zug(wer, &verlauf, &katalog.werkzeuge, AUFTRAG)?;

        if let Some((ein, aus)) = antwort.marken {
            marken.0 += ein;
            marken.1 += aus;
        }

        // Kam kein Griff in der richtigen Form, aber einer steht im Text:
        // einsammeln statt den Zug wegzuwerfen. Siehe `griff_aus_text` — der
        // Fehler sitzt in der Uebertragung, nicht im Modell.
        if antwort.griffe.is_empty() {
            if let Some(text) = antwort.text.as_deref() {
                let bekannte: Vec<&str> =
                    katalog.herkunft.keys().map(String::as_str).collect();

                if let Some(gerettet) = anbieter::griff_aus_text(text, &bekannte) {
                    if laut {
                        eprintln!(
                            "   ⚠ Griff kam als Text statt in der Werkzeugform — eingesammelt."
                        );
                    }

                    antwort.griffe.push(gerettet);
                    antwort.text = None;
                }
            }
        }

        if antwort.griffe.is_empty() {
            let text = antwort.text.unwrap_or_else(|| {
                "Der Anbieter hat weder Text noch einen Werkzeuggriff geliefert.".to_string()
            });

            return Ok(Ergebnis {
                text,
                zuege: nr,
                griffe: griffe_gesamt,
                marken,
            });
        }

        if laut {
            if let Some(t) = antwort.text.as_deref().filter(|t| !t.trim().is_empty()) {
                println!("{t}");
            }
        }

        verlauf.push(Nachricht::Modell {
            text: antwort.text.clone(),
            griffe: antwort.griffe.clone(),
        });

        for griff in &antwort.griffe {
            griffe_gesamt += 1;

            if laut {
                eprintln!("   ⚒ {} {}", griff.name, kurz(&griff.argumente));
            }

            let (inhalt, fehler) = greifen(katalog, griff);

            verlauf.push(Nachricht::Ergebnis {
                id: griff.id.clone(),
                inhalt,
                fehler,
            });
        }
    }

    Err(format!(
        "Nach {ZUEGE_GRENZE} Zuegen und {griffe_gesamt} Griffen war das Modell nicht fertig. \
         Abgebrochen, damit ein im Kreis laufender Lauf kein Geld verbrennt."
    ))
}

fn kurz(argumente: &serde_json::Value) -> String {
    let Some(felder) = argumente.as_object() else {
        return String::new();
    };

    if felder.is_empty() {
        return String::new();
    }

    let text = felder
        .iter()
        .map(|(k, v)| {
            let wert = match v {
                serde_json::Value::String(s) => s.clone(),
                anders => anders.to_string(),
            };

            format!("{k}={}", schneiden(&wert, 40))
        })
        .collect::<Vec<_>>()
        .join(" ");

    schneiden(&text, 90)
}

fn schneiden(text: &str, grenze: usize) -> String {
    let einzeilig = text.replace('\n', " ");

    if einzeilig.chars().count() <= grenze {
        return einzeilig;
    }

    // Nach **Zeichen** schneiden, nicht nach Bytes: ein Schnitt mitten in ein
    // Mehrbyte-Zeichen laesst Rust in Panik geraten, und Umlaute sind hier die
    // Regel.
    einzeilig.chars().take(grenze).collect::<String>() + "…"
}

// ---------------------------------------------------------------------------
// Die Messung
// ---------------------------------------------------------------------------

/// Was eine Messung ergeben hat.
pub struct Messung {
    pub proben: usize,
    pub strukturiert: usize,
    pub aus_text: usize,
    /// Das gemessene Kontextfenster — `None` heisst „bei der Probengroesse
    /// kein Deckel gefunden", **nicht** „unbegrenzt".
    pub kontext: Option<u64>,
}

impl Messung {
    pub fn faehig(&self) -> bool {
        self.strukturiert + self.aus_text > 0
    }

    pub fn satz(&self) -> String {
        if !self.faehig() {
            return format!("greift NICHT ({}/{} Proben)", 0, self.proben);
        }

        let mut teile = vec![format!(
            "{}/{} in der Werkzeugform",
            self.strukturiert, self.proben
        )];

        if self.aus_text > 0 {
            teile.push(format!("{} aus dem Text gerettet", self.aus_text));
        }

        teile.join(", ")
    }
}

/// Anzahl Proben pro Messung.
///
/// **Warum nicht eine.** Am 15.10.2026 habe ich `qwen2.5:14b` mit **einer**
/// Probe gemessen, keinen Griff bekommen und „kann nicht" notiert. Drei Proben
/// spaeter: zwei richtige Griffe, einer als Text. Das Modell kann es, nur
/// unzuverlaessig — und eine Stichprobe von eins kann zwischen „kann nicht"
/// und „kann meistens" nicht unterscheiden. Genau dieser Unterschied
/// entscheidet aber, ob man das Modell austauscht oder den Adapter haertet.
const PROBEN: usize = 3;
const _: () = assert!(PROBEN >= 2);

/// Misst, ob ein Anbieter Werkzeuge greifen kann — mit echten Aufrufen.
///
/// **Warum gemessen und nicht aus der Dokumentation gelesen.** Ollamas
/// Dokumentation nennt Werkzeuge als Faehigkeit der Schnittstelle. Das ist
/// richtig und beantwortet die Frage nicht — die Schnittstelle kann es, das
/// Modell vielleicht nicht, und zwei Modelle auf demselben Ollama verhalten
/// sich verschieden.
pub fn messen(wer: &Anbieter) -> Result<Messung, String> {
    const NAME: &str = "probe_laufzeit";

    let probe = serde_json::json!({
        "name": NAME,
        "description": "Liefert die Laufzeit dieses Rechners. Nimmt keine Argumente.",
        "inputSchema": { "type": "object", "properties": {} },
    });

    let verlauf = vec![Nachricht::Nutzer(
        "Wie lange laeuft dieser Rechner? Benutze das Werkzeug.".to_string(),
    )];

    let mut messung = Messung {
        proben: PROBEN,
        strukturiert: 0,
        aus_text: 0,
        kontext: kontext_messen(wer),
    };

    for _ in 0..PROBEN {
        let antwort = zug(
            wer,
            &verlauf,
            std::slice::from_ref(&probe),
            "Du hast ein Werkzeug. Benutze es.",
        )?;

        if antwort.griffe.iter().any(|g| g.name == NAME) {
            messung.strukturiert += 1;
            continue;
        }

        match antwort
            .text
            .as_deref()
            .and_then(|t| anbieter::griff_aus_text(t, &[NAME]))
        {
            Some(_) => messung.aus_text += 1,
            None => {
                if let Some(text) = antwort.text.as_deref() {
                    eprintln!("      ohne Griff, Text: {}", schneiden(text, 120));
                }
            }
        }
    }

    Ok(messung)
}

/// Groesse der Kontextprobe, in geschaetzten Marken.
const KONTEXTPROBE: u64 = 12_000;
const _: () = assert!(KONTEXTPROBE >= 4_096 * 2);

/// Misst, wo der Server den Prompt abschneidet.
///
/// **Warum ueberhaupt.** Gemessen am 15.10.2026: `llama3.1:8b` kann 131072
/// Marken, Ollama wertete aber genau 4096 aus und meldete keinen Fehler. Mit
/// 188 angebotenen Werkzeugen sah der halbierte Katalog aus wie ein dummes
/// Modell — es griff etwas Unverwandtes und reichte eine Fehlermeldung als
/// Argumente durch.
///
/// Ueber den OpenAI-kompatiblen Weg laesst sich das pro Anfrage nicht heben
/// (geprueft: `options.num_ctx` und ein Feld auf oberster Ebene wirken beide
/// nicht). Also bleibt nur: den Deckel **kennen** und davor haltmachen.
///
/// Die Probe schickt bewusst mehr, als in ein kleines Fenster passt, und liest
/// zurueck, wie viel davon gewertet wurde. Kommt weniger zurueck als geschickt,
/// ist das der Deckel.
fn kontext_messen(wer: &Anbieter) -> Option<u64> {
    let fuellung = "Das ist Fuellmaterial fuer die Kontextmessung. "
        .repeat((KONTEXTPROBE * 4 / 46) as usize);

    let verlauf = vec![Nachricht::Nutzer(fuellung)];
    let geschaetzt = anbieter::marken_schaetzen(&anbieter::nutzlast(wer, &verlauf, &[], "x"));

    let antwort = zug(wer, &verlauf, &[], "x").ok()?;
    let (gewertet, _) = antwort.marken?;

    // Zehn Prozent Spielraum: die Schaetzung ist grob, und ein paar Prozent
    // Abweichung sind kein Deckel.
    if gewertet as f64 >= geschaetzt as f64 * 0.9 {
        return None;
    }

    Some(gewertet)
}

// ---------------------------------------------------------------------------
// Befehlszeile
// ---------------------------------------------------------------------------

const HILFE: &str = "\
ai-brain-harness schleife — der eigene Agentenloop

  schleife <frage>                 Einen Lauf mit dem Standardanbieter
  schleife --liste                 Eingetragene Anbieter zeigen
  schleife --messen [<name>]       Werkzeugfaehigkeit messen (echter Aufruf)
  schleife --standard <name>       Standardanbieter setzen
  schleife --eintragen <name> --form <openai|anthropic> --basis <url> --modell <m>
                                   [--schluessel-variable <VAR>]

Zusaetze:
  --anbieter <name>                Diesen Anbieter statt des Standards
  --rechte steuernd|lesend         Rechte am Rechner (Standard: lesend)
  --ohne-werkzeuge                 Reines Gespraech, nichts angeboten
  --ohne-ai-brain                  Nur Rechnerwerkzeuge anbieten
  --still                          Keine Zwischenmeldungen
";

fn wert<'a>(args: &'a [String], flagge: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flagge)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

pub fn verwalten(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HILFE}");
        return Ok(());
    }

    if args.iter().any(|a| a == "--liste") {
        return liste_zeigen();
    }

    if args.iter().any(|a| a == "--eintragen") {
        return eintragen(args);
    }

    if let Some(name) = wert(args, "--standard") {
        return standard_setzen(name);
    }

    if args.iter().any(|a| a == "--messen") {
        return messung_fahren(args);
    }

    lauf(args)
}

fn liste_zeigen() -> Result<(), String> {
    let ablage = anbieter::laden()?;

    if ablage.anbieter.is_empty() {
        println!("Kein Anbieter eingetragen.");
        println!();
        println!("Beispiele:");
        println!(
            "  schleife --eintragen ollama --form openai \\\n    \
             --basis http://127.0.0.1:11434/v1 --modell llama3.1:8b"
        );
        println!(
            "  schleife --eintragen openrouter --form openai \\\n    \
             --basis https://openrouter.ai/api/v1 --modell qwen/qwen3-235b-a22b \\\n    \
             --schluessel-variable OPENROUTER_API_KEY"
        );
        return Ok(());
    }

    println!("Anbieter:");
    println!();

    for a in &ablage.anbieter {
        let stern = if ablage.standard.as_deref() == Some(&a.name) {
            "*"
        } else {
            " "
        };

        println!("{stern} {}", a.name);
        println!("    Form   : {}", a.form.name());
        println!("    Basis  : {}", a.basis);
        println!("    Modell : {}", a.modell);
        println!(
            "    Rechte : {}",
            a.schluessel_variable
                .as_deref()
                .map(|v| format!("${v}"))
                .unwrap_or_else(|| "kein Schluessel noetig".to_string())
        );
        println!("    {}", a.werkzeuge.zeichen());

        if a.werkzeuge == Werkzeugfaehig::Ungemessen {
            println!("             → mit 'schleife --messen {}' pruefen", a.name);
        }

        println!();
    }

    if ablage.standard.is_none() {
        println!("Kein Standard gesetzt — mit '--standard <name>'.");
    }

    Ok(())
}

fn eintragen(args: &[String]) -> Result<(), String> {
    let name = wert(args, "--eintragen")
        .ok_or_else(|| "'--eintragen' braucht einen Namen.".to_string())?;

    let form = anbieter::Form::lesen(
        wert(args, "--form").ok_or_else(|| "'--form' fehlt (openai oder anthropic).".to_string())?,
    )?;

    let basis = wert(args, "--basis")
        .ok_or_else(|| "'--basis' fehlt (die Adresse des Anbieters).".to_string())?;

    let modell =
        wert(args, "--modell").ok_or_else(|| "'--modell' fehlt (der Modellname).".to_string())?;

    let mut ablage = anbieter::laden()?;

    let neu = Anbieter {
        name: name.to_string(),
        form,
        basis: basis.trim_end_matches('/').to_string(),
        modell: modell.to_string(),
        schluessel_variable: wert(args, "--schluessel-variable").map(str::to_string),
        // Ein neuer Eintrag ist **ungemessen**, nicht faehig. Wer hier `Ja`
        // voreinstellt, baut die Annahme ein, die der ganze Befund widerlegt.
        werkzeuge: Werkzeugfaehig::Ungemessen,
        kontext: None,
    };

    match ablage.anbieter.iter_mut().find(|a| a.name == name) {
        Some(alt) => {
            println!("Anbieter '{name}' ueberschrieben.");
            *alt = neu;
        }
        None => {
            println!("Anbieter '{name}' eingetragen.");
            ablage.anbieter.push(neu);
        }
    }

    if ablage.standard.is_none() {
        ablage.standard = Some(name.to_string());
        println!("Als Standard gesetzt (es gab noch keinen).");
    }

    let pfad = anbieter::sichern(&ablage)?;
    println!("Ablage: {}", pfad.display());
    println!();
    println!("Jetzt messen, ob das Modell Werkzeuge greifen kann:");
    println!("  ai-brain-harness schleife --messen {name}");

    Ok(())
}

fn standard_setzen(name: &str) -> Result<(), String> {
    let mut ablage = anbieter::laden()?;

    if ablage.finden(name).is_none() {
        return Err(format!("Anbieter '{name}' ist nicht eingetragen."));
    }

    ablage.standard = Some(name.to_string());
    anbieter::sichern(&ablage)?;

    println!("Standardanbieter: {name}");

    Ok(())
}

fn messung_fahren(args: &[String]) -> Result<(), String> {
    let ablage = anbieter::laden()?;
    let wunsch = wert(args, "--messen").or_else(|| wert(args, "--anbieter"));

    // Ohne Namen ALLE messen: so bleibt die Messung wiederholbar, wenn Modelle
    // dazukommen oder getauscht werden.
    let zu_messen: Vec<&Anbieter> = match wunsch {
        Some(name) => vec![ablage.waehlen(Some(name))?],
        None if ablage.anbieter.is_empty() => {
            return Err("Kein Anbieter eingetragen — nichts zu messen.".to_string());
        }
        None => ablage.anbieter.iter().collect(),
    };

    println!("Messe Werkzeugfaehigkeit mit einem echten Aufruf.");
    println!();

    let mut fehlgeschlagen = Vec::new();

    for a in zu_messen {
        print!("  {} ({}) … ", a.name, a.modell);
        use std::io::Write;
        let _ = std::io::stdout().flush();

        match messen(a) {
            Ok(messung) => {
                println!("{}", messung.satz());
                anbieter::befund_merken(&a.name, messung.faehig(), messung.kontext)?;

                match messung.kontext {
                    Some(fenster) => println!(
                        "      Kontextfenster: {fenster} Marken — der Server deckelt dort, \
                         ohne es zu melden."
                    ),
                    None => println!(
                        "      Kontextfenster: bei {KONTEXTPROBE} Marken kein Deckel gefunden."
                    ),
                }

                if messung.aus_text > 0 {
                    println!(
                        "      → Der Loop sammelt Text-Griffe ein; verlaesslich ist das Modell \
                         damit trotzdem nicht."
                    );
                }
            }
            Err(grund) => {
                println!("nicht messbar");
                println!("      {grund}");
                // Nicht messbar ist **nicht** „kann nicht". Ein unerreichbarer
                // Anbieter wuerde sonst dauerhaft als unfaehig vermerkt, und
                // der Eintrag waere nach dem naechsten Start falsch.
                fehlgeschlagen.push(a.name.clone());
            }
        }
    }

    if !fehlgeschlagen.is_empty() {
        println!();
        println!(
            "Nicht gemessen (bleiben ungemessen): {}",
            fehlgeschlagen.join(", ")
        );
    }

    Ok(())
}

fn lauf(args: &[String]) -> Result<(), String> {
    let frage: Vec<&str> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| {
            if a.starts_with("--") {
                return false;
            }

            // Der Wert hinter einer Flagge ist kein Teil der Frage.
            !args
                .get(i.wrapping_sub(1))
                .is_some_and(|v| matches!(v.as_str(), "--anbieter" | "--rechte"))
        })
        .map(|(_, a)| a.as_str())
        .collect();

    let frage = frage.join(" ");

    if frage.trim().is_empty() {
        print!("{HILFE}");
        return Ok(());
    }

    let ablage = anbieter::laden()?;
    let gewaehlt = ablage.waehlen(wert(args, "--anbieter"))?;

    let hoheit = match wert(args, "--rechte") {
        Some("steuernd") => Hoheit::Steuernd,
        Some("lesend") | None => Hoheit::Lesend,
        Some(anders) => {
            return Err(format!(
                "Rechte '{anders}' kenne ich nicht — 'lesend' oder 'steuernd'."
            ));
        }
    };

    // Dieselbe Variable, die die Maske setzt: der Riegel in `rechner.rs`
    // entscheidet weiter, nicht der Loop.
    if hoheit == Hoheit::Steuernd {
        std::env::set_var("AI_BRAIN_RECHNER_RECHTE", "steuernd");
    }

    let laut = !args.iter().any(|a| a == "--still");

    let katalog = if args.iter().any(|a| a == "--ohne-werkzeuge") {
        Katalog {
            werkzeuge: Vec::new(),
            herkunft: std::collections::BTreeMap::new(),
        }
    } else {
        katalog(hoheit, !args.iter().any(|a| a == "--ohne-ai-brain"))?
    };

    if laut {
        let (brain, rechner) = katalog.anzahl();
        eprintln!(
            "⟡ {} · {} · {brain} Werkzeuge aus AI Brain, {rechner} von diesem Rechner",
            gewaehlt.name, gewaehlt.modell
        );
    }

    let ergebnis = fahren(gewaehlt, &katalog, &frage, laut)?;

    println!("{}", ergebnis.text);

    if laut {
        eprintln!();
        eprintln!(
            "⟡ {} Zuege, {} Griffe, {} ein / {} aus",
            ergebnis.zuege, ergebnis.griffe, ergebnis.marken.0, ergebnis.marken.1
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_mit_feldern() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "befehl": { "type": "string" },
                "frist": { "type": "number" },
            },
            "required": ["befehl"],
        })
    }

    #[test]
    fn erfundene_argumente_werden_verworfen_und_genannt() {
        // Der gemessene Fall: `llama3.1:8b` schickte {"Laufzeit":"true"} fuer
        // ein Werkzeug mit leerem Schema.
        let leer = serde_json::json!({ "type": "object", "properties": {} });
        let (behalten, verworfen) =
            argumente_filtern(&leer, &serde_json::json!({ "Laufzeit": "true" }));

        assert_eq!(behalten, serde_json::json!({}));
        assert_eq!(verworfen, vec!["Laufzeit".to_string()]);
    }

    #[test]
    fn echte_argumente_bleiben_stehen() {
        let (behalten, verworfen) = argumente_filtern(
            &schema_mit_feldern(),
            &serde_json::json!({ "befehl": "uptime", "frist": 10 }),
        );

        assert_eq!(
            behalten,
            serde_json::json!({ "befehl": "uptime", "frist": 10 })
        );
        assert!(verworfen.is_empty());
    }

    #[test]
    fn nur_das_erfundene_feld_faellt_weg() {
        let (behalten, verworfen) = argumente_filtern(
            &schema_mit_feldern(),
            &serde_json::json!({ "befehl": "uptime", "dringend": true }),
        );

        assert_eq!(behalten, serde_json::json!({ "befehl": "uptime" }));
        assert_eq!(verworfen, vec!["dringend".to_string()]);
    }

    #[test]
    fn schema_ohne_properties_filtert_nicht() {
        // „Das Schema sagt nichts dazu" ist nicht „keine Felder erlaubt".
        // Wer hier filtert, wirft bei knapp beschriebenen Werkzeugen jedes
        // richtige Argument weg.
        let stumm = serde_json::json!({ "type": "object" });
        let gegeben = serde_json::json!({ "irgendwas": 1 });
        let (behalten, verworfen) = argumente_filtern(&stumm, &gegeben);

        assert_eq!(behalten, gegeben);
        assert!(verworfen.is_empty());
    }

    #[test]
    fn nicht_objekt_wird_leeres_objekt() {
        let (behalten, _) = argumente_filtern(
            &schema_mit_feldern(),
            &serde_json::json!("das ist kein objekt"),
        );

        assert_eq!(behalten, serde_json::json!({}));
    }

    // -- Katalog -------------------------------------------------------------

    #[test]
    fn rechnerwerkzeuge_sind_im_katalog_und_lesend_gefiltert() {
        let lesend = katalog(Hoheit::Lesend, false).expect("ohne AI Brain muss das gehen");
        let steuernd = katalog(Hoheit::Steuernd, false).expect("ohne AI Brain muss das gehen");

        let (brain, rechner_lesend) = lesend.anzahl();
        assert_eq!(brain, 0, "ohne AI Brain darf nichts von dort kommen");

        let (_, rechner_steuernd) = steuernd.anzahl();
        assert!(
            rechner_steuernd > rechner_lesend,
            "ein steuernder Lauf muss mehr Werkzeuge anbieten als ein lesender \
             ({rechner_steuernd} vs {rechner_lesend})"
        );

        assert_eq!(lesend.quelle("rechner-last"), Some(Quelle::Rechner));
        assert_eq!(
            lesend.quelle("rechner-befehl"),
            None,
            "ein aenderndes Werkzeug darf im lesenden Lauf nicht angeboten werden"
        );
    }

    #[test]
    fn unbekannter_griff_nennt_die_angebotenen() {
        let k = katalog(Hoheit::Lesend, false).expect("ohne AI Brain muss das gehen");

        let (text, fehler) = greifen(
            &k,
            &Griff {
                id: "x".to_string(),
                name: "gibt-es-nicht".to_string(),
                argumente: serde_json::json!({}),
            },
        );

        assert!(fehler);
        assert!(
            text.contains("rechner-last"),
            "die Meldung muss nennen, was es stattdessen gibt: {text}"
        );
    }

    #[test]
    fn schema_wird_zum_namen_gefunden() {
        let k = katalog(Hoheit::Lesend, false).expect("ohne AI Brain muss das gehen");
        let schema = schema_von(&k, "rechner-last");

        assert_eq!(schema["type"], "object", "Schema fehlt: {schema}");
    }

    // -- Kuerzen -------------------------------------------------------------

    #[test]
    fn umlaute_werden_nicht_mitten_im_zeichen_geschnitten() {
        // Nach Bytes geschnitten geraet Rust hier in Panik.
        let lang = "ä".repeat(200);
        let kurz = schneiden(&lang, 10);

        assert_eq!(kurz.chars().count(), 11, "10 Zeichen plus Auslassung");
    }

    #[test]
    fn mehrzeiliges_wird_einzeilig() {
        assert_eq!(schneiden("a\nb", 10), "a b");
    }

    #[test]
    fn kurze_argumente_bleiben_lesbar() {
        let text = kurz(&serde_json::json!({ "befehl": "uptime" }));
        assert_eq!(text, "befehl=uptime");
    }

    #[test]
    fn leere_argumente_ergeben_leeren_text() {
        assert_eq!(kurz(&serde_json::json!({})), "");
    }

    // -- Messung -------------------------------------------------------------

    #[test]
    fn messung_ohne_treffer_ist_unfaehig() {
        let m = Messung {
            proben: 3,
            strukturiert: 0,
            aus_text: 0,
            kontext: None,
        };

        assert!(!m.faehig());
        assert!(m.satz().contains("NICHT"), "{}", m.satz());
    }

    #[test]
    fn ein_gerettetes_zaehlt_als_faehig_wird_aber_benannt() {
        // Der gemessene Fall qwen2.5:14b: 2 richtig, 1 als Text.
        let m = Messung {
            proben: 3,
            strukturiert: 2,
            aus_text: 1,
            kontext: Some(4096),
        };

        assert!(m.faehig());
        let satz = m.satz();
        assert!(satz.contains("2/3"), "{satz}");
        assert!(
            satz.contains("gerettet"),
            "die Unzuverlaessigkeit muss sichtbar bleiben: {satz}"
        );
    }

    /// Die Lehre vom 15.10.2026 als Zusicherung im **Bau**, nicht zur Laufzeit:
    /// eine Stichprobe von eins kann „kann nicht" nicht von „kann meistens"
    /// unterscheiden. Sinkt `PROBEN` auf 1, kommt der Harness nicht mehr
    /// zusammen — das ist der Riegel, den ein Test nicht leisten kann.
    const _: () = assert!(PROBEN >= 2);
}
