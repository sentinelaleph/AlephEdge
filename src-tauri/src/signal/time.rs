//! RFC3339 timestamp parsing for Sentinel's `created_at` / `expires_at`.
//!
//! Hand-rolled because chrono is not a dependency. Sentinel emits Go's
//! RFC3339Nano (nanoseconds, variable length) and sometimes a numeric offset
//! (`+03:00`) instead of `Z`; the previous parser only understood a trailing
//! `Z`, so an offset timestamp read as "unparseable" and the signal was refused
//! as expired.

/// Parses "YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)" into UNIX millis.
/// Fractional seconds of any length are accepted (truncated to millis).
/// Returns None for anything else, including pre-1970 instants.
pub fn parse_rfc3339_ms(s: &str) -> Option<u64> {
    let s = s.trim();
    let (date, rest) = s.split_once(['T', 't', ' '])?;
    let (clock, offset_secs) = split_offset(rest)?;

    let mut d = date.split('-');
    let y: i64 = num(d.next()?, 4)?;
    let m: i64 = num(d.next()?, 2)?;
    let day: i64 = num(d.next()?, 2)?;
    if d.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }

    let (hms, frac) = clock.split_once('.').unwrap_or((clock, ""));
    let mut t = hms.split(':');
    let h: i64 = num(t.next()?, 2)?;
    let min: i64 = num(t.next()?, 2)?;
    let sec: i64 = num(t.next()?, 2)?;
    if t.next().is_some() || h > 23 || min > 59 || sec > 60 {
        return None;
    }
    if !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // Right-pad to 3 digits so ".5" = 500 ms, then drop sub-millisecond digits.
    let mut frac3: String = frac.chars().take(3).collect();
    while frac3.len() < 3 {
        frac3.push('0');
    }
    let millis: i64 = frac3.parse().ok()?;

    // Days since epoch via Howard Hinnant's civil-days algorithm.
    let (y2, m2) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (m2 - 3) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + h * 3_600 + min * 60 + sec - offset_secs;
    u64::try_from(secs * 1000 + millis).ok()
}

/// Splits the time part into (clock, offset seconds east of UTC).
fn split_offset(rest: &str) -> Option<(&str, i64)> {
    if let Some(clock) = rest.strip_suffix(['Z', 'z']) {
        return Some((clock, 0));
    }
    let idx = rest.rfind(['+', '-'])?;
    let (clock, off) = rest.split_at(idx);
    let sign = if off.starts_with('-') { -1 } else { 1 };
    let (oh, om) = off[1..].split_once(':')?;
    let oh: i64 = num(oh, 2)?;
    let om: i64 = num(om, 2)?;
    if oh > 23 || om > 59 {
        return None;
    }
    Some((clock, sign * (oh * 3_600 + om * 60)))
}

/// Parses an all-digit field of exactly `len` characters.
fn num(s: &str, len: usize) -> Option<i64> {
    (s.len() == len && s.chars().all(|c| c.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}
