# Venue dry run (Bybit, OKX)

Real money stays off on a venue until this run passes on that venue's own test network (`DRY_RUN_PASSED` in `src-tauri/src/exchange/orders.rs`). The run is the ignored test `testnet_dry_run` in `src-tauri/src/exchange/testnet_tests.rs`. It never reaches a real account. Binance sends to its futures testnet. Bybit and OKX run with `ALEPH_EDGE_VENUE_SANDBOX=1`, which the test sets itself before the first client exists.

## Command (one per venue)

Run from `src-tauri/`. Use a bash shell (Git Bash):

```bash
# OKX demo trading
ALEPH_TESTNET_VENUE=okx ALEPH_TESTNET_KEY=... ALEPH_TESTNET_SECRET=... ALEPH_TESTNET_PASSPHRASE=... \
  cargo test --features live --lib testnet_dry_run -- --ignored --nocapture

# Bybit testnet
ALEPH_TESTNET_VENUE=bybit ALEPH_TESTNET_KEY=... ALEPH_TESTNET_SECRET=... \
  cargo test --features live --lib testnet_dry_run -- --ignored --nocapture

# Binance futures testnet (the reference run, passed 2026-10-05)
ALEPH_TESTNET_VENUE=binance ALEPH_TESTNET_KEY=... ALEPH_TESTNET_SECRET=... \
  cargo test --features live --lib testnet_dry_run -- --ignored --nocapture
```

| Variable | Needed for | Value |
|---|---|---|
| `ALEPH_TESTNET_VENUE` | all | `okx`, `bybit` or `binance` |
| `ALEPH_TESTNET_KEY` | all | test-network API key |
| `ALEPH_TESTNET_SECRET` | all | test-network API secret |
| `ALEPH_TESTNET_PASSPHRASE` | OKX | the passphrase set when the demo key was created |

Never put these values in a file in the repository or in shell history that is synced. Without a key the test prints `skipped` and passes.

## Keys

| Venue | Where | Permissions | Notes |
|---|---|---|---|
| OKX | okx.com → Trade → Demo trading → Personal center → Demo Trading API | Read + Trade. No Withdraw. | Needs the passphrase. An IP allowlist is optional; if you set one, add the public IP of the machine that runs the test. |
| Bybit | testnet.bybit.com → API → System-generated key | Read-Write. Contract / Derivatives: Orders + Positions. No Withdraw, no Transfer. | Must be a TESTNET key (api-testnet.bybit.com). A "Demo Trading" key from the main site does not work here. Without an IP allowlist Bybit expires the key after 3 months. Get test USDT from the testnet faucet. |

Before the run, on the test account:

- one-way position mode (OKX: "Net mode"; Bybit: one-way for USDT perpetuals);
- no position and no open order on BTCUSDT and ETHUSDT (the run refuses to start otherwise and touches nothing);
- about 1,000 test USDT free. OKX trades whole contracts (0.01 BTC, 0.1 ETH), so the BTC leg is larger than the 130 USDT used elsewhere.
- OKX: an account mode that allows isolated futures margin (Futures mode or above, not Spot mode).

## What it checks

It prints one line per step as it goes, then a table. Every row must read PASS:

1. account read, flat start on both symbols, one-way mode
2. symbol rules (trading, lot step, tick, minimum notional), isolated margin, leverage 2x
3. market entry with the signal bot's client id, position read back
4. entry found by client id; a lost entry recognised as ours from the closed-order history (reconcile)
5. stop placed and read back live
6. breakeven move as the engine makes it: Bybit / OKX change the stop in place (Bybit trading-stop, OKX amend-algos) and read the new level back; Binance places a quantity stop beside and cancels the old one
7. market add and reduce
8. stop cancelled by id, then reads CANCELED (OKX: cancel-algos by our algo client id)
9. confirmed close: account reads flat
10. cancel sweep, then no orders left
11. closed-order history holds our entry client id (FILLED) and reduce-only closes
12. Close all over BTCUSDT and ETHUSDT, each with a stop: both flat, no orders left

Whatever fails, it then flattens and sweeps both symbols. If the cleanup line reads FAIL, close what is left on the exchange's website.

The line `INFO open orders after the move` is not a pass/fail row. Write down what it says for Bybit (whether a position stop counts as an open order).

## After a PASS

Add the venue to `DRY_RUN_PASSED` (orders.rs) and to the fallback in `src/lib/venues.ts`. Update the two tests that pin the list (`the_dry_run_list_is_binance_alone_today`, `venues.test.ts`) and the FAQ line "Which exchanges can place real orders?". That is a release decision: keep the run's table with the release notes.
