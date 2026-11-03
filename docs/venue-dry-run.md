# Venue dry run

There are 15 venues: 8 with an order path, each enabled for live only after this run passes (Binance is verified), and 7 with price data and paper trading, which have no order path to run. Hyperliquid is one of the 7 in 0.3.22: its orders are signed with secp256k1, and the crate ccxt uses for that (k256 0.13 on crypto-bigint 0.5.5) has an open timing advisory, RUSTSEC-2026-0387, with no fixed release. It returns as an order venue when one exists. Real money stays off on an order venue until this run passes on that venue's own test network (`DRY_RUN_PASSED` in `src-tauri/src/exchange/orders.rs`). The run is the ignored test `testnet_dry_run` in `src-tauri/src/exchange/testnet_tests.rs`. It never reaches a real account. Binance sends to its futures testnet. Every other order venue runs with `ALEPH_EDGE_VENUE_SANDBOX=1`, which the test sets itself before the first client exists. Any other id is refused, so nothing goes to production.

`cargo test --lib dry_run_plan -- --nocapture` prints the plan below from the code.

## Command (one per venue)

Run from `src-tauri/`. Use a bash shell (Git Bash):

```bash
ALEPH_TESTNET_VENUE=<venue id> ALEPH_TESTNET_KEY=... ALEPH_TESTNET_SECRET=... [ALEPH_TESTNET_PASSPHRASE=...] \
  cargo test --lib testnet_dry_run -- --ignored --nocapture
```

| Variable | Needed for | Value |
|---|---|---|
| `ALEPH_TESTNET_VENUE` | all | a venue id from the table below |
| `ALEPH_TESTNET_KEY` | all | test-network API key |
| `ALEPH_TESTNET_SECRET` | all | test-network API secret |
| `ALEPH_TESTNET_PASSPHRASE` | OKX, Bitget | the passphrase set when the key was created |

Never put these values in a file in the repository or in shell history that is synced. Without a key the test prints `skipped` and passes.

## Per venue

| Venue id | Test network | Key fields | Protective stop | Breakeven move |
|---|---|---|---|---|
| `binance` | Futures testnet (testnet.binancefuture.com). Passed 2026-10-05 | key + secret | closePosition stop | quantity stop beside, old one cancelled |
| `bybit` | testnet (api-testnet.bybit.com) | key + secret | position stop (trading-stop) | in place |
| `okx` | demo trading (`x-simulated-trading: 1`) | key + secret + passphrase | algo, `closeFraction=1` | in place (amend-algos) |
| `bitget` | demo trading (demo API key, `paptrading` header) | key + secret + passphrase | position stop-loss (`pos_loss`) | new stop, read back, old cancelled |
| `gate` | testnet (api-testnet.gateapi.io) | key + secret | reduce-only stop, full size, re-synced | new stop, read back, old cancelled |
| `bingx` | VST demo (open-api-vst.bingx.com) | key + secret | `STOP_MARKET` with `closePosition` | new stop, read back, old cancelled |
| `woo` | staging (api.staging.woox.io) | key + secret | reduce-only stop, full size, re-synced | new stop, read back, old cancelled |
| `deribit` | testnet (test.deribit.com) | client id + client secret | reduce-only stop, full size, re-synced | new stop, read back, old cancelled |

A "re-synced" stop is placed for the position's full size and replaced at the same level after every add or reduce the app makes. A position changed outside the app is not followed until the app's next order on that symbol.

Expected failure that is not a bug: Deribit has no per-position isolated margin, so its "isolated margin" row fails and the venue cannot go live; account read and closing still work there.

## Keys

Create a test-network key with trading rights only: no withdrawal, no transfer. Give each test account about 1,000 test USDT (USDC on Deribit).

| Venue | Where | Notes |
|---|---|---|
| OKX | okx.com → Trade → Demo trading → Personal center → Demo Trading API | Read + Trade. Needs the passphrase. Account mode must allow isolated futures margin. |
| Bybit | testnet.bybit.com → API → System-generated key | Contract / Derivatives: Orders + Positions. Must be a TESTNET key; a "Demo Trading" key from the main site does not work. Test USDT from the faucet. |
| Bitget | bitget.com → Demo trading → create a Demo API key | Futures order + position rights. Passphrase required. Bitget asks for KYC before demo keys. |
| Gate | gate.com → API Management → create key with account type "Testnet" | Perpetual futures read-write; wallet read-only; withdrawal off. Real keys do not work on the testnet. |
| BingX | bingx.com → API Management (main account), demo funds via the VST faucet | Perpetual futures trading on; withdrawals and transfers off. Whether a main-site key works on the VST host is not confirmed: the run's first row shows it. |
| WOO X | WOO X staging site (separate account) | Trading on, withdrawal and transfer off. Where staging keys are made is not documented: ask WOO X support if the staging site gives none. |
| Deribit | test.deribit.com → Account → API → add key | Scopes: trade read-write, wallet read only (wallet read-write is refused: it can withdraw). Separate account from the main site. |

Before the run, on the test account:

- one-way position mode (OKX "Net mode", Bitget "one-way", Gate "single", BingX one-way, WOO X ONE_WAY);
- no position and no open order on BTCUSDT and ETHUSDT (the run refuses to start otherwise and touches nothing);
- enough free margin: OKX and Gate trade whole contracts, so a leg can be larger than the 130 USDT used elsewhere.

## What it checks

It prints one line per step as it goes, then a table. Every row must read PASS:

1. account read, flat start on both symbols, one-way mode
2. symbol rules (trading, lot step, tick, minimum notional), isolated margin, leverage 2x
3. market entry with the signal bot's client id, position read back
4. entry found by client id; a lost entry recognised as ours from the closed-order history (reconcile)
5. stop placed and read back live
6. breakeven move as the engine makes it on this venue (the table's last column), with the new level read back
7. market add and reduce (on re-synced venues the stop follows the new size)
8. stop cancelled by id, then reads CANCELED
9. confirmed close: account reads flat
10. cancel sweep, then no orders left
11. closed-order history holds our entry client id (FILLED) and reduce-only closes
12. Close all over BTCUSDT and ETHUSDT, each with a stop: both flat, no orders left

Whatever fails, it then flattens and sweeps both symbols. If the cleanup line reads FAIL, close what is left on the exchange's website.

The line `INFO open orders after the move` is not a pass/fail row. Write down what it says (whether a position stop counts as an open order on that venue).

## After a PASS

Add the venue to `DRY_RUN_PASSED` (orders.rs) and to the fallback in `src/lib/venues.ts`. Update the tests that pin the list (`the_dry_run_list_is_binance_alone_today`, `only_binance_reads_verified_today`, `venues.test.ts`), the README venue table and the FAQ line "Which exchanges can place real orders?". That is a release decision: keep the run's table with the release notes.
