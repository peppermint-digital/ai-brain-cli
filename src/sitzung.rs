//! Der eigene Sitzungsspeicher.
//!
//! **Wofuer.** Ein Lauf des eigenen Loops endet, wenn das Modell fertig ist.
//! Ohne Ablage faengt der naechste bei null an — jede Nachfrage muesste den
//! ganzen Zusammenhang neu mitbringen. `claude` loest das mit `--resume` und
//! einer eigenen Ablage; dieser Loop braucht seine eigene, weil er die von
//! `claude` nicht lesen darf und nicht lesen soll.
//!
//! **Was hier NICHT passiert: Kuerzen.** Waechst eine Sitzung ueber das
//! Kontextfenster, wird sie nicht heimlich beschnitten — der Riegel in
//! `schleife::fahren` haelt davor an und sagt es. Stilles Kuerzen ist genau der
//! Fehler, der bei Ollamas 4096-Marken-Deckel so teuer war: das Modell sieht
//! die Haelfte und niemand erfaehrt davon.

use crate::anbieter::{Griff, Nachricht};

fn jetzt() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn ordner() -> Result<std::path::PathBuf, String> {
    let pfad = crate::config::config_dir()?.join("sitzungen");

    std::fs::create_dir_all(&pfad)
        .map_err(|e| format!("{} ist nicht anlegbar: {e}", pfad.display()))?;

    Ok(pfad)
}

/// Eine gespeicherte Sitzung.
pub struct Sitzung {
    pub kennung: String,
    pub anbieter: String,
    pub angelegt: u64,
    pub beruehrt: u64,
    pub verlauf: Vec<Nachricht>,
}

impl Sitzung {
    pub fn neu(anbieter: &str) -> Self {
        let jetzt = jetzt();

        Sitzung {
            // Die Kennung ist der Zeitstempel. Kurz, sortierbar, und ohne
            // Zufallsquelle — die braeuchte eine Abhaengigkeit fuer nichts.
            kennung: format!("{jetzt}"),
            anbieter: anbieter.to_string(),
            angelegt: jetzt,
            beruehrt: jetzt,
            verlauf: Vec::new(),
        }
    }

    /// Die erste Frage — als Titel in der Liste.
    pub fn titel(&self) -> String {
        self.verlauf
            .iter()
            .find_map(|n| match n {
                Nachricht::Nutzer(text) => Some(text.clone()),
                _ => None,
            })
            .map(|t| {
                let einzeilig = t.replace('\n', " ");

                if einzeilig.chars().count() <= 60 {
                    einzeilig
                } else {
                    einzeilig.chars().take(60).collect::<String>() + "…"
                }
            })
            .unwrap_or_else(|| "(ohne Frage)".to_string())
    }

    pub fn zuege(&self) -> usize {
        self.verlauf
            .iter()
            .filter(|n| matches!(n, Nachricht::Nutzer(_)))
            .count()
    }
}

fn nachricht_zu_json(n: &Nachricht) -> serde_json::Value {
    match n {
        Nachricht::Nutzer(text) => serde_json::json!({ "rolle": "nutzer", "text": text }),
        Nachricht::Modell { text, griffe } => serde_json::json!({
            "rolle": "modell",
            "text": text,
            "griffe": griffe
                .iter()
                .map(|g| serde_json::json!({
                    "id": g.id,
                    "name": g.name,
                    "argumente": g.argumente,
                }))
                .collect::<Vec<_>>(),
        }),
        Nachricht::Ergebnis { id, inhalt, fehler } => serde_json::json!({
            "rolle": "ergebnis",
            "id": id,
            "inhalt": inhalt,
            "fehler": fehler,
        }),
    }
}

