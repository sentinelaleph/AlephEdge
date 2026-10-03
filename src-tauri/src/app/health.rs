//! Health snapshot types + the empty baseline the health command fills.
//!
//! `mock_snapshot` (name kept for its call site) is an honest "nothing known
//! yet" snapshot: live providers (exchange status, Sentinel stream, BTC macro)
//! overwrite it in `app::commands::get_health_snapshot`, and anything they do
//! not overwrite reaches the UI as unknown/empty, never as an invented value.

use serde::{Deserialize, Serialize};

/// Coarse traffic-light status shared by every health row.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthLevel {
    Ok,
    Warn,
    Down,
    /// Not yet known / warming up.
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeHealth {
    pub id: String,
    pub name: String,
    pub level: HealthLevel,
    /// Round-trip latency in milliseconds, if measured.
    pub latency_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalHealth {
    pub level: HealthLevel,
    pub connected: bool,
    /// Seconds since the last signal was received, if any.
    pub last_signal_secs: Option<u32>,
    pub latency_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BtcPulse {
    /// "bullish" | "bearish" | "neutral".
    pub trend: String,
    /// 0.0–1.0 trend strength.
    pub strength: f64,
    pub phase: String,
    pub price: f64,
    pub change_pct: f64,
    /// Recent close series for a mini sparkline (oldest → newest).
    pub sparkline: Vec<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VaultState {
    Locked,
    Unlocked,
    /// No vault created yet.
    Absent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MembershipState {
    Active,
    Expiring,
    Inactive,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthSnapshot {
    pub exchanges: Vec<ExchangeHealth>,
    pub signal: SignalHealth,
    pub btc: BtcPulse,
    pub vault: VaultState,
    pub membership: MembershipState,
    /// UNIX millis when this snapshot was produced.
    pub generated_at: u64,
}

/// Baseline snapshot the health command overwrites with live providers
/// (`app::commands::get_health_snapshot`).
///
/// Every field is an honest "not known yet": no exchanges, signal down, BTC
/// pulse empty (price 0, no sparkline), membership/vault unknown/locked. It
/// used to synthesize a BTC pulse (≈63 200, "Consolidation", −1.2 %, a sine
/// sparkline) that stayed in the snapshot whenever Sentinel's macro feed did
/// not answer, so a fabricated price reached the UI as if it were real; the UI
/// had to fingerprint those exact values to hide them. `tick` is kept for the
/// call site's signature and no longer shapes any value.
pub fn mock_snapshot(_tick: u64) -> HealthSnapshot {
    HealthSnapshot {
        exchanges: Vec::new(),
        signal: SignalHealth {
            level: HealthLevel::Unknown,
            connected: false,
            last_signal_secs: None,
            latency_ms: None,
        },
        btc: BtcPulse {
            trend: "unknown".into(),
            strength: 0.0,
            phase: String::new(),
            price: 0.0,
            change_pct: 0.0,
            sparkline: Vec::new(),
        },
        vault: VaultState::Locked,
        membership: MembershipState::Unknown,
        generated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_baseline_invents_no_market_data() {
        // Whatever the live feeds fail to overwrite reaches the UI as-is.
        let s = mock_snapshot(7);
        assert_eq!(s.btc.price, 0.0, "a fabricated BTC price");
        assert!(s.btc.sparkline.is_empty(), "a fabricated sparkline");
        assert_eq!(s.btc.change_pct, 0.0);
        assert!(!s.signal.connected);
        assert!(matches!(s.membership, MembershipState::Unknown));
        assert!(s.generated_at > 1_780_000_000_000, "generated_at is not now");
    }
}
