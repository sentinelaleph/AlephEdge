//! What the signal bots' restore decides from the store, Tauri-free: the
//! saved configs (Paper / Live mode kept, the arm never), the open-position
//! book, the one-time mapping of a LIVE build's data to Live bots, and the
//! real-money rows the book must not manage. `BotManager::restore` applies
//! the plan; the update tests run the same function on old data dirs.

use std::collections::HashSet;
use std::path::Path;

use crate::store::StoreManager;

use super::engine::book::{self, Note};
use super::model::{BotConfig, BotKind, OpenPosition, TradeMode};
use super::{config_key, mapped_mode, split_foreign_live_rows};

/// Set once the first 0.3.19 start mapped data written before modes existed
/// (a LIVE build's real-money bots) to Live bots. Never mapped again: a bot
/// the user later made Paper stays Paper.
pub const MODE_MAP_KEY: &str = "trade_mode_mapped";

const KINDS: [BotKind; 4] = super::ALL_KINDS;

/// The restore's decisions.
#[derive(Debug, Default)]
pub struct SignalRestore {
    /// Every readable saved config, mode mapped, arm cleared.
    pub configs: Vec<BotConfig>,
    /// The book: paper rows, and real rows of Live bots.
    pub positions: Vec<OpenPosition>,
    /// Real-money rows the book does not manage ("kind signal_id"):
    /// unreadable ones and real rows on a Paper bot. Money in play.
    pub held_labels: Vec<String>,
    /// Bots not resumed: a real row of theirs is held.
    pub blocked: HashSet<BotKind>,
    /// Bots the one-time mapping made Live now.
    pub mapped: Vec<BotKind>,
    pub notes: Vec<Note>,
}

/// Reads and decides. Store writes: a settled row's delete (book.rs), and
/// the mapping's saved configs and its once-key.
pub fn plan(store: &StoreManager, dir: &Path) -> SignalRestore {
    let mut out = SignalRestore::default();
    let stored_live = read_configs(store, dir, &mut out);
    let restored = book::read_book(store, dir, &mut out.notes);
    let mut real: HashSet<BotKind> = restored.positions.iter().filter(|p| p.live).map(|p| p.bot_kind).collect();
    real.extend(restored.live_unreadable.iter().copied());
    map_modes_once(store, dir, &stored_live, &real, &mut out);
    // After the mapping only a Live bot holds real rows (a bot goes back to
    // Paper only once flat). A real row on a Paper bot is a line in a file,
    // not an order this app made: it is held out of the book (no close,
    // manage or reconcile reaches the venue for it), left in the store,
    // named, and counted as money in play.
    let mode_of = |k: BotKind| out.configs.iter().find(|c| c.kind == k).map_or(TradeMode::Paper, |c| c.mode);
    let (kept, held) = split_foreign_live_rows(restored.positions, mode_of);
    out.held_labels = restored.unreadable_labels;
    out.blocked = restored.live_unreadable;
    for p in &held {
        let label = format!("{} {}", p.bot_kind.as_str(), p.signal_id);
        out.notes.push(Note::new(&p.symbol, "liveRowOnPaperBot", Some(label.clone())));
        out.held_labels.push(label);
        out.blocked.insert(p.bot_kind);
    }
    out.positions = kept;
    out
}

/// The saved configs. The Paper / Live mode is restored; the arm (`live`)
/// never is: real-money trading is re-confirmed every session, never
/// re-armed from a file on disk. A config that parses but fails today's
/// validation is still loaded, so restored positions keep the bot's
/// max-loss; `start` refuses it until it is saved again, and the feed says
/// why. Returns the kinds whose stored config said `live` (a LIVE build
/// saved it while the bot traded real money).
fn read_configs(store: &StoreManager, dir: &Path, out: &mut SignalRestore) -> HashSet<BotKind> {
    let mut stored_live = HashSet::new();
    for kind in KINDS {
        let body = match store.get_meta(dir, &config_key(kind)) {
            Ok(Some(body)) => body,
            Ok(None) => continue,
            Err(e) => {
                out.notes.push(Note::new("—", "storeReadFailed", Some(e)));
                continue;
            }
        };
        match serde_json::from_str::<BotConfig>(&body) {
            Ok(mut cfg) if cfg.kind == kind => {
                if cfg.live {
                    stored_live.insert(kind);
                }
                cfg.live = false;
                if let Err(code) = cfg.validate() {
                    out.notes.push(Note::new("—", "restoreConfigInvalid", Some(code)));
                }
                out.configs.push(cfg);
            }
            _ => out.notes.push(Note::new("—", "restoreRowUnreadable", Some(format!("{} config", kind.as_str())))),
        }
    }
    stored_live
}

/// First start of a build with modes: a bot a LIVE build ran on real money
/// (its saved config said `live`, or it holds a real position row, readable
/// or not) becomes a Live bot, stopped until its typed LIVE; the change is
/// saved. Every other bot stays Paper. Runs once (`MODE_MAP_KEY`); a store
/// that cannot say whether it ran maps nothing and tries again next start.
fn map_modes_once(store: &StoreManager, dir: &Path, stored_live: &HashSet<BotKind>, real: &HashSet<BotKind>, out: &mut SignalRestore) {
    match store.get_meta(dir, MODE_MAP_KEY) {
        Ok(None) => {}
        Ok(Some(_)) => return,
        Err(e) => return out.notes.push(Note::new("—", "storeReadFailed", Some(e))),
    }
    for cfg in out.configs.iter_mut() {
        let mode = mapped_mode(cfg.mode, stored_live.contains(&cfg.kind), real.contains(&cfg.kind));
        if mode == cfg.mode {
            continue;
        }
        cfg.mode = mode;
        let saved = serde_json::to_string(&*cfg)
            .map_err(|e| e.to_string())
            .and_then(|body| store.set_meta(dir, &config_key(cfg.kind), &body));
        if let Err(e) = saved {
            out.notes.push(Note::new("—", "storeWriteFailed", Some(e)));
        }
        out.notes.push(Note::new("—", "liveBotMapped", Some(cfg.kind.as_str().to_string())));
        out.mapped.push(cfg.kind);
    }
    if let Err(e) = store.set_meta(dir, MODE_MAP_KEY, "1") {
        out.notes.push(Note::new("—", "storeWriteFailed", Some(e)));
    }
}
