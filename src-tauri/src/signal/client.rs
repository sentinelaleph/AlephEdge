//! Real Sentinel signal-stream transport: mint a single-use ticket, then open
//! the SSE connection with it (mirrors `sentinel-alephv2/frontend/src/services/api/stream.ts`
//! — browsers can't send an Authorization header to `EventSource`, so a
//! short-lived ticket stands in for it).

use futures_util::StreamExt;
use reqwest::{Client, Url};
use std::time::{Duration, Instant};

use super::model::StreamTicketResponse;

/// Ticket minting is a short request and gets a timeout. The stream GET below
/// deliberately does NOT — it is a long-lived SSE body that must stay open.
const TICKET_TIMEOUT: Duration = Duration::from_secs(8);
/// Hard cap on one event's bytes (its unterminated line plus its data lines)
/// — a server that never sends a frame boundary can't grow memory without
/// bound.
const MAX_STREAM_BUFFER: usize = 1 << 20; // 1 MiB
/// Max gap between stream reads before we treat the connection as dead. The
/// server sends a heartbeat every 15s, so a silence of 45s means the socket is
/// half-open (proxy dropped it, laptop slept, network blip) — a plain
/// `bytes.next()` would then block forever and the stream would never
/// reconnect. Timing out here surfaces an error so the supervisor rebuilds it.
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(45);

/// Why a stream session ended. Typed, so the supervisor decides on the
/// variant (a 401 refreshes the token) instead of matching on text; `code()`
/// is the stable `errors.*` code the UI localizes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamError {
    TicketNetwork,
    TicketStatus(u16),
    TicketMalformed,
    BaseUrlMalformed,
    StreamNetwork,
    StreamStatus(u16),
    StreamIdle,
    StreamRead,
    StreamOversized,
}

impl StreamError {
    /// The access token was refused: refresh it before the next attempt.
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Self::TicketStatus(401) | Self::StreamStatus(401))
    }

    /// Stable code (+ `|detail`) for `errors.*` (see `src/lib/errorText.ts`).
    pub fn code(&self) -> String {
        match self {
            Self::TicketNetwork => "signalTicketNetwork".into(),
            Self::TicketStatus(s) => format!("signalTicketRefused|{s}"),
            Self::TicketMalformed => "signalTicketMalformed".into(),
            Self::BaseUrlMalformed => "signalBaseUrlMalformed".into(),
            Self::StreamNetwork => "signalStreamNetwork".into(),
            Self::StreamStatus(s) => format!("signalStreamRefused|{s}"),
            Self::StreamIdle => "signalStreamIdle".into(),
            Self::StreamRead => "signalStreamRead".into(),
            Self::StreamOversized => "signalStreamOversized".into(),
        }
    }
}

/// One parsed SSE frame, dispatched by its `event:` name. `Heartbeat` carries
/// no payload we act on — its arrival alone is the liveness signal.
pub enum SseEvent {
    Signal(String),
    Heartbeat,
    /// A signal was voided server-side; payload carries the signal id.
    Invalidation(String),
    /// NEW (2026-09-18): Sentinel vetoed a signal because BTC's regime turned
    /// against it — unlike `Invalidation`, this one also closes an open
    /// position. Payload is the full veto JSON (see `model::RegimeVetoEvent`).
    Veto(String),
    Other,
}

/// Mints a stream ticket. Returns the ticket plus the request's round-trip
/// time (the one honest latency number available for a push-style feed).
pub async fn request_ticket(
    client: &Client,
    base: &str,
    token: &str,
) -> Result<(String, u32), StreamError> {
    let started = Instant::now();
    // Sentinel's JSON middleware rejects any POST whose Content-Type isn't
    // application/json with 415 — so send the header (+ an empty JSON body),
    // exactly as the web client does. Without this the stream never connects.
    let res = client
        .post(format!("{base}/api/v1/stream/ticket"))
        .bearer_auth(token)
        .header("Content-Type", "application/json")
        .body("{}")
        .timeout(TICKET_TIMEOUT)
        .send()
        .await
        .map_err(|_| StreamError::TicketNetwork)?;
    let latency_ms = started.elapsed().as_millis() as u32;
    if !res.status().is_success() {
        return Err(StreamError::TicketStatus(res.status().as_u16()));
    }
    let body: StreamTicketResponse = res
        .json()
        .await
        .map_err(|_| StreamError::TicketMalformed)?;
    Ok((body.ticket, latency_ms))
}

fn stream_url(base: &str, ticket: &str) -> Result<Url, StreamError> {
    let mut url = Url::parse(base)
        .map_err(|_| StreamError::BaseUrlMalformed)?
        .join("/api/v1/stream")
        .map_err(|_| StreamError::BaseUrlMalformed)?;
    url.query_pairs_mut().append_pair("ticket", ticket);
    Ok(url)
}

