//! Anbieter und ihre zwei Formen.
//!
//! Der Harness mietet nicht mehr nur `claude`. Er spricht selbst mit
//! Sprachmodellen — und zwar in genau zwei Formen, nicht in einer pro Anbieter:
//!
//! | Form        | Weg                       | Deckt ab                          |
//! |-------------|---------------------------|-----------------------------------|
//! | `openai`    | `/v1/chat/completions`    | OpenRouter, Ollama, vLLM, Groq, … |
//! | `anthropic` | `/v1/messages`            | Anthropic mit API-Schluessel      |
//!
//! Ollama hat zwar einen **eigenen** Weg (`/api/chat`) mit eigenem
//! Werkzeugformat. Gemessen am 15.10.2026 liefert er bei einem faehigen Modell
//! dasselbe Ergebnis wie der OpenAI-kompatible Weg — ein dritter Vertrag ohne
//! Gegenwert. Deshalb bleibt es bei zwei.

use std::collections::BTreeMap;

/// Die Form, in der ein Anbieter angesprochen wird.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    OpenAi,
    Anthropic,
}

impl Form {
    pub fn lesen(text: &str) -> Result<Self, String> {
        match text {
            "openai" => Ok(Form::OpenAi),
            "anthropic" => Ok(Form::Anthropic),
            anders => Err(format!(
                "Form '{anders}' kenne ich nicht. Es gibt 'openai' und 'anthropic'."
            )),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Form::OpenAi => "openai",
            Form::Anthropic => "anthropic",
        }
    }
}

/// Ob ein Modell Werkzeuge greifen kann.
///
/// Drei Zustaende, nicht zwei. `Ungemessen` ist nicht dasselbe wie `Nein` —
/// gemessen am 15.10.2026 kann `llama3.1:8b` auf Ollama Werkzeuge greifen und
/// `qwen2.5:14b` auf demselben Ollama nicht. Werkzeugfaehigkeit gehoert also
/// zum **Modell**, nicht zum Anbieter, und laesst sich aus der Adresse nicht
/// ableiten. Wer sie raet, baut einen Loop, der Werkzeuge anbietet, die das
/// Modell ignoriert — und das sieht aus wie ein Fehler im Loop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Werkzeugfaehig {
    Ja,
    Nein,
    Ungemessen,
}

impl Werkzeugfaehig {
    fn lesen(wert: Option<bool>) -> Self {
        match wert {
            Some(true) => Werkzeugfaehig::Ja,
            Some(false) => Werkzeugfaehig::Nein,
            None => Werkzeugfaehig::Ungemessen,
        }
    }

    fn schreiben(&self) -> Option<bool> {
        match self {
            Werkzeugfaehig::Ja => Some(true),
            Werkzeugfaehig::Nein => Some(false),
            Werkzeugfaehig::Ungemessen => None,
        }
    }

    pub fn zeichen(&self) -> &'static str {
        match self {
            Werkzeugfaehig::Ja => "Werkzeuge: ja",
            Werkzeugfaehig::Nein => "Werkzeuge: NEIN",
            Werkzeugfaehig::Ungemessen => "Werkzeuge: ungemessen",
        }
    }
}

/// Ein eingetragener Anbieter.
#[derive(Clone, Debug)]
pub struct Anbieter {
    pub name: String,
    pub form: Form,
    pub basis: String,
    pub modell: String,
    /// Umgebungsvariable, aus der der Schluessel kommt. Der Schluessel selbst
    /// wird **nicht** in der Ablage gehalten — ein Klartext-Schluessel in einer
    /// Konfigurationsdatei ist genau die offene Tuer, die wir beim
    /// Credential-Proxy vermieden haben.
    pub schluessel_variable: Option<String>,
    pub werkzeuge: Werkzeugfaehig,
    /// Das Kontextfenster, das der **Server** wirklich benutzt — gemessen.
    ///
    /// **Warum gemessen und nicht aus dem Modell gelesen.** `llama3.1:8b` kann
    /// 131072 Marken. Ollama deckelt trotzdem bei 4096, wenn
    /// `OLLAMA_CONTEXT_LENGTH` nicht gesetzt ist — und **meldet das nicht**.
    /// Gemessen am 15.10.2026: `prompt_eval_count` kam als genau `4096`
    /// zurueck, obwohl der Prompt 4829 Marken lang war. Ueber den
    /// OpenAI-kompatiblen Weg laesst sich das pro Anfrage auch nicht
    /// hochsetzen; weder `options.num_ctx` noch ein Feld oben wirken.
    ///
    /// Die Folge im Betrieb sah wie Modelldummheit aus: mit 188 angebotenen
    /// Werkzeugen griff das Modell etwas Unverwandtes und reichte eine
    /// **Fehlermeldung als Argumente** durch. Es hatte den Katalog nur zur
    /// Haelfte gesehen.
    pub kontext: Option<u64>,
}

impl Anbieter {
    pub fn schluessel(&self) -> Result<Option<String>, String> {
        match &self.schluessel_variable {
            None => Ok(None),
            Some(variable) => match std::env::var(variable) {
                Ok(wert) if !wert.trim().is_empty() => Ok(Some(wert)),
                _ => Err(format!(
                    "Anbieter '{}' braucht einen Schluessel aus ${variable} — die Variable ist leer oder fehlt.",
                    self.name
                )),
            },
        }
    }

