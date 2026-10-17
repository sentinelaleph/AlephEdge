//! "Before you trade" (0.3.17): one read-only Sentinel payload
//! (`GET /api/public/edge/before-you-trade`, schema `before-you-trade/1`)
//! with the BTC regime, the chart read and its hit record, volatility,
//! funding, the template evidence (lab, ghost track, bot farm), the news
//! digest with its measured impact, and new listings.
//!
//! The app passes the payload through untouched (the typed contract lives in
//! `src/lib/ipc/beforeYouTrade/beforeYouTrade.ts`); this side only checks the
//! schema, caches it for its TTL, and keeps the last good copy on disk so an
//! offline desk still shows it, with its age. Every figure keeps its own
//! `status` / `reason` from the server: nothing is filled in here.
//!
//! Links: a news or listing source opens only if it is one of the URLs in the
//! payload this desk actually received (`byt_open_link`); the page cannot be
//! made to open an arbitrary address.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

pub const PATH: &str = "/api/public/edge/before-you-trade";
pub const SCHEMA: &str = "before-you-trade/1";
/// The server builds the payload at most once per 5 minutes.
const DEFAULT_TTL_MS: u64 = 300_000;
const CACHE_FILE: &str = "before-you-trade.json";

pub fn url_on(base: &str) -> String {
    format!("{}{PATH}", base.trim_end_matches('/'))
}

/// What the page (and the Assistant's read tool) gets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BeforeYouTradeView {
    pub payload: Value,
    /// When this desk received `payload` (ms since epoch).
    pub fetched_at_ms: u64,
    /// The last fetch failed and this is the previous copy.
    pub stale: bool,
    /// Why the last fetch failed (stable code), when it did.
    pub error: Option<String>,
}

/// Body -> payload, or a stable error code. Anything that is not a
/// `before-you-trade/1` object is refused rather than half-rendered.
pub fn parse(body: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| "bytUnavailable".to_string())?;
    match v.get("schema").and_then(Value::as_str) {
        Some(SCHEMA) => Ok(v),
        Some(_) => Err("bytSchema".into()),
        None => Err("bytUnavailable".into()),
    }
}

/// The payload's own cache TTL (`cache_ttl_seconds`), 5 min when absent.
pub fn ttl_ms(payload: &Value) -> u64 {
    payload
        .get("cache_ttl_seconds")
        .and_then(Value::as_u64)
        .filter(|s| *s > 0 && *s <= 3600)
        .map_or(DEFAULT_TTL_MS, |s| s * 1000)
}

/// Every source link the payload carries (news lines, listing announcements).
pub fn links(payload: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |v: Option<&Value>| {
        if let Some(s) = v.and_then(Value::as_str) {
            if !s.is_empty() {
                out.push(s.to_string());
            }
        }
    };
    if let Some(lines) = payload.pointer("/news/lines").and_then(Value::as_array) {
        for l in lines {
            push(l.get("url"));
        }
    }
    for list in ["/listings/upcoming", "/listings/recent"] {
        if let Some(rows) = payload.pointer(list).and_then(Value::as_array) {
            for r in rows {
                push(r.pointer("/announcement/url"));
            }
        }
    }
    out
}

/// A link may open only if it is an https URL the payload itself carries.
pub fn link_allowed(payload: &Value, url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://") && !url.chars().any(char::is_whitespace) && links(payload).iter().any(|l| l == url)
}

#[derive(Serialize, Deserialize)]
struct Disk {
    fetched_at_ms: u64,
    payload: Value,
}

fn cache_path(dir: &Path) -> PathBuf {
    dir.join(CACHE_FILE)
}

fn read_disk(dir: &Path) -> Option<(u64, Value)> {
    let body = std::fs::read_to_string(cache_path(dir)).ok()?;
    let d: Disk = serde_json::from_str(&body).ok()?;
    (d.payload.get("schema").and_then(Value::as_str) == Some(SCHEMA)).then_some((d.fetched_at_ms, d.payload))
}

fn write_disk(dir: &Path, at: u64, payload: &Value) {
    let Ok(body) = serde_json::to_string(&Disk { fetched_at_ms: at, payload: payload.clone() }) else {
        return;
    };
    let tmp = dir.join(format!("{CACHE_FILE}.tmp"));
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, cache_path(dir));
    }
}

fn memory() -> &'static Mutex<Option<(u64, Value)>> {
    static C: OnceLock<Mutex<Option<(u64, Value)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .unwrap_or_default()
    })
}

async fn fetch() -> Result<Value, String> {
    let resp = client()
        .get(url_on(&super::endpoints::api_base()))
        .send()
        .await
        .map_err(|_| "bytUnavailable".to_string())?;
    if !resp.status().is_success() {
        return Err(format!("bytHttp|{}", resp.status().as_u16()));
    }
    let body = resp.text().await.map_err(|_| "bytUnavailable".to_string())?;
    parse(&body)
}

