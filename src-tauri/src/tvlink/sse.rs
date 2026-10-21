//! The relay's event stream (`GET /api/edge/tv-webhook/events/stream`), as
//! a pure decoder and the reconnect rule:
//!
//! ```text
//! retry: 3000
//!
//! id: 42
//! event: events
//! data: {"events":[...],"now":1760000000000}
//!
//! event: ping
//! data: {"now":1760000015000}
//! ```
//!
//! The server closes a stream after 5 minutes; the app reconnects at once
//! with the last id (`?after=` and `Last-Event-ID`). A refused stream (429:
//! three already open for the account, or any failure) falls back to the
//! events poll for a while, then the stream is tried again (`next_mode`).

use std::time::Duration;

use super::api::{parse_events, EventsPage};

/// One dispatched frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Frame {
    pub id: Option<String>,
    pub event: String,
    pub data: String,
    pub retry_ms: Option<u64>,
}

/// What one frame means to the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Events { id: Option<String>, page: EventsPage },
    /// Keep-alive with the server's time.
    Ping { now: Option<u64> },
    /// `retry:` alone, or an event the app does not use.
    Other,
}

/// Longest line or event body kept (the relay's events page is far below).
const MAX_BUFFER: usize = 2 * 1024 * 1024;

/// Incremental decoder: lines end in CRLF, LF or CR; a blank line
/// dispatches. Bytes are buffered so a UTF-8 character split across two
/// chunks survives.
#[derive(Default)]
pub struct Decoder {
    line: Vec<u8>,
    skip_lf: bool,
    started: bool,
    cur: Frame,
    has_data: bool,
    size: usize,
}

impl Decoder {
    /// Feeds one chunk; returns the frames it completed, in order. Err when
    /// a frame grows past the buffer limit (the stream is then dropped).
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Frame>, &'static str> {
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
                    if self.line.len() + self.size > MAX_BUFFER {
                        return Err("tvStreamOversized");
                    }
                }
            }
        }
        Ok(out)
    }

    fn end_line(&mut self, out: &mut Vec<Frame>) {
        let raw = std::mem::take(&mut self.line);
        let mut line = String::from_utf8_lossy(&raw).into_owned();
        if !std::mem::replace(&mut self.started, true) {
            if let Some(rest) = line.strip_prefix('\u{feff}') {
                line = rest.to_string();
            }
        }
        if line.is_empty() {
            let frame = std::mem::take(&mut self.cur);
            let had = std::mem::take(&mut self.has_data);
            self.size = 0;
            if had || frame.retry_ms.is_some() {
                out.push(frame);
            }
            return;
        }
        if line.starts_with(':') {
            return;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line.as_str(), ""),
        };
        match field {
            "id" if !value.contains('\0') => self.cur.id = Some(value.to_string()),
            "event" => self.cur.event = value.to_string(),
            "data" => {
                if self.has_data {
                    self.cur.data.push('\n');
                }
                self.cur.data.push_str(value);
                self.has_data = true;
                self.size += value.len() + 1;
            }
            "retry" => self.cur.retry_ms = value.trim().parse().ok(),
            _ => {}
        }
    }
}

/// Reads one frame. An `events` frame whose data is not the events page is
/// `Err` (the stream is then treated as failed, the poll takes over).
pub fn item(frame: &Frame) -> Result<Item, &'static str> {
    match frame.event.as_str() {
        "events" | "message" | "" if !frame.data.is_empty() => {
            let page = parse_events(&frame.data).map_err(|_| "tvBadResponse")?;
            Ok(Item::Events { id: frame.id.clone(), page })
        }
        "ping" => {
            let now = serde_json::from_str::<serde_json::Value>(&frame.data)
                .ok()
                .and_then(|v| super::api::time_ms(v.get("now")));
            Ok(Item::Ping { now })
        }
        _ => Ok(Item::Other),
    }
}

/// How events are read now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Connect (or reconnect) the stream.
    Stream,
    /// Poll for this long, then try the stream again.
    Poll(Duration),
}

/// How a stream session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// The server closed it (its 5-minute cap): reconnect with the last id.
    Closed,
    /// No byte for the idle limit (pings come every 15 s).
    Idle,
    /// Refused or broken: an `errors.*` code (`tvRateLimited` on 429).
    Failed(String),
    /// The app ended it ("Receive alerts" turned off).
    Stopped,
}