    fn lesen(name: &str, wert: &serde_json::Value) -> Result<Self, String> {
        let feld = |schluessel: &str| -> Result<String, String> {
            wert.get(schluessel)
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("Anbieter '{name}': Feld '{schluessel}' fehlt."))
        };

        Ok(Anbieter {
            name: name.to_string(),
            form: Form::lesen(&feld("form")?)?,
            basis: feld("basis")?.trim_end_matches('/').to_string(),
            modell: feld("modell")?,
            schluessel_variable: wert
                .get("schluessel_variable")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            werkzeuge: Werkzeugfaehig::lesen(
                wert.get("werkzeuge").and_then(|v| v.as_bool()),
            ),
            kontext: wert.get("kontext").and_then(|v| v.as_u64()),
        })
    }

    fn schreiben(&self) -> serde_json::Value {
        let mut eintrag = serde_json::json!({
            "form": self.form.name(),
            "basis": self.basis,
            "modell": self.modell,
        });

        if let Some(variable) = &self.schluessel_variable {
            eintrag["schluessel_variable"] = serde_json::json!(variable);
        }

        if let Some(wert) = self.werkzeuge.schreiben() {
            eintrag["werkzeuge"] = serde_json::json!(wert);
        }

        if let Some(wert) = self.kontext {
            eintrag["kontext"] = serde_json::json!(wert);
        }

        eintrag
    }
}

/// Ein Werkzeuggriff, den das Modell verlangt.
#[derive(Clone, Debug, PartialEq)]
pub struct Griff {
    pub id: String,
    pub name: String,
    pub argumente: serde_json::Value,
}

/// Eine Nachricht im Gespraech — anbieterneutral.
///
/// Die Umwandlung in die Form des Anbieters passiert erst im Adapter. Dadurch
/// bleibt der Loop selbst frei von Anbieterwissen, und ein dritter Anbieter
/// kostet einen Adapter statt eines Umbaus.
#[derive(Clone, Debug)]
pub enum Nachricht {
    Nutzer(String),
    Modell {
        text: Option<String>,
        griffe: Vec<Griff>,
    },
    Ergebnis {
        id: String,
        inhalt: String,
        fehler: bool,
    },
}

/// Was ein Zug des Modells geliefert hat.
#[derive(Clone, Debug)]
pub struct Antwort {
    pub text: Option<String>,
    pub griffe: Vec<Griff>,
    pub marken: Option<(u64, u64)>,
}

/// Zieht die Argumente aus einem Griff — in **beiden** Formen.
///
/// Gemessen am 15.10.2026: Ollamas OpenAI-kompatibler Weg liefert `arguments`
/// als **Zeichenkette** mit JSON darin, der native Weg als **Objekt**. Beides
/// ist im Umlauf, und wer nur eine Form annimmt, scheitert beim Wechsel des
/// Anbieters an einer Stelle, die wie ein Modellfehler aussieht.
fn argumente_lesen(wert: Option<&serde_json::Value>) -> serde_json::Value {
    match wert {
        Some(serde_json::Value::String(text)) => {
            if text.trim().is_empty() {
                return serde_json::json!({});
            }

            serde_json::from_str(text).unwrap_or_else(|_| serde_json::json!({}))
        }
        Some(objekt @ serde_json::Value::Object(_)) => objekt.clone(),
        _ => serde_json::json!({}),
    }
}

/// Zieht einen Werkzeuggriff aus dem **Text** der Antwort.
///
/// **Warum es das braucht.** Gemessen am 15.10.2026 mit `qwen2.5:14b` auf
/// Ollama: von drei gleichen Anfragen ohne Systemauftrag kamen zwei als
/// richtiger Griff zurueck und eine als Text —
///
/// ```text
/// ичество
/// {"name": "probe_laufzeit", "arguments": {}}
/// </tool_call>
/// ```
///
/// Das durchgesickerte `</tool_call>` nennt die Ursache: Ollama klammert
/// Werkzeuggriffe in Marken und liest sie beim Empfang zurueck. Stolpert das
/// Modell beim oeffnenden Teil, scheitert das Zuruecklesen, und der fertige
/// Griff landet unzerlegt im Textfeld. Das Modell hat seine Arbeit getan; die
/// Form ist auf dem Weg kaputtgegangen.
///
/// Ein Loop, der hier aufgibt, verliert einen ganzen Zug an einem Fehler, der
/// nicht ihm und nicht dem Modell gehoert.
///
/// **Der Riegel gegen Phantomgriffe:** Es wird nur anerkannt, was auf einen
/// Namen aus der **angebotenen** Liste zeigt. Sonst wuerde ein Modell, das
/// ueber JSON spricht („du koenntest `{"name": "loeschen"}` schicken"),
/// versehentlich einen Griff ausloesen.
pub fn griff_aus_text(text: &str, bekannte: &[&str]) -> Option<Griff> {
    let sauber = text.replace("<tool_call>", " ").replace("</tool_call>", " ");

    for anfang in sauber
        .char_indices()
        .filter(|(_, z)| *z == '{')
        .map(|(i, _)| i)
    {
        let Some(ende) = klammer_ende(&sauber, anfang) else {
            continue;
        };

        let Ok(wert) = serde_json::from_str::<serde_json::Value>(&sauber[anfang..ende]) else {
            continue;
        };

        let Some(name) = wert.get("name").and_then(|n| n.as_str()) else {
            continue;
        };

        if !bekannte.contains(&name) {
            continue;
        }

        return Some(Griff {
            id: format!("aus_text_{name}"),
            name: name.to_string(),
            argumente: argumente_lesen(
                wert.get("arguments").or_else(|| wert.get("parameters")),
            ),
        });
    }

    None
}

