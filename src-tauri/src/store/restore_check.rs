//! Stored rows that restore cannot use. `strategy::load_bots` and
//! `strategy::load_open_cycles` skip a row that no longer parses, which made
//! a bot vanish, or let it resume and open a new cycle over the open one it
//! lost, without a word. Restore asks this module for those rows, names
//! them on screen and keeps the affected bots stopped (fail closed: the rows
//! stay on disk untouched for a build that can read them).

use rusqlite::Connection;

use crate::bot::strategy::cycle::CycleState;
use crate::bot::strategy::model::StrategyConfig;

#[derive(Debug, Default, PartialEq)]
pub struct UnreadableRows {
    /// Bots (not archived) whose `config_json` no longer parses.
    pub configs: Vec<String>,
    /// Open cycles, (bot id, seq), whose state is missing or no longer parses.
    pub cycles: Vec<(String, u32)>,
}

impl UnreadableRows {
    pub fn is_empty(&self) -> bool {
        self.configs.is_empty() && self.cycles.is_empty()
    }
}

fn err(e: rusqlite::Error) -> String {
    e.to_string()
}

pub fn unreadable_rows(conn: &Connection) -> Result<UnreadableRows, String> {
    let mut out = UnreadableRows::default();
    let mut st = conn
        .prepare("SELECT id, config_json FROM strategy_bots WHERE archived_at IS NULL ORDER BY created_at")
        .map_err(err)?;
    let rows = st
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(err)?;
    for row in rows {
        let (id, cfg) = row.map_err(err)?;
        if serde_json::from_str::<StrategyConfig>(&cfg).is_err() {
            out.configs.push(id);
        }
    }
    let mut st = conn
        .prepare(
            "SELECT c.bot_id, c.seq, c.state_json FROM strategy_cycles c
             JOIN strategy_bots b ON b.id = c.bot_id
             WHERE c.closed_at IS NULL AND b.archived_at IS NULL ORDER BY c.bot_id, c.seq",
        )
        .map_err(err)?;
    let rows = st
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?, r.get::<_, Option<String>>(2)?))
        })
        .map_err(err)?;
    for row in rows {
        let (id, seq, state) = row.map_err(err)?;
        let readable = state.is_some_and(|s| serde_json::from_str::<CycleState>(&s).is_ok());
        if !readable {
            out.cycles.push((id, seq));
        }
    }
    Ok(out)
}
