//! App-level surface: the composition helpers that aren't a domain subsystem.
//! `commands` (health snapshot + version IPC), `cockpit` (the top-bar
//! connectivity checks), and `health` (the aggregated health model) live here
//! so the crate root stays at its two entry files (lib.rs, main.rs).

pub mod cockpit;
pub mod commands;
pub mod endpoints;
pub mod health;
