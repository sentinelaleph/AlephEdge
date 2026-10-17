//! App-level surface: the composition helpers that aren't a domain subsystem.
//! `commands` (health snapshot + version IPC), `cockpit` (the top-bar
//! connectivity checks), and `health` (the aggregated health model) live here
//! so the crate root stays at its two entry files (lib.rs, main.rs).

pub mod about;
pub mod backup;
pub mod before_you_trade;
pub mod cockpit;
pub mod commands;
pub mod endpoints;
pub mod foreign_fill;
pub mod health;
pub mod instance;
pub mod keychain;
pub mod market_trend;
pub mod preflight;
pub mod presets_live;
pub mod settings_transfer;
pub mod update;
pub mod window;
