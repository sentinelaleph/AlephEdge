//! Trade-table SQL: insert, list, aggregate stats, CSV export. Pure functions
//! over a `rusqlite::Connection` — the connection lifecycle lives in `mod.rs`.

use rusqlite::{params, Connection};

use super::model::{PnlStats, TradeRecord};

pub fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS trades (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            signal_id TEXT NOT NULL,
            bot_kind TEXT NOT NULL,
            exchange_id TEXT NOT NULL DEFAULT 'binance',
            symbol TEXT NOT NULL,
            direction TEXT NOT NULL,
            entry REAL NOT NULL,
            exit REAL NOT NULL,
            leverage INTEGER NOT NULL,
            capital REAL NOT NULL,
            pnl_pct REAL NOT NULL,
            pnl_quote REAL NOT NULL,
            exit_reason TEXT NOT NULL,
            fr_at_open REAL,
            ld_at_open REAL,
            opened_at INTEGER NOT NULL,
            closed_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_trades_closed_at ON trades (closed_at DESC);",
    )
    .map_err(|_| "storeMigrationFailed".to_string())?;
    // Older DBs (F3 shipped the table before exchange_id existed) get the
    // column added here. On a fresh DB the column already exists and this
    // ALTER errors — that's expected and ignored.
    let _ = conn.execute(
        "ALTER TABLE trades ADD COLUMN exchange_id TEXT NOT NULL DEFAULT 'binance'",
        [],
    );
    // Same pattern for the ledger-parity column; nullable so old rows stay valid.
    let _ = conn.execute("ALTER TABLE trades ADD COLUMN unlevered_net_pct REAL", []);
    // Live execution columns: paper rows stay live = 0 with NULL fills.
    let _ = conn.execute(
        "ALTER TABLE trades ADD COLUMN live INTEGER NOT NULL DEFAULT 0",
        [],
    );
    let _ = conn.execute("ALTER TABLE trades ADD COLUMN fill_entry REAL", []);
    let _ = conn.execute("ALTER TABLE trades ADD COLUMN fill_exit REAL", []);
    let _ = conn.execute("ALTER TABLE trades ADD COLUMN commission_usdt REAL", []);
    // Sizing columns: rows from before sizing are fixed-mode with NULL size.
    let _ = conn.execute(
        "ALTER TABLE trades ADD COLUMN sizing_mode TEXT NOT NULL DEFAULT 'fixed'",
        [],
    );
    let _ = conn.execute("ALTER TABLE trades ADD COLUMN risk_pct REAL", []);
    let _ = conn.execute("ALTER TABLE trades ADD COLUMN notional_usdt REAL", []);
    let _ = conn.execute("ALTER TABLE trades ADD COLUMN effective_leverage REAL", []);
    let _ = conn.execute(
        "ALTER TABLE trades ADD COLUMN risk_capped INTEGER NOT NULL DEFAULT 0",
        [],
    );
    // Veto columns (2026-09-18): NULL on every row but a "veto" exit.
    let _ = conn.execute("ALTER TABLE trades ADD COLUMN veto_reason_code TEXT", []);
    let _ = conn.execute("ALTER TABLE trades ADD COLUMN veto_reason_text TEXT", []);
    // Take-profit target (2026-09-22): NULL on older rows (all TP1).
    let _ = conn.execute("ALTER TABLE trades ADD COLUMN tp_target TEXT", []);
    let _ = conn.execute("ALTER TABLE trades ADD COLUMN tp_fallback_from TEXT", []);
    Ok(())
}

pub fn insert(conn: &Connection, t: &TradeRecord) -> Result<(), String> {
    conn.execute(
        "INSERT INTO trades (signal_id, bot_kind, exchange_id, symbol, direction, entry, exit,
            leverage, capital, pnl_pct, pnl_quote, exit_reason, fr_at_open, ld_at_open, opened_at, closed_at,
            unlevered_net_pct, live, fill_entry, fill_exit, commission_usdt, sizing_mode, risk_pct,
            notional_usdt, effective_leverage, risk_capped, veto_reason_code, veto_reason_text,
            tp_target, tp_fallback_from)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
            ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30)",
        params![
            t.signal_id, t.bot_kind, t.exchange_id, t.symbol, t.direction, t.entry, t.exit,
            t.leverage, t.capital, t.pnl_pct, t.pnl_quote, t.exit_reason, t.fr_at_open,
            t.ld_at_open, t.opened_at, t.closed_at, t.unlevered_net_pct, t.live, t.fill_entry,
            t.fill_exit, t.commission_usdt, t.sizing_mode, t.risk_pct, t.notional_usdt,
            t.effective_leverage, t.risk_capped, t.veto_reason_code, t.veto_reason_text,
            t.tp_target, t.tp_fallback_from
        ],
    )
    .map(|_| ())
    .map_err(|_| "storeWriteFailed".to_string())
}