/// Opens the SSE connection and calls `on_event` for every frame until the
/// stream ends or errors. Returns when the connection closes — the caller
/// decides whether/when to reconnect.
pub async fn run_stream(
    client: &Client,
    base: &str,
    ticket: &str,
    mut on_event: impl FnMut(SseEvent),
) -> Result<(), StreamError> {
    let url = stream_url(base, ticket)?;
    let res = client
        .get(url)
        .send()
        .await
        .map_err(|_| StreamError::StreamNetwork)?;
    if !res.status().is_success() {
        return Err(StreamError::StreamStatus(res.status().as_u16()));
    }

    let mut decoder = SseDecoder::default();
    let mut bytes = res.bytes_stream();
    // Bound each read on the idle timeout — a dead socket that never yields
    // another chunk must not strand the stream (see STREAM_IDLE_TIMEOUT).
    loop {
        let next = tokio::time::timeout(STREAM_IDLE_TIMEOUT, bytes.next()).await;
        let chunk = match next {
            Err(_) => return Err(StreamError::StreamIdle),
            Ok(None) => break, // server closed cleanly
            Ok(Some(chunk)) => chunk.map_err(|_| StreamError::StreamRead)?,
        };
        for event in decoder.push(&chunk)? {
            on_event(event);
        }
    }
    Ok(())
}

/// Incremental SSE decoder (WHATWG "event stream interpretation").
///
/// Lines end in CRLF, a lone LF **or a lone CR**; a blank line dispatches the
/// event. Framing used to look only for `\n\n`, so a server (or proxy) that
/// writes CRLF never produced a frame boundary: no event ever dispatched and
/// the buffer grew until the 1 MiB cap killed the stream. A CR that ends one
/// chunk is resolved by the next byte (`skip_lf`), so a CRLF split across
/// chunks is still one line end.
///
/// Raw BYTES are buffered (not a lossy String) so a multibyte UTF-8 char split
/// across two chunks is not corrupted; a line is decoded only once complete,
/// and line ends are ASCII, so they never fall inside a character.
#[derive(Default)]
pub struct SseDecoder {
    /// The current, unterminated line.
    line: Vec<u8>,
    /// The previous byte was a CR: a LF right after it belongs to that CR.
    skip_lf: bool,
    /// The stream's first line is still to come (a leading BOM is dropped).
    started: bool,
    event_name: String,
    data: Vec<String>,
    /// Bytes held in `data` — bounded like the line itself.
    data_bytes: usize,
}

impl SseDecoder {
    /// Feeds one chunk; returns the events it completed, in order.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, StreamError> {
        let mut out = Vec::new();
        for &b in chunk {
            if std::mem::take(&mut self.skip_lf) && b == b'\n' {
                continue;
            }
            match b {
                b'\r' => {
                    self.skip_lf = true;
                    self.end_line(&mut out);
                }
                b'\n' => self.end_line(&mut out),
                _ => {
                    self.line.push(b);
                    if self.line.len() + self.data_bytes > MAX_STREAM_BUFFER {
                        return Err(StreamError::StreamOversized);
                    }
                }
            }
        }
        Ok(out)
    }

    fn end_line(&mut self, out: &mut Vec<SseEvent>) {
        let raw = std::mem::take(&mut self.line);
        let mut line = String::from_utf8_lossy(&raw).into_owned();
        if !std::mem::replace(&mut self.started, true) {
            if let Some(rest) = line.strip_prefix('\u{feff}') {
                line = rest.to_string();
            }
        }
        if line.is_empty() {
            if let Some(event) = self.dispatch() {
                out.push(event);
            }
            return;
        }
        if line.starts_with(':') {
            return; // comment / keepalive padding
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line.as_str(), ""),
        };
        match field {
            "event" => self.event_name = value.to_string(),
            "data" => {
                self.data_bytes += value.len() + 1;
                self.data.push(value.to_string());
            }
            _ => {} // id / retry / unknown: not used here
        }
    }

    /// Ends the current event. Without a `data:` line nothing is dispatched
    /// (comments and keepalive padding some SSE servers send).
    fn dispatch(&mut self) -> Option<SseEvent> {
        let name = std::mem::take(&mut self.event_name);
        let data = std::mem::take(&mut self.data);
        self.data_bytes = 0;
        if data.is_empty() {
            return None;
        }
        Some(classify_event(&name, data.join("\n")))
    }
}