/// After this many failed streams in a row the poll runs for `POLL_SPELL`
/// before the stream is tried again.
pub const POLL_SPELL: Duration = Duration::from_secs(60);
/// The relay's own `retry:` hint, used after one failure.
pub const DEFAULT_RETRY: Duration = Duration::from_millis(3_000);
/// No byte for this long ends a stream (the server pings every 15 s).
pub const IDLE_LIMIT: Duration = Duration::from_secs(45);

/// What follows a stream session: a clean close reconnects at once; a
/// first idle or failure retries after the relay's `retry` hint; a 429 (too
/// many streams for the account) or a second failure in a row falls back
/// to the poll for `POLL_SPELL`.
pub fn next_mode(end: &End, failures_in_row: u32) -> (Mode, Duration) {
    match end {
        End::Closed | End::Stopped => (Mode::Stream, Duration::ZERO),
        End::Failed(code) if code == "tvRateLimited" => (Mode::Poll(POLL_SPELL), Duration::ZERO),
        _ if failures_in_row >= 2 => (Mode::Poll(POLL_SPELL), Duration::ZERO),
        _ => (Mode::Stream, DEFAULT_RETRY),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(chunks: &[&[u8]]) -> Vec<Frame> {
        let mut d = Decoder::default();
        chunks.iter().flat_map(|c| d.push(c).unwrap()).collect()
    }

    #[test]
    fn frames_carry_id_event_data_and_retry() {
        let body = b"retry: 3000\n\nid: 42\nevent: events\ndata: {\"events\":[{\"id\":42,\"receivedAt\":1760000000000,\"body\":\"x\",\"appOffline\":true}],\"now\":1760000001000}\n\n: comment\nevent: ping\ndata: {\"now\":1760000015000}\n\n";
        let frames = all(&[body]);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].retry_ms, Some(3000));
        assert_eq!(item(&frames[0]), Ok(Item::Other));
        assert_eq!(frames[1].id.as_deref(), Some("42"));
        match item(&frames[1]).unwrap() {
            Item::Events { id, page } => {
                assert_eq!(id.as_deref(), Some("42"));
                assert_eq!(page.now, Some(1_760_000_001_000));
                assert_eq!(page.events.len(), 1);
                assert!(page.events[0].app_offline, "the relay's offline flag is read");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(item(&frames[2]), Ok(Item::Ping { now: Some(1_760_000_015_000) }));
    }

    #[test]
    fn a_frame_split_anywhere_decodes_the_same() {
        let body = "id: 7\r\nevent: events\r\ndata: {\"events\":[],\"now\":5}\r\n\r\nevent: ping\rdata: {\"now\":6}\r\r".as_bytes();
        let whole = all(&[body]);
        for cut in 1..body.len() {
            assert_eq!(all(&[&body[..cut], &body[cut..]]), whole, "cut at {cut}");
        }
        assert_eq!(whole.len(), 2);
        // A multibyte character split across two chunks.
        let text = "data: {\"events\":[{\"id\":1,\"body\":\"é\"}]}\n\n".as_bytes();
        let at = text.iter().position(|&b| b == 0xc3).unwrap() + 1;
        let f = all(&[&text[..at], &text[at..]]);
        assert!(f[0].data.contains('é'));
    }

    #[test]
    fn an_unreadable_events_frame_fails_the_stream() {
        let f = Frame { event: "events".into(), data: "not json".into(), ..Default::default() };
        assert_eq!(item(&f), Err("tvBadResponse"));
        let mut d = Decoder::default();
        assert_eq!(d.push(&vec![b'x'; MAX_BUFFER + 1]), Err("tvStreamOversized"));
    }

    #[test]
    fn a_closed_stream_reconnects_and_a_refused_one_falls_back_to_the_poll() {
        assert_eq!(next_mode(&End::Closed, 0), (Mode::Stream, Duration::ZERO));
        assert_eq!(next_mode(&End::Idle, 1), (Mode::Stream, DEFAULT_RETRY));
        assert_eq!(next_mode(&End::Failed("tvHttp|502".into()), 1), (Mode::Stream, DEFAULT_RETRY));
        assert_eq!(next_mode(&End::Failed("tvHttp|502".into()), 2), (Mode::Poll(POLL_SPELL), Duration::ZERO));
        assert_eq!(next_mode(&End::Failed("tvRateLimited".into()), 1), (Mode::Poll(POLL_SPELL), Duration::ZERO), "three streams already open");
    }
}