/// Findet das Ende des JSON-Objekts, das bei `anfang` beginnt.
///
/// Zaehlt Klammern und ueberspringt dabei Zeichenketten — eine Klammer in
/// einem Wert darf die Zaehlung nicht verschieben.
fn klammer_ende(text: &str, anfang: usize) -> Option<usize> {
    let mut tiefe = 0_i32;
    let mut in_zeichenkette = false;
    let mut maskiert = false;

    for (i, z) in text[anfang..].char_indices() {
        if maskiert {
            maskiert = false;
            continue;
        }

        match z {
            '\\' if in_zeichenkette => maskiert = true,
            '"' => in_zeichenkette = !in_zeichenkette,
            '{' if !in_zeichenkette => tiefe += 1,
            '}' if !in_zeichenkette => {
                tiefe -= 1;

                if tiefe == 0 {
                    return Some(anfang + i + z.len_utf8());
                }
            }
            _ => {}
        }
    }

    None
}

/// Baut die Werkzeugliste in der Form des Anbieters.
pub fn werkzeuge_fuer(form: Form, werkzeuge: &[serde_json::Value]) -> Vec<serde_json::Value> {
    werkzeuge
        .iter()
        .filter_map(|w| {
            let name = w.get("name")?.as_str()?;
            let beschreibung = w
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("Keine Beschreibung.");
            let schema = w
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({ "type": "object", "properties": {} }));

            Some(match form {
                Form::OpenAi => serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": beschreibung,
                        "parameters": schema,
                    },
                }),
                Form::Anthropic => serde_json::json!({
                    "name": name,
                    "description": beschreibung,
                    "input_schema": schema,
                }),
            })
        })
        .collect()
}

/// Wandelt den Gespraechsverlauf in die Form des Anbieters.
pub fn verlauf_fuer(form: Form, verlauf: &[Nachricht]) -> Vec<serde_json::Value> {
    match form {
        Form::OpenAi => verlauf_openai(verlauf),
        Form::Anthropic => verlauf_anthropic(verlauf),
    }
}

fn verlauf_openai(verlauf: &[Nachricht]) -> Vec<serde_json::Value> {
    verlauf
        .iter()
        .map(|n| match n {
            Nachricht::Nutzer(text) => serde_json::json!({ "role": "user", "content": text }),
            Nachricht::Modell { text, griffe } => {
                let mut eintrag = serde_json::json!({
                    "role": "assistant",
                    "content": text.clone().unwrap_or_default(),
                });

                if !griffe.is_empty() {
                    eintrag["tool_calls"] = serde_json::json!(griffe
                        .iter()
                        .map(|g| serde_json::json!({
                            "id": g.id,
                            "type": "function",
                            "function": {
                                "name": g.name,
                                // OpenAI erwartet die Argumente als Zeichenkette,
                                // nicht als Objekt.
                                "arguments": g.argumente.to_string(),
                            },
                        }))
                        .collect::<Vec<_>>());
                }

                eintrag
            }
            Nachricht::Ergebnis { id, inhalt, .. } => serde_json::json!({
                "role": "tool",
                "tool_call_id": id,
                "content": inhalt,
            }),
        })
        .collect()
}

fn verlauf_anthropic(verlauf: &[Nachricht]) -> Vec<serde_json::Value> {
    let mut ausgabe: Vec<serde_json::Value> = Vec::new();

    for n in verlauf {
        match n {
            Nachricht::Nutzer(text) => ausgabe.push(serde_json::json!({
                "role": "user",
                "content": [{ "type": "text", "text": text }],
            })),
            Nachricht::Modell { text, griffe } => {
                let mut inhalt: Vec<serde_json::Value> = Vec::new();

                if let Some(t) = text.as_deref().filter(|t| !t.trim().is_empty()) {
                    inhalt.push(serde_json::json!({ "type": "text", "text": t }));
                }

                for g in griffe {
                    inhalt.push(serde_json::json!({
                        "type": "tool_use",
                        "id": g.id,
                        "name": g.name,
                        "input": g.argumente,
                    }));
                }

                ausgabe.push(serde_json::json!({ "role": "assistant", "content": inhalt }));
            }
            Nachricht::Ergebnis { id, inhalt, fehler } => {
                // Anthropic erwartet Werkzeugergebnisse als **Nutzer**-Nachricht.
                // Mehrere Ergebnisse hintereinander gehoeren in dieselbe
                // Nachricht, sonst wechseln die Rollen unzulaessig.
                let block = serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": id,
                    "content": inhalt,
                    "is_error": fehler,
                });

                match ausgabe.last_mut() {
                    Some(letzte)
                        if letzte.get("role").and_then(|r| r.as_str()) == Some("user")
                            && letzte
                                .get("content")
                                .and_then(|c| c.as_array())
                                .is_some_and(|c| {
                                    c.iter().all(|b| {
                                        b.get("type").and_then(|t| t.as_str())
                                            == Some("tool_result")
                                    })
                                }) =>
                    {
                        if let Some(felder) =
                            letzte.get_mut("content").and_then(|c| c.as_array_mut())
                        {
                            felder.push(block);
                        }
                    }
                    _ => ausgabe.push(serde_json::json!({
                        "role": "user",
                        "content": [block],
                    })),
                }
            }
        }
    }

    ausgabe
}

