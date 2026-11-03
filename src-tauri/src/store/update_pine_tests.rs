//! Update-persistence proof for the Pine bot (spec section 11): every data
//! dir an older release wrote (`fixtures/update*/<release>/`, without the
//! `pine_actions` table and without any `pine` field) opens with this
//! build: the table is added empty, every saved bot config still reads (as
//! a config without Pine settings), no Pine bot appears, and the new table
//! takes its first key.

use std::path::{Path, PathBuf};

use crate::bot::model::{BotConfig, BotKind};
use crate::store::pine::Claim;
use crate::store::update_common::assert_scratch_dir;
use crate::store::StoreManager;

fn fixtures() -> Vec<PathBuf> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/store/fixtures");
    let mut v = Vec::new();
    for group in ["update", "update_live"] {
        if let Ok(rd) = std::fs::read_dir(base.join(group)) {
            v.extend(rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.join("edge.db").exists()));
        }
    }
    v.sort();
    assert!(!v.is_empty(), "no update fixtures");
    v
}

fn stage(fx: &Path) -> PathBuf {
    let tmp = std::env::temp_dir().join(format!("ae-upd-pine-{}", uuid::Uuid::new_v4()));
    assert_scratch_dir(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::copy(fx.join("edge.db"), tmp.join("edge.db")).unwrap();
    tmp
}

#[test]
fn an_older_store_without_the_pine_table_or_fields_loads() {
    for fx in fixtures() {
        let name = fx.display().to_string();
        // The old file really has no Pine table.
        {
            let c = rusqlite::Connection::open(fx.join("edge.db")).unwrap();
            let n: i64 = c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = 'pine_actions'", [], |r| r.get(0)).unwrap();
            assert_eq!(n, 0, "[{name}] fixture already has the table");
        }
        let dir = stage(&fx);
        let store = StoreManager::new();
        assert_eq!(store.open_error(&dir), None, "[{name}] the store opens");
        assert!(store.pine_results(&dir, 10).unwrap().is_empty(), "[{name}] the table is added empty");
        assert_eq!(store.pine_opens_since(&dir, 0).unwrap(), 0);
        assert!(store.pine_pending(&dir).unwrap().is_empty());
        assert_eq!(store.get_meta(&dir, crate::bot::pinebot::HALT_KEY).unwrap(), None, "no halt left over");
        // Every saved signal-bot config still reads, as before, with no Pine settings.
        let mut read = 0;
        for kind in crate::bot::ALL_KINDS {
            if let Some(raw) = store.get_meta(&dir, &format!("bot_config.{}", kind.as_str())).unwrap() {
                let cfg: BotConfig = serde_json::from_str(&raw).unwrap_or_else(|e| panic!("[{name}] {kind:?}: {e}"));
                assert!(cfg.pine.is_none(), "[{name}] {kind:?}");
                read += 1;
            }
        }
        assert!(read >= 1, "[{name}] the saved bot configs");
        // The restore reads them all and makes no Pine bot.
        let plan = crate::bot::restore::plan(&store, &dir);
        assert!(plan.configs.iter().all(|c| c.kind != BotKind::Pine), "[{name}]");
        assert!(plan.notes.iter().all(|n| n.key != "restoreRowUnreadable"), "[{name}] {:?}", plan.notes.iter().map(|n| n.key).collect::<Vec<_>>());
        // The new table works on the migrated file, and survives a reopen.
        let claim = Claim { key: "pine:h:BTCUSDT:1", script_hash: "h", symbol: "BTCUSDT", bar_close_ms: 1, now_ms: 2, body: "{}" };
        assert!(store.pine_claim(&dir, &claim).unwrap());
        drop(store);
        let again = StoreManager::new();
        assert!(!again.pine_claim(&dir, &claim).unwrap(), "[{name}] claimed once across a restart");
        drop(again);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