/// (signal_id, bot_kind) of every trade opened at or after `since_ms`: the
/// signals a bot already traded, so a restart never enters one again.
pub fn recent_keys(conn: &Connection, since_ms: u64) -> Result<Vec<(String, String)>, String> {
    let mut stmt = conn
        .prepare("SELECT DISTINCT signal_id, bot_kind FROM trades WHERE opened_at >= ?1")
        .map_err(|_| "storeQueryFailed".to_string())?;
    let rows = stmt
        .query_map(params![since_ms as i64], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|_| "storeQueryFailed".to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|_| "storeQueryFailed".to_string())
}

pub fn list(conn: &Connection, limit: u32) -> Result<Vec<TradeRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, signal_id, bot_kind, exchange_id, symbol, direction, entry, exit, leverage,
                    capital, pnl_pct, pnl_quote, exit_reason, fr_at_open, ld_at_open, opened_at, closed_at,
                    unlevered_net_pct, live, fill_entry, fill_exit, commission_usdt,
                    sizing_mode, risk_pct, notional_usdt, effective_leverage, risk_capped,
                    veto_reason_code, veto_reason_text, tp_target, tp_fallback_from
             FROM trades ORDER BY closed_at DESC LIMIT ?1",
        )
        .map_err(|_| "storeQueryFailed".to_string())?;
    let rows = stmt
        .query_map(params![limit], |r| {
            Ok(TradeRecord {
                id: r.get(0)?,
                signal_id: r.get(1)?,
                bot_kind: r.get(2)?,
                exchange_id: r.get(3)?,
                symbol: r.get(4)?,
                direction: r.get(5)?,
                entry: r.get(6)?,
                exit: r.get(7)?,
                leverage: r.get(8)?,
                capital: r.get(9)?,
                pnl_pct: r.get(10)?,
                pnl_quote: r.get(11)?,
                exit_reason: r.get(12)?,
                fr_at_open: r.get(13)?,
                ld_at_open: r.get(14)?,
                opened_at: r.get(15)?,
                closed_at: r.get(16)?,
                unlevered_net_pct: r.get(17)?,
                live: r.get(18)?,
                fill_entry: r.get(19)?,
                fill_exit: r.get(20)?,
                commission_usdt: r.get(21)?,
                sizing_mode: r.get(22)?,
                risk_pct: r.get(23)?,
                notional_usdt: r.get(24)?,
                effective_leverage: r.get(25)?,
                risk_capped: r.get(26)?,
                veto_reason_code: r.get(27)?,
                veto_reason_text: r.get(28)?,
                tp_target: r.get(29)?,
                tp_fallback_from: r.get(30)?,
                // Not stored twice: identical to pnl_quote / pnl_pct by
                // construction, so derived on read (legacy rows included).
                pnl_usdt: r.get(11)?,
                pnl_pct_of_capital: r.get(10)?,
            })
        })
        .map_err(|_| "storeQueryFailed".to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| "storeRowMalformed".to_string())
}

/// Aggregates in one pass over closed trades (oldest → newest for drawdown).
pub fn stats(conn: &Connection, utc_day_start_ms: u64) -> Result<PnlStats, String> {
    stats_scoped(conn, utc_day_start_ms, None)
}

