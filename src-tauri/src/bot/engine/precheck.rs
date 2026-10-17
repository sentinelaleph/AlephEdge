//! Pre-trade checks. Every decision is made by the pure rules in
//! `precheck_rules.rs`, which the headless bot farm
//! compile too; the pipeline that runs them in order (membership, validity,
//! Sentinel status, funding, depth, budget) is `pipeline::precheck_with`, fed
//! by the desktop's managers in `entry.rs`. Re-exported here so
//! `precheck::Skip`, `precheck::is_final` etc. keep their paths.

pub use super::precheck_rules::*;
