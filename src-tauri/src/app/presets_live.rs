//! Preset ghost track (`presets_live`): the 12 templates run forward on the
//! server from a fixed start (8 Oct 2026) by the same simulator as their
//! historical numbers, published at ribqa.com every 6 hours. Read-only here:
//! the app shows the track next to the test verdict and never acts on it.

use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

const URL: &str = "https://ribqa.com/downloads/aleph-edge/presets-live.json";
const CACHE_MS: u64 = 10 * 60_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LivePoint {
    pub d: String,
    pub r: f64,
    #[serde(default)]
    pub dd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LiveTemplate {
    pub id: String,
    pub return_pct: f64,
    pub bots: u32,
    pub cycles: u32,
    pub open_cycles: u32,
    pub worst_dd_pct: Option<f64>,
    pub median_dd_pct: Option<f64>,
    pub win_rate_cycles: Option<f64>,
    pub history: Vec<LivePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PresetsLive {
    pub t0: String,
    pub updated_at: String,
    pub data_end: String,
    pub days: f64,
    pub min_days_for_ranking: f64,
    pub templates: Vec<LiveTemplate>,
}

/// The server file is snake_case; the app speaks camelCase.
#[derive(Deserialize)]
struct Wire {
    t0: String,
    updated_at: String,
    data_end: String,
    days: f64,
    min_days_for_ranking: f64,
    templates: Vec<WireTemplate>,
}

#[derive(Deserialize)]
struct WireTemplate {
    id: String,
    return_pct: f64,
    bots: u32,
    cycles: u32,
    open_cycles: u32,
    worst_dd_pct: Option<f64>,
    median_dd_pct: Option<f64>,
    win_rate_cycles: Option<f64>,
    #[serde(default)]
    history: Vec<LivePoint>,
}

pub fn parse(body: &str) -> Result<PresetsLive, String> {
    let w: Wire = serde_json::from_str(body).map_err(|_| "liveUnavailable".to_string())?;
    Ok(PresetsLive {
        t0: w.t0,
        updated_at: w.updated_at,
        data_end: w.data_end,
        days: w.days,
        min_days_for_ranking: w.min_days_for_ranking,
        templates: w
            .templates
            .into_iter()
            .map(|t| LiveTemplate {
                id: t.id,
                return_pct: t.return_pct,
                bots: t.bots,
                cycles: t.cycles,
                open_cycles: t.open_cycles,
                worst_dd_pct: t.worst_dd_pct,
                median_dd_pct: t.median_dd_pct,
                win_rate_cycles: t.win_rate_cycles,
                history: t.history,
            })
            .collect(),
    })
}

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_default()
    })
}

fn cache() -> &'static Mutex<Option<(u64, PresetsLive)>> {
    static C: OnceLock<Mutex<Option<(u64, PresetsLive)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

#[tauri::command]
pub async fn presets_live() -> Result<PresetsLive, String> {
    let now = crate::bot::engine::now_ms();
    if let Some((at, v)) = cache().lock().expect("presets live cache").clone() {
        if now.saturating_sub(at) < CACHE_MS {
            return Ok(v);
        }
    }
    let body = client()
        .get(URL)
        .send()
        .await
        .map_err(|_| "liveUnavailable".to_string())?
        .text()
        .await
        .map_err(|_| "liveUnavailable".to_string())?;
    let v = parse(&body)?;
    *cache().lock().expect("presets live cache") = Some((now, v.clone()));
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn reads_the_server_file() {
        let body = r#"{"version":1,"t0":"2026-10-08T00:00:00Z","updated_at":"x","data_end":"y","days":2.5,
            "min_days_for_ranking":30,"method":"m","templates":[{"id":"dca_long_classic","lab_id":"T01",
            "return_pct":0.42,"bots":5,"cycles":7,"open_cycles":5,"worst_dd_pct":-1.2,"median_dd_pct":-0.4,
            "win_rate_cycles":1.0,"history":[{"d":"2026-10-08","r":0.1,"dd":-0.5}]}]}"#;
        let v = parse(body).unwrap();
        assert_eq!(v.templates[0].id, "dca_long_classic");
        assert_eq!(v.templates[0].history[0].r, 0.1);
        assert_eq!(v.min_days_for_ranking, 30.0);
    }

    #[test]
    fn junk_is_an_honest_error() {
        assert_eq!(parse("<html>").unwrap_err(), "liveUnavailable");
    }
}