fn nachricht_aus_json(wert: &serde_json::Value) -> Option<Nachricht> {
    match wert.get("rolle").and_then(|r| r.as_str())? {
        "nutzer" => Some(Nachricht::Nutzer(
            wert.get("text")?.as_str()?.to_string(),
        )),
        "modell" => Some(Nachricht::Modell {
            text: wert.get("text").and_then(|t| t.as_str()).map(str::to_string),
            griffe: wert
                .get("griffe")
                .and_then(|g| g.as_array())
                .map(|liste| {
                    liste
                        .iter()
                        .filter_map(|g| {
                            Some(Griff {
                                id: g.get("id")?.as_str()?.to_string(),
                                name: g.get("name")?.as_str()?.to_string(),
                                argumente: g
                                    .get("argumente")
                                    .cloned()
                                    .unwrap_or_else(|| serde_json::json!({})),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }),
        "ergebnis" => Some(Nachricht::Ergebnis {
            id: wert.get("id")?.as_str()?.to_string(),
            inhalt: wert.get("inhalt")?.as_str()?.to_string(),
            fehler: wert
                .get("fehler")
                .and_then(|f| f.as_bool())
                .unwrap_or(false),
        }),
        // Eine unbekannte Rolle wird **uebersprungen**, nicht geraten. Ein
        // falsch wiederhergestellter Verlauf waere schlimmer als ein kuerzerer:
        // er sieht vollstaendig aus.
        _ => None,
    }
}

pub fn sichern(sitzung: &Sitzung) -> Result<std::path::PathBuf, String> {
    let pfad = ordner()?.join(format!("{}.json", sitzung.kennung));

    let roh = serde_json::json!({
        "kennung": sitzung.kennung,
        "anbieter": sitzung.anbieter,
        "angelegt": sitzung.angelegt,
        "beruehrt": jetzt(),
        "verlauf": sitzung.verlauf.iter().map(nachricht_zu_json).collect::<Vec<_>>(),
    });

    let text = serde_json::to_string_pretty(&roh)
        .map_err(|e| format!("Sitzung ist nicht schreibbar: {e}"))?;

    std::fs::write(&pfad, format!("{text}\n"))
        .map_err(|e| format!("{} ist nicht schreibbar: {e}", pfad.display()))?;

    Ok(pfad)
}

pub fn laden(kennung: &str) -> Result<Sitzung, String> {
    // Keine Pfadtrenner in der Kennung: sonst zeigt `--fortsetzen ../../etwas`
    // aus dem Ordner heraus.
    if kennung.contains('/') || kennung.contains('\\') || kennung.contains("..") {
        return Err(format!("'{kennung}' ist keine Sitzungskennung."));
    }

    let pfad = ordner()?.join(format!("{kennung}.json"));

    if !pfad.exists() {
        return Err(format!(
            "Sitzung '{kennung}' gibt es nicht. Mit 'schleife --sitzungen' nachsehen."
        ));
    }

    let text = std::fs::read_to_string(&pfad)
        .map_err(|e| format!("{} ist nicht lesbar: {e}", pfad.display()))?;

    let roh: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("{} ist kein JSON: {e}", pfad.display()))?;

    Ok(Sitzung {
        kennung: kennung.to_string(),
        anbieter: roh
            .get("anbieter")
            .and_then(|a| a.as_str())
            .unwrap_or_default()
            .to_string(),
        angelegt: roh.get("angelegt").and_then(|a| a.as_u64()).unwrap_or(0),
        beruehrt: roh.get("beruehrt").and_then(|a| a.as_u64()).unwrap_or(0),
        verlauf: roh
            .get("verlauf")
            .and_then(|v| v.as_array())
            .map(|liste| liste.iter().filter_map(nachricht_aus_json).collect())
            .unwrap_or_default(),
    })
}

/// Alle Sitzungen, die neueste zuerst.
pub fn alle() -> Result<Vec<Sitzung>, String> {
    let mut gefunden = Vec::new();

    let eintraege = std::fs::read_dir(ordner()?)
        .map_err(|e| format!("Sitzungen sind nicht auflistbar: {e}"))?;

    for eintrag in eintraege.flatten() {
        let pfad = eintrag.path();

        if pfad.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let Some(kennung) = pfad.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };

        // Eine unlesbare Sitzung darf die Liste nicht zum Scheitern bringen —
        // sonst macht eine kaputte Datei alle anderen unerreichbar.
        if let Ok(sitzung) = laden(kennung) {
            gefunden.push(sitzung);
        }
    }

    // Neueste zuerst: absteigend, darum das Minus statt sort_by_key.
    gefunden.sort_by_key(|s| std::cmp::Reverse(s.beruehrt));

    Ok(gefunden)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verlauf_uebersteht_den_umweg_ueber_json() {
        let verlauf = [
            Nachricht::Nutzer("Wie geht es dem Rechner?".to_string()),
            Nachricht::Modell {
                text: Some("Ich sehe nach.".to_string()),
                griffe: vec![Griff {
                    id: "a".to_string(),
                    name: "rechner-last".to_string(),
                    argumente: serde_json::json!({ "pfad": "/" }),
                }],
            },
            Nachricht::Ergebnis {
                id: "a".to_string(),
                inhalt: "up 21 days".to_string(),
                fehler: false,
            },
        ];

        let zurueck: Vec<Nachricht> = verlauf
            .iter()
            .map(nachricht_zu_json)
            .filter_map(|w| nachricht_aus_json(&w))
            .collect();

        assert_eq!(zurueck.len(), 3, "keine Nachricht darf verloren gehen");

        match &zurueck[1] {
            Nachricht::Modell { text, griffe } => {
                assert_eq!(text.as_deref(), Some("Ich sehe nach."));
                assert_eq!(griffe[0].name, "rechner-last");
                assert_eq!(griffe[0].argumente, serde_json::json!({ "pfad": "/" }));
            }
            anders => panic!("falsche Rolle wiederhergestellt: {anders:?}"),
        }

        match &zurueck[2] {
            Nachricht::Ergebnis { id, inhalt, fehler } => {
                assert_eq!(id, "a");
                assert_eq!(inhalt, "up 21 days");
                assert!(!fehler);
            }
            anders => panic!("falsche Rolle wiederhergestellt: {anders:?}"),
        }
    }

    #[test]
    fn unbekannte_rolle_wird_uebersprungen_nicht_geraten() {
        let roh = serde_json::json!({ "rolle": "zukunft", "text": "x" });
        assert!(nachricht_aus_json(&roh).is_none());
    }

    #[test]
    fn pfadtrenner_in_der_kennung_wird_abgelehnt() {
        for boese in ["../../etc/passwd", "a/b", "..", "a\\b"] {
            assert!(
                laden(boese).is_err(),
                "'{boese}' darf nicht als Kennung durchgehen"
            );
        }
    }

    #[test]
    fn titel_kommt_aus_der_ersten_frage() {
        let mut s = Sitzung::neu("ollama");
        s.verlauf.push(Nachricht::Modell {
            text: Some("egal".to_string()),
            griffe: Vec::new(),
        });
        s.verlauf
            .push(Nachricht::Nutzer("Wie viele Tasks sind offen?".to_string()));

        assert_eq!(s.titel(), "Wie viele Tasks sind offen?");
    }

    #[test]
    fn langer_titel_wird_auf_zeichen_geschnitten() {
        let mut s = Sitzung::neu("ollama");
        s.verlauf.push(Nachricht::Nutzer("ä".repeat(100)));

        // Nach Bytes geschnitten geriete das in Panik.
        assert_eq!(s.titel().chars().count(), 61);
    }

    #[test]
    fn zuege_zaehlen_nur_nutzerfragen() {
        let mut s = Sitzung::neu("ollama");
        s.verlauf.push(Nachricht::Nutzer("eins".to_string()));
        s.verlauf.push(Nachricht::Modell {
            text: None,
            griffe: Vec::new(),
        });
        s.verlauf.push(Nachricht::Ergebnis {
            id: "a".to_string(),
            inhalt: "x".to_string(),
            fehler: false,
        });
        s.verlauf.push(Nachricht::Nutzer("zwei".to_string()));

        assert_eq!(s.zuege(), 2);
    }

    #[test]
    fn titel_ohne_frage_bleibt_lesbar() {
        assert_eq!(Sitzung::neu("x").titel(), "(ohne Frage)");
    }
}