/// Liest die Antwort des Anbieters.
pub fn antwort_lesen(form: Form, roh: &serde_json::Value) -> Result<Antwort, String> {
    if let Some(fehler) = roh.get("error") {
        let grund = fehler
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("kein Grund genannt");

        return Err(format!("Der Anbieter lehnt ab: {grund}"));
    }

    match form {
        Form::OpenAi => antwort_openai(roh),
        Form::Anthropic => antwort_anthropic(roh),
    }
}

fn antwort_openai(roh: &serde_json::Value) -> Result<Antwort, String> {
    let nachricht = roh
        .pointer("/choices/0/message")
        .ok_or_else(|| "Antwort hat keine Nachricht (/choices/0/message fehlt).".to_string())?;

    let text = nachricht
        .get("content")
        .and_then(|c| c.as_str())
        .map(str::to_string)
        .filter(|t| !t.trim().is_empty());

    let griffe = nachricht
        .get("tool_calls")
        .and_then(|g| g.as_array())
        .map(|liste| {
            liste
                .iter()
                .enumerate()
                .filter_map(|(nr, g)| {
                    let name = g.pointer("/function/name")?.as_str()?.to_string();

                    Some(Griff {
                        id: g
                            .get("id")
                            .and_then(|i| i.as_str())
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("griff_{nr}")),
                        name,
                        argumente: argumente_lesen(g.pointer("/function/arguments")),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Antwort {
        text,
        griffe,
        marken: marken(roh, "/usage/prompt_tokens", "/usage/completion_tokens"),
    })
}

fn antwort_anthropic(roh: &serde_json::Value) -> Result<Antwort, String> {
    let inhalt = roh
        .get("content")
        .and_then(|c| c.as_array())
        .ok_or_else(|| "Antwort hat keinen Inhalt (content fehlt).".to_string())?;

    let mut texte: Vec<String> = Vec::new();
    let mut griffe: Vec<Griff> = Vec::new();

    for block in inhalt {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                    texte.push(t.to_string());
                }
            }
            Some("tool_use") => {
                if let (Some(id), Some(name)) = (
                    block.get("id").and_then(|i| i.as_str()),
                    block.get("name").and_then(|n| n.as_str()),
                ) {
                    griffe.push(Griff {
                        id: id.to_string(),
                        name: name.to_string(),
                        argumente: block
                            .get("input")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!({})),
                    });
                }
            }
            _ => {}
        }
    }

    let gesamt = texte.join("\n");

    Ok(Antwort {
        text: Some(gesamt).filter(|t| !t.trim().is_empty()),
        griffe,
        marken: marken(roh, "/usage/input_tokens", "/usage/output_tokens"),
    })
}

fn marken(roh: &serde_json::Value, ein: &str, aus: &str) -> Option<(u64, u64)> {
    Some((
        roh.pointer(ein)?.as_u64()?,
        roh.pointer(aus)?.as_u64().unwrap_or(0),
    ))
}

/// Die Adresse, an die der Zug geht.
pub fn adresse(anbieter: &Anbieter) -> String {
    match anbieter.form {
        Form::OpenAi => format!("{}/chat/completions", anbieter.basis),
        Form::Anthropic => format!("{}/v1/messages", anbieter.basis),
    }
}

/// Die Nutzlast fuer einen Zug.
pub fn nutzlast(
    anbieter: &Anbieter,
    verlauf: &[Nachricht],
    werkzeuge: &[serde_json::Value],
    auftrag: &str,
) -> serde_json::Value {
    let nachrichten = verlauf_fuer(anbieter.form, verlauf);
    let angebot = werkzeuge_fuer(anbieter.form, werkzeuge);

    match anbieter.form {
        Form::OpenAi => {
            // Bei OpenAI gehoert der Auftrag als erste Nachricht in den
            // Verlauf, nicht in ein eigenes Feld.
            let mut alle = vec![serde_json::json!({ "role": "system", "content": auftrag })];
            alle.extend(nachrichten);

            let mut nutzlast = serde_json::json!({
                "model": anbieter.modell,
                "messages": alle,
            });

            if !angebot.is_empty() {
                nutzlast["tools"] = serde_json::json!(angebot);
            }

            nutzlast
        }
        Form::Anthropic => {
            let mut nutzlast = serde_json::json!({
                "model": anbieter.modell,
                "max_tokens": 8192,
                "system": auftrag,
                "messages": nachrichten,
            });

            if !angebot.is_empty() {
                nutzlast["tools"] = serde_json::json!(angebot);
            }

            nutzlast
        }
    }
}