/// Maps an event name to its dispatch arm.
fn classify_event(name: &str, data: String) -> SseEvent {
    match name {
        "signal" => SseEvent::Signal(data),
        "heartbeat" => SseEvent::Heartbeat,
        // Server-side protection layer (PRD §1/§3.3): an invalidation frame
        // (`btc_trend_flip` etc.) voids a signal — the bot must not enter it.
        // Open positions are NOT closed on it (measured −163 pts; OFF upstream).
        "invalidation" | "signal_invalidated" => SseEvent::Invalidation(data),
        // NEW (2026-09-18): the BTC-regime guard's own veto — closes an open
        // position immediately, on top of blocking entry. Sentinel sends the
        // legacy "invalidation" frame too, for the same signal; both are
        // handled (see `signal::veto::mark_regime_veto`).
        "veto" => SseEvent::Veto(data),
        _ => SseEvent::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses one complete frame (terminated here) for the dispatch tests.
    fn parse_frame(frame: &str) -> Option<SseEvent> {
        let mut d = SseDecoder::default();
        let mut out = d.push(format!("{frame}\n\n").as_bytes()).unwrap();
        assert!(out.len() <= 1);
        out.pop()
    }

    fn describe(e: &SseEvent) -> String {
        match e {
            SseEvent::Signal(d) => format!("signal:{d}"),
            SseEvent::Heartbeat => "heartbeat".into(),
            SseEvent::Invalidation(d) => format!("invalidation:{d}"),
            SseEvent::Veto(d) => format!("veto:{d}"),
            SseEvent::Other => "other".into(),
        }
    }

    fn decode_all(chunks: &[&[u8]]) -> Vec<String> {
        let mut d = SseDecoder::default();
        chunks
            .iter()
            .flat_map(|c| d.push(c).unwrap())
            .map(|e| describe(&e))
            .collect()
    }

    #[test]
    fn veto_frame_dispatches_to_veto_not_invalidation() {
        let frame = "event: veto\ndata: {\"signal_id\":\"s1\"}";
        assert!(
            matches!(parse_frame(frame), Some(SseEvent::Veto(d)) if d == r#"{"signal_id":"s1"}"#)
        );
    }

    #[test]
    fn legacy_invalidation_names_still_dispatch_to_invalidation() {
        for name in ["invalidation", "signal_invalidated"] {
            let frame = format!("event: {name}\ndata: {{\"signal_id\":\"s1\"}}");
            assert!(
                matches!(parse_frame(&frame), Some(SseEvent::Invalidation(_))),
                "{name}"
            );
        }
    }

    #[test]
    fn frame_without_data_is_dropped() {
        assert!(parse_frame("event: veto").is_none());
        assert!(parse_frame(": keepalive").is_none());
    }

    /// CRLF, lone LF and lone CR all end a line; a blank line of any kind
    /// dispatches. The old framer saw only `\n\n`, so CRLF streams never
    /// dispatched a single event.
    #[test]
    fn mixed_line_endings_all_frame() {
        let stream = b"event: signal\r\ndata: a\r\n\r\n\
event: veto\rdata: b\r\r\
event: heartbeat\ndata: c\n\n\
event: invalidation\r\ndata: d\n\r";
        assert_eq!(
            decode_all(&[stream]),
            vec!["signal:a", "veto:b", "heartbeat", "invalidation:d"]
        );
    }

    /// A CRLF split across chunks is ONE line end, not a line end plus a
    /// blank line (which would dispatch half an event).
    #[test]
    fn crlf_split_across_chunks_is_one_line_end() {
        let got = decode_all(&[b"event: signal\r", b"\ndata: x\r", b"\ndata: y\r", b"\n\r", b"\n"]);
        assert_eq!(got, vec!["signal:x\ny"]);
    }

    /// Byte-by-byte delivery gives the same events as one chunk.
    #[test]
    fn byte_by_byte_matches_one_chunk() {
        let stream: &[u8] = b"\xef\xbb\xbfevent: signal\r\ndata: {\"s\":\"\xc3\xa7\"}\r\n\r\nevent: veto\rdata:v\r\r";
        let whole = decode_all(&[stream]);
        let bytes: Vec<&[u8]> = stream.chunks(1).collect();
        assert_eq!(decode_all(&bytes), whole);
        // BOM dropped, the split multibyte char intact, one optional space
        // after the colon removed.
        assert_eq!(whole, vec!["signal:{\"s\":\"\u{e7}\"}", "veto:v"]);
    }

    #[test]
    fn an_unterminated_flood_is_refused() {
        let mut d = SseDecoder::default();
        let big = vec![b'x'; MAX_STREAM_BUFFER + 1];
        assert!(matches!(d.push(&big), Err(StreamError::StreamOversized)));
    }

    /// The supervisor refreshes the token on the TYPE, not on text that a
    /// later localization would change.
    #[test]
    fn only_a_401_asks_for_a_token_refresh() {
        assert!(StreamError::TicketStatus(401).is_unauthorized());
        assert!(StreamError::StreamStatus(401).is_unauthorized());
        assert!(!StreamError::TicketStatus(503).is_unauthorized());
        assert!(!StreamError::TicketNetwork.is_unauthorized());
        assert!(!StreamError::StreamIdle.is_unauthorized());
    }

    #[test]
    fn errors_are_stable_codes() {
        assert_eq!(StreamError::TicketStatus(401).code(), "signalTicketRefused|401");
        assert_eq!(StreamError::StreamStatus(502).code(), "signalStreamRefused|502");
        assert_eq!(StreamError::StreamIdle.code(), "signalStreamIdle");
        for e in [
            StreamError::TicketNetwork,
            StreamError::TicketMalformed,
            StreamError::BaseUrlMalformed,
            StreamError::StreamNetwork,
            StreamError::StreamRead,
            StreamError::StreamOversized,
        ] {
            let c = e.code();
            assert!(c.starts_with("signal") && !c.contains(' '), "{c}");
        }
    }
}