/// The decision a read makes, pure: serve the fresh copy, or fall back.
pub fn resolve(
    fetched: Result<Value, String>,
    now: u64,
    previous: Option<(u64, Value)>,
) -> Result<BeforeYouTradeView, String> {
    match fetched {
        Ok(payload) => Ok(BeforeYouTradeView { payload, fetched_at_ms: now, stale: false, error: None }),
        Err(e) => match previous {
            Some((at, payload)) => Ok(BeforeYouTradeView { payload, fetched_at_ms: at, stale: true, error: Some(e) }),
            None => Err(e),
        },
    }
}

/// The current payload: the cached copy within its TTL, else a fetch, else
/// the last good copy (marked stale). Shared with the Assistant's read tool.
pub async fn current(app: &AppHandle, refresh: bool) -> Result<BeforeYouTradeView, String> {
    let now = crate::bot::engine::now_ms();
    let dir = app.path().app_data_dir().ok();
    let mut cached = memory().lock().expect("byt cache").clone();
    if cached.is_none() {
        if let Some(d) = dir.as_deref() {
            cached = read_disk(d);
        }
    }
    if let (false, Some((at, v))) = (refresh, cached.as_ref()) {
        if now.saturating_sub(*at) < ttl_ms(v) {
            return Ok(BeforeYouTradeView { payload: v.clone(), fetched_at_ms: *at, stale: false, error: None });
        }
    }
    let view = resolve(fetch().await, now, cached)?;
    if !view.stale {
        *memory().lock().expect("byt cache") = Some((view.fetched_at_ms, view.payload.clone()));
        if let Some(d) = dir.as_deref() {
            write_disk(d, view.fetched_at_ms, &view.payload);
        }
    }
    Ok(view)
}

#[tauri::command]
pub async fn before_you_trade(app: AppHandle, refresh: Option<bool>) -> Result<BeforeYouTradeView, String> {
    current(&app, refresh.unwrap_or(false)).await
}

/// Opens one of the payload's own source links in the browser.
#[tauri::command]
pub fn byt_open_link(app: AppHandle, url: String) -> Result<(), String> {
    let mut payload = memory().lock().expect("byt cache").clone().map(|(_, v)| v);
    if payload.is_none() {
        payload = app.path().app_data_dir().ok().and_then(|d| read_disk(&d)).map(|(_, v)| v);
    }
    match payload {
        Some(p) if link_allowed(&p, &url) => app.opener().open_url(url, None::<&str>).map_err(|_| "linkOpenFailed".to_string()),
        _ => Err("linkNotAllowed".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../../src/lib/ipc/beforeYouTrade/beforeYouTrade.fixture.json");

    #[test]
    fn url_follows_the_api_base() {
        assert_eq!(url_on("https://ribqa.com"), "https://ribqa.com/api/public/edge/before-you-trade");
        assert_eq!(url_on("http://localhost:8080/"), "http://localhost:8080/api/public/edge/before-you-trade");
    }

    #[test]
    fn reads_the_live_fixture() {
        let v = parse(FIXTURE).unwrap();
        assert_eq!(v["schema"], SCHEMA);
        assert_eq!(ttl_ms(&v), 300_000);
        assert_eq!(v.pointer("/presets/lab/templates").and_then(Value::as_array).map(Vec::len), Some(12));
        assert!(!links(&v).is_empty());
    }

    #[test]
    fn another_schema_or_junk_is_an_honest_error() {
        assert_eq!(parse("<html>").unwrap_err(), "bytUnavailable");
        assert_eq!(parse(r#"{"schema":"before-you-trade/2"}"#).unwrap_err(), "bytSchema");
        assert_eq!(parse(r#"{"market":{}}"#).unwrap_err(), "bytUnavailable");
    }

    #[test]
    fn offline_serves_the_last_copy_with_its_age() {
        let old = parse(FIXTURE).unwrap();
        let v = resolve(Err("bytUnavailable".into()), 9_000, Some((1_000, old.clone()))).unwrap();
        assert!(v.stale);
        assert_eq!(v.fetched_at_ms, 1_000, "the age is the copy's, not now");
        assert_eq!(v.error.as_deref(), Some("bytUnavailable"));
        assert_eq!(resolve(Err("bytHttp|503".into()), 9_000, None).unwrap_err(), "bytHttp|503");
        let fresh = resolve(Ok(old), 9_000, None).unwrap();
        assert!(!fresh.stale && fresh.error.is_none() && fresh.fetched_at_ms == 9_000);
    }

    #[test]
    fn only_the_payloads_own_links_open() {
        let v = parse(FIXTURE).unwrap();
        let first = links(&v)[0].clone();
        assert!(link_allowed(&v, &first));
        assert!(!link_allowed(&v, "https://evil.example/x"));
        assert!(!link_allowed(&v, &first.replacen("https://", "http://", 1)));
        assert!(!link_allowed(&v, "file:///C:/Windows/system32/calc.exe"));
    }

    #[test]
    fn the_disk_copy_round_trips() {
        let dir = std::env::temp_dir().join(format!("byt-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let v = parse(FIXTURE).unwrap();
        write_disk(&dir, 42, &v);
        assert_eq!(read_disk(&dir), Some((42, v)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