/// Die Kopfzeilen, die der Anbieter braucht.
pub fn kopfzeilen(anbieter: &Anbieter) -> Result<BTreeMap<String, String>, String> {
    let mut kopf = BTreeMap::new();
    kopf.insert("Content-Type".to_string(), "application/json".to_string());

    if let Some(schluessel) = anbieter.schluessel()? {
        match anbieter.form {
            Form::OpenAi => {
                kopf.insert("Authorization".to_string(), format!("Bearer {schluessel}"));
            }
            Form::Anthropic => {
                kopf.insert("x-api-key".to_string(), schluessel);
                kopf.insert("anthropic-version".to_string(), "2023-06-01".to_string());
            }
        }
    }

    Ok(kopf)
}

// ---------------------------------------------------------------------------
// Ablage
// ---------------------------------------------------------------------------

fn ablage_pfad() -> Result<std::path::PathBuf, String> {
    Ok(crate::config::config_dir()?.join("anbieter.json"))
}

#[derive(Debug, Default)]
pub struct Ablage {
    pub anbieter: Vec<Anbieter>,
    pub standard: Option<String>,
}

impl Ablage {
    pub fn finden(&self, name: &str) -> Option<&Anbieter> {
        self.anbieter.iter().find(|a| a.name == name)
    }

    /// Der Anbieter fuer diesen Lauf — benannt oder der Standard.
    pub fn waehlen(&self, wunsch: Option<&str>) -> Result<&Anbieter, String> {
        match wunsch {
            Some(name) => self.finden(name).ok_or_else(|| {
                format!(
                    "Anbieter '{name}' ist nicht eingetragen. Eingetragen: {}",
                    self.namen()
                )
            }),
            None => {
                let standard = self.standard.as_deref().ok_or_else(|| {
                    "Kein Standardanbieter gesetzt. Mit 'schleife --standard <name>' setzen."
                        .to_string()
                })?;

                self.finden(standard).ok_or_else(|| {
                    format!(
                        "Der Standardanbieter '{standard}' ist nicht eingetragen. Eingetragen: {}",
                        self.namen()
                    )
                })
            }
        }
    }