/// `stats` over real trades only (`Some(true)`), simulated only
/// (`Some(false)`), or both (`None`). Real and simulated money must never
/// share one total: the live daily stop reads real losses only.
pub fn stats_scoped(
    conn: &Connection,
    utc_day_start_ms: u64,
    live: Option<bool>,
) -> Result<PnlStats, String> {
    let filter = match live {
        Some(true) => " WHERE live = 1",
        Some(false) => " WHERE live = 0",
        None => "",
    };
    let mut stmt = conn
        .prepare(&format!(
            "SELECT pnl_quote, pnl_pct, exit_reason, closed_at FROM trades{filter} ORDER BY closed_at ASC"
        ))
        .map_err(|_| "storeQueryFailed".to_string())?;
    let rows: Vec<(f64, f64, String, u64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .map_err(|_| "storeQueryFailed".to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "storeRowMalformed".to_string())?;

    let (mut wins, mut losses) = (0u32, 0u32);
    let (mut gross_profit, mut gross_loss) = (0f64, 0f64);
    let (mut net, mut today, mut sum_pct) = (0f64, 0f64, 0f64);
    let (mut peak, mut max_dd) = (0f64, 0f64);
    for (pnl_quote, pnl_pct, reason, closed_at) in &rows {
        net += pnl_quote;
        sum_pct += pnl_pct;
        if *closed_at >= utc_day_start_ms {
            today += pnl_quote;
        }
        // Qualified winrate counts only genuine TP/SL resolutions (PRD §5.6).
        match (reason.as_str(), *pnl_quote >= 0.0) {
            ("tp", _) => wins += 1,
            ("sl", _) => losses += 1,
            _ => {}
        }
        if *pnl_quote >= 0.0 {
            gross_profit += pnl_quote;
        } else {
            gross_loss += -pnl_quote;
        }
        peak = peak.max(net);
        max_dd = max_dd.max(peak - net);
    }

    let resolved = wins + losses;
    Ok(PnlStats {
        total_trades: rows.len() as u32,
        wins,
        losses,
        win_rate: (resolved > 0).then(|| f64::from(wins) / f64::from(resolved)),
        net_pnl_quote: net,
        avg_pnl_pct: if rows.is_empty() {
            0.0
        } else {
            sum_pct / rows.len() as f64
        },
        profit_factor: (gross_loss > 0.0 && gross_profit > 0.0).then(|| gross_profit / gross_loss),
        max_drawdown_quote: max_dd,
        today_pnl_quote: today,
    })
}

/// One CSV text field (RFC 4180). Free text reaches this file — veto reasons
/// come from Sentinel verbatim — and a bare comma in one shifted every column
/// after it, so the export no longer lined up with its header. A field that
/// opens with a formula character is prefixed with `'` so a spreadsheet shows
/// it as text instead of evaluating it.
fn csv_field(raw: &str) -> String {
    let guarded = if raw.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{raw}")
    } else {
        raw.to_string()
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

/// CSV of all trades (PRD §5.6 one-click export). Returns the CSV body.
pub fn export_csv(conn: &Connection) -> Result<String, String> {
    let trades = list(conn, u32::MAX)?;
    let mut out = String::from(
        "id,signal_id,bot_kind,exchange_id,symbol,direction,entry,exit,leverage,capital,pnl_pct,pnl_quote,exit_reason,fr_at_open,ld_at_open,opened_at,closed_at,unlevered_net_pct,live,fill_entry,fill_exit,commission_usdt,sizing_mode,risk_pct,notional_usdt,effective_leverage,risk_capped,veto_reason_code,veto_reason_text,tp_target,tp_fallback_from\n",
    );
    let opt = |v: Option<f64>| v.map(|v| v.to_string()).unwrap_or_default();
    let opt_s = |v: &Option<String>| csv_field(v.as_deref().unwrap_or_default());
    for t in trades {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            t.id,
            csv_field(&t.signal_id),
            csv_field(&t.bot_kind),
            csv_field(&t.exchange_id),
            csv_field(&t.symbol),
            csv_field(&t.direction),
            t.entry,
            t.exit,
            t.leverage,
            t.capital,
            t.pnl_pct,
            t.pnl_quote,
            csv_field(&t.exit_reason),
            opt(t.fr_at_open),
            opt(t.ld_at_open),
            t.opened_at,
            t.closed_at,
            opt(t.unlevered_net_pct),
            t.live,
            opt(t.fill_entry),
            opt(t.fill_exit),
            opt(t.commission_usdt),
            csv_field(&t.sizing_mode),
            opt(t.risk_pct),
            opt(t.notional_usdt),
            opt(t.effective_leverage),
            t.risk_capped,
            opt_s(&t.veto_reason_code),
            opt_s(&t.veto_reason_text),
            opt_s(&t.tp_target),
            opt_s(&t.tp_fallback_from)
        ));
    }
    Ok(out)
}