    fn namen(&self) -> String {
        if self.anbieter.is_empty() {
            return "keiner".to_string();
        }

        self.anbieter
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub fn laden() -> Result<Ablage, String> {
    let pfad = ablage_pfad()?;

    if !pfad.exists() {
        return Ok(Ablage::default());
    }

    let text = std::fs::read_to_string(&pfad)
        .map_err(|e| format!("{} ist nicht lesbar: {e}", pfad.display()))?;

    let roh: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("{} ist kein JSON: {e}", pfad.display()))?;

    let mut anbieter = Vec::new();

    if let Some(eintraege) = roh.get("anbieter").and_then(|a| a.as_object()) {
        for (name, wert) in eintraege {
            anbieter.push(Anbieter::lesen(name, wert)?);
        }
    }

    Ok(Ablage {
        anbieter,
        standard: roh
            .get("standard")
            .and_then(|s| s.as_str())
            .map(str::to_string),
    })
}

pub fn sichern(ablage: &Ablage) -> Result<std::path::PathBuf, String> {
    let pfad = ablage_pfad()?;

    if let Some(ordner) = pfad.parent() {
        std::fs::create_dir_all(ordner)
            .map_err(|e| format!("{} ist nicht anlegbar: {e}", ordner.display()))?;
    }

    let mut eintraege = serde_json::Map::new();

    for a in &ablage.anbieter {
        eintraege.insert(a.name.clone(), a.schreiben());
    }

    let mut roh = serde_json::json!({ "anbieter": eintraege });

    if let Some(standard) = &ablage.standard {
        roh["standard"] = serde_json::json!(standard);
    }

    let text = serde_json::to_string_pretty(&roh)
        .map_err(|e| format!("Anbieter sind nicht schreibbar: {e}"))?;

    std::fs::write(&pfad, format!("{text}\n"))
        .map_err(|e| format!("{} ist nicht schreibbar: {e}", pfad.display()))?;

    Ok(pfad)
}

pub fn befund_merken(name: &str, faehig: bool, kontext: Option<u64>) -> Result<(), String> {
    let mut ablage = laden()?;

    let eintrag = ablage
        .anbieter
        .iter_mut()
        .find(|a| a.name == name)
        .ok_or_else(|| format!("Anbieter '{name}' ist nicht eingetragen."))?;

    eintrag.werkzeuge = if faehig {
        Werkzeugfaehig::Ja
    } else {
        Werkzeugfaehig::Nein
    };

    // Ein nicht gemessenes Kontextfenster loescht das zuletzt gemessene
    // **nicht**. Sonst waere ein fehlgeschlagener Messlauf schlimmer als
    // keiner.
    if kontext.is_some() {
        eintrag.kontext = kontext;
    }

    sichern(&ablage)?;

    Ok(())
}

/// Schaetzt, wie viele Marken eine Nutzlast belegt.
///
/// Grobe Regel: vier Zeichen je Marke. Das ist ungenau und genuegt — es geht
/// nicht um eine Rechnung, sondern um die Frage „passt das ueberhaupt
/// annaehernd". Ein Katalog, der das Fenster um das Dreifache sprengt, wird
/// von jeder Schaetzung erkannt.
pub fn marken_schaetzen(nutzlast: &serde_json::Value) -> u64 {
    (nutzlast.to_string().len() / 4) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Argumente in beiden Formen ------------------------------------------

    #[test]
    fn argumente_als_zeichenkette_werden_gelesen() {
        // So liefert Ollamas OpenAI-kompatibler Weg.
        let wert = serde_json::json!("{\"pfad\":\"/tmp\"}");
        assert_eq!(
            argumente_lesen(Some(&wert)),
            serde_json::json!({ "pfad": "/tmp" })
        );
    }

    #[test]
    fn argumente_als_objekt_werden_gelesen() {
        // So liefert Ollamas nativer Weg — gemessen am 15.10.2026.
        let wert = serde_json::json!({ "pfad": "/tmp" });
        assert_eq!(argumente_lesen(Some(&wert)), wert);
    }

    #[test]
    fn kaputte_argumente_ergeben_leeres_objekt() {
        let wert = serde_json::json!("{das ist kein json");
        assert_eq!(argumente_lesen(Some(&wert)), serde_json::json!({}));
    }

    // -- Griff aus Text ------------------------------------------------------

    /// Genau die Antwort, die `qwen2.5:14b` am 15.10.2026 geliefert hat —
    /// Stoerzeichen, Griff und durchgesickerte schliessende Marke.
    const ECHTE_KAPUTTE_ANTWORT: &str =
        "ичество\n{\"name\": \"probe_laufzeit\", \"arguments\": {}}\n</tool_call>";

    #[test]
    fn griff_wird_aus_der_echten_kaputten_antwort_gerettet() {
        let griff = griff_aus_text(ECHTE_KAPUTTE_ANTWORT, &["probe_laufzeit"])
            .expect("der Griff steht im Text und muss gefunden werden");

        assert_eq!(griff.name, "probe_laufzeit");
        assert_eq!(griff.argumente, serde_json::json!({}));
    }

    #[test]
    fn unbekannter_name_wird_nicht_zum_griff() {
        // Der Riegel gegen Phantomgriffe: ein Modell, das ueber JSON spricht,
        // darf nichts ausloesen.
        assert!(griff_aus_text(
            "Du koenntest {\"name\": \"alles-loeschen\", \"arguments\": {}} schicken.",
            &["rechner-last"],
        )
        .is_none());
    }

    #[test]
    fn klammer_in_zeichenkette_verschiebt_die_zaehlung_nicht() {
        // Die Klammer im Wert muss **unpaarig** sein, sonst prueft der Test
        // nichts: bei "echo }{" heben sich die beiden gegenseitig auf, und
        // eine Zaehlung ohne Zeichenkettenschutz kommt zufaellig richtig
        // heraus. Mit der Mutationsprobe am 15.10.2026 aufgefallen — der erste
        // Entwurf dieses Tests blieb gruen, als ich den Schutz entfernte.
        let griff = griff_aus_text(
            "{\"name\": \"rechner-befehl\", \"arguments\": {\"befehl\": \"echo }\"}}",
            &["rechner-befehl"],
        )
        .expect("eine Klammer im Wert darf das Ende nicht verschieben");

        assert_eq!(griff.argumente, serde_json::json!({ "befehl": "echo }" }));
    }

    #[test]
    fn text_ohne_griff_bleibt_ohne_griff() {
        assert!(griff_aus_text("Der Rechner laeuft seit drei Wochen.", &["rechner-last"]).is_none());
    }

    #[test]
    fn erstes_objekt_ohne_namen_blockiert_das_zweite_nicht() {
        let griff = griff_aus_text(
            "{\"unfug\": 1} dann {\"name\": \"rechner-last\", \"arguments\": {}}",
            &["rechner-last"],
        )
        .expect("ein Objekt ohne Namen darf die Suche nicht beenden");

        assert_eq!(griff.name, "rechner-last");
    }

    // -- Werkzeugformen ------------------------------------------------------

    fn ein_werkzeug() -> serde_json::Value {
        serde_json::json!({
            "name": "rechner-last",
            "description": "Laufzeit und Last.",
            "inputSchema": { "type": "object", "properties": {} },
        })
    }

    #[test]
    fn openai_form_klammert_in_function() {
        let gebaut = werkzeuge_fuer(Form::OpenAi, &[ein_werkzeug()]);
        assert_eq!(gebaut[0]["type"], "function");
        assert_eq!(gebaut[0]["function"]["name"], "rechner-last");
        assert!(gebaut[0]["function"]["parameters"].is_object());
    }

    #[test]
    fn anthropic_form_nutzt_input_schema() {
        let gebaut = werkzeuge_fuer(Form::Anthropic, &[ein_werkzeug()]);
        assert_eq!(gebaut[0]["name"], "rechner-last");
        assert!(gebaut[0]["input_schema"].is_object());
        assert!(gebaut[0].get("function").is_none());
    }

    // -- Verlauf -------------------------------------------------------------

    fn verlauf_mit_zwei_ergebnissen() -> Vec<Nachricht> {
        vec![
            Nachricht::Nutzer("Wie geht es dem Rechner?".to_string()),
            Nachricht::Modell {
                text: None,
                griffe: vec![
                    Griff {
                        id: "a".to_string(),
                        name: "rechner-last".to_string(),
                        argumente: serde_json::json!({}),
                    },
                    Griff {
                        id: "b".to_string(),
                        name: "rechner-platte".to_string(),
                        argumente: serde_json::json!({}),
                    },
                ],
            },
            Nachricht::Ergebnis {
                id: "a".to_string(),
                inhalt: "up 3 weeks".to_string(),
                fehler: false,
            },
            Nachricht::Ergebnis {
                id: "b".to_string(),
                inhalt: "42% belegt".to_string(),
                fehler: false,
            },
        ]
    }

    #[test]
    fn openai_argumente_gehen_als_zeichenkette_raus() {
        let gebaut = verlauf_fuer(Form::OpenAi, &verlauf_mit_zwei_ergebnissen());

        // OpenAI erwartet eine Zeichenkette. Ein Objekt wird mit 400 abgelehnt.
        assert!(gebaut[1]["tool_calls"][0]["function"]["arguments"].is_string());
        assert_eq!(gebaut[2]["role"], "tool");
        assert_eq!(gebaut[2]["tool_call_id"], "a");
    }

    #[test]
    fn anthropic_fasst_mehrere_ergebnisse_in_eine_nachricht() {
        let gebaut = verlauf_fuer(Form::Anthropic, &verlauf_mit_zwei_ergebnissen());

        // Nutzer, Modell, EIN Ergebnisblock — nicht zwei. Zwei
        // Nutzernachrichten hintereinander lehnt Anthropic ab.
        assert_eq!(gebaut.len(), 3, "zwei Ergebnisse gehoeren in eine Nachricht");
        assert_eq!(gebaut[2]["role"], "user");
        assert_eq!(
            gebaut[2]["content"].as_array().map(Vec::len),
            Some(2),
            "beide Ergebnisse muessen in derselben Nachricht stehen"
        );
        assert_eq!(gebaut[2]["content"][0]["type"], "tool_result");
    }

    #[test]
    fn anthropic_argumente_bleiben_objekt() {
        let verlauf = vec![Nachricht::Modell {
            text: None,
            griffe: vec![Griff {
                id: "a".to_string(),
                name: "rechner-befehl".to_string(),
                argumente: serde_json::json!({ "befehl": "uptime" }),
            }],
        }];

        let gebaut = verlauf_fuer(Form::Anthropic, &verlauf);
        assert!(gebaut[0]["content"][0]["input"].is_object());
    }

    #[test]
    fn nachricht_nach_ergebnis_startet_eine_neue_nachricht() {
        let mut verlauf = verlauf_mit_zwei_ergebnissen();
        verlauf.push(Nachricht::Nutzer("Und die Platte?".to_string()));

        let gebaut = verlauf_fuer(Form::Anthropic, &verlauf);

        // Die Nutzerfrage darf NICHT in den Ergebnisblock rutschen.
        assert_eq!(gebaut.len(), 4);
        assert_eq!(gebaut[3]["content"][0]["type"], "text");
    }

    // -- Antwort lesen -------------------------------------------------------

    #[test]
    fn openai_antwort_wird_gelesen() {
        let roh = serde_json::json!({
            "choices": [{ "message": {
                "content": null,
                "tool_calls": [{
                    "id": "call_1",
                    "function": { "name": "rechner-last", "arguments": "{}" },
                }],
            }}],
            "usage": { "prompt_tokens": 100, "completion_tokens": 20 },
        });

        let antwort = antwort_lesen(Form::OpenAi, &roh).expect("lesbar");
        assert_eq!(antwort.griffe.len(), 1);
        assert_eq!(antwort.griffe[0].name, "rechner-last");
        assert_eq!(antwort.marken, Some((100, 20)));
    }

    #[test]
    fn openai_antwort_ohne_id_bekommt_eine() {
        // Gemessen: Ollamas nativer Weg liefert `index` statt `id`.
        let roh = serde_json::json!({
            "choices": [{ "message": {
                "tool_calls": [{ "function": { "name": "rechner-last", "arguments": {} } }],
            }}],
        });

        let antwort = antwort_lesen(Form::OpenAi, &roh).expect("lesbar");
        assert!(
            !antwort.griffe[0].id.is_empty(),
            "ohne Kennung kann das Ergebnis nicht zugeordnet werden"
        );
    }

    #[test]
    fn anthropic_antwort_wird_gelesen() {
        let roh = serde_json::json!({
            "content": [
                { "type": "text", "text": "Ich sehe nach." },
                { "type": "tool_use", "id": "toolu_1", "name": "rechner-last", "input": {} },
            ],
            "usage": { "input_tokens": 50, "output_tokens": 10 },
        });

        let antwort = antwort_lesen(Form::Anthropic, &roh).expect("lesbar");
        assert_eq!(antwort.text.as_deref(), Some("Ich sehe nach."));
        assert_eq!(antwort.griffe[0].id, "toolu_1");
        assert_eq!(antwort.marken, Some((50, 10)));
    }

    #[test]
    fn fehler_des_anbieters_wird_fehler() {
        let roh = serde_json::json!({ "error": { "message": "model not found" } });
        let fehler = antwort_lesen(Form::OpenAi, &roh).expect_err("muss scheitern");
        assert!(fehler.contains("model not found"), "{fehler}");
    }

    // -- Faehigkeit ----------------------------------------------------------

    #[test]
    fn fehlende_angabe_ist_ungemessen_nicht_unfaehig() {
        // Der Unterschied ist der ganze Befund: `Ungemessen` fordert eine
        // Messung, `Nein` behauptet ein Ergebnis.
        assert_eq!(Werkzeugfaehig::lesen(None), Werkzeugfaehig::Ungemessen);
        assert_eq!(Werkzeugfaehig::lesen(Some(false)), Werkzeugfaehig::Nein);
        assert_eq!(Werkzeugfaehig::lesen(Some(true)), Werkzeugfaehig::Ja);
    }

    #[test]
    fn ungemessen_wird_nicht_in_die_ablage_geschrieben() {
        let a = Anbieter {
            name: "x".to_string(),
            form: Form::OpenAi,
            basis: "http://x".to_string(),
            modell: "m".to_string(),
            schluessel_variable: None,
            werkzeuge: Werkzeugfaehig::Ungemessen,
            kontext: None,
        };

        assert!(
            a.schreiben().get("werkzeuge").is_none(),
            "ungemessen heisst: kein Eintrag, nicht 'false'"
        );
    }

    #[test]
    fn form_lehnt_unbekanntes_ab() {
        assert!(Form::lesen("ollama").is_err());
        assert_eq!(Form::lesen("openai").unwrap(), Form::OpenAi);
    }

    #[test]
    fn fehlender_schluessel_nennt_die_variable() {
        let a = Anbieter {
            name: "openrouter".to_string(),
            form: Form::OpenAi,
            basis: "http://x".to_string(),
            modell: "m".to_string(),
            schluessel_variable: Some("GIBT_ES_NICHT_12345".to_string()),
            werkzeuge: Werkzeugfaehig::Ja,
            kontext: None,
        };

        let fehler = a.schluessel().expect_err("muss scheitern");
        assert!(fehler.contains("GIBT_ES_NICHT_12345"), "{fehler}");
    }

    #[test]
    fn anthropic_kopfzeilen_nutzen_x_api_key() {
        std::env::set_var("TEST_SCHLUESSEL_ANBIETER", "sk-test");

        let a = Anbieter {
            name: "anthropic".to_string(),
            form: Form::Anthropic,
            basis: "https://api.anthropic.com".to_string(),
            modell: "claude-opus-5".to_string(),
            schluessel_variable: Some("TEST_SCHLUESSEL_ANBIETER".to_string()),
            werkzeuge: Werkzeugfaehig::Ja,
            kontext: None,
        };

        let kopf = kopfzeilen(&a).expect("Schluessel ist gesetzt");
        assert_eq!(kopf.get("x-api-key").map(String::as_str), Some("sk-test"));
        assert!(
            kopf.contains_key("anthropic-version"),
            "ohne Version lehnt Anthropic ab"
        );
        assert!(!kopf.contains_key("Authorization"));

        std::env::remove_var("TEST_SCHLUESSEL_ANBIETER");
    }

    #[test]
    fn adressen_unterscheiden_sich_je_form() {
        let bau = |form| Anbieter {
            name: "x".to_string(),
            form,
            basis: "https://h".to_string(),
            modell: "m".to_string(),
            schluessel_variable: None,
            werkzeuge: Werkzeugfaehig::Ja,
            kontext: None,
        };

        assert_eq!(adresse(&bau(Form::OpenAi)), "https://h/chat/completions");
        assert_eq!(adresse(&bau(Form::Anthropic)), "https://h/v1/messages");
    }

    #[test]
    fn openai_nutzlast_setzt_den_auftrag_als_erste_nachricht() {
        let a = Anbieter {
            name: "x".to_string(),
            form: Form::OpenAi,
            basis: "https://h".to_string(),
            modell: "m".to_string(),
            schluessel_variable: None,
            werkzeuge: Werkzeugfaehig::Ja,
            kontext: None,
        };

        let gebaut = nutzlast(&a, &[Nachricht::Nutzer("hallo".to_string())], &[], "sei knapp");
        assert_eq!(gebaut["messages"][0]["role"], "system");
        assert_eq!(gebaut["messages"][0]["content"], "sei knapp");
        assert!(
            gebaut.get("system").is_none(),
            "OpenAI kennt kein eigenes system-Feld"
        );
    }

    #[test]
    fn anthropic_nutzlast_setzt_den_auftrag_ins_eigene_feld() {
        let a = Anbieter {
            name: "x".to_string(),
            form: Form::Anthropic,
            basis: "https://h".to_string(),
            modell: "m".to_string(),
            schluessel_variable: None,
            werkzeuge: Werkzeugfaehig::Ja,
            kontext: None,
        };

        let gebaut = nutzlast(&a, &[Nachricht::Nutzer("hallo".to_string())], &[], "sei knapp");
        assert_eq!(gebaut["system"], "sei knapp");
        assert_eq!(gebaut["messages"][0]["role"], "user");
        assert!(gebaut["max_tokens"].is_number(), "Anthropic verlangt max_tokens");
    }

    #[test]
    fn leere_werkzeugliste_ergibt_kein_tools_feld() {
        let a = Anbieter {
            name: "x".to_string(),
            form: Form::OpenAi,
            basis: "https://h".to_string(),
            modell: "m".to_string(),
            schluessel_variable: None,
            werkzeuge: Werkzeugfaehig::Ja,
            kontext: None,
        };

        let gebaut = nutzlast(&a, &[], &[], "x");
        assert!(
            gebaut.get("tools").is_none(),
            "ein leeres tools-Feld lehnen manche Anbieter ab"
        );
    }
}
