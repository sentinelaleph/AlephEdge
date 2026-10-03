# Aleph Edge

A trading desk that runs on your own machine.

Aleph Edge subscribes to signals from [Sentinel Aleph](https://ribqa.com),
applies your risk limits, and manages positions on your exchange account. It is
a Rust + Tauri 2 desktop application with a React front end. **Your exchange API
keys never leave your device** — they are held in a local encrypted vault, and
there is no server component that could hold them for you.

**Automated** order submission is **off**, behind a compile-time constant
(`LIVE_TRADING_ENABLED = false`). No bot in this build opens, manages or closes
a position on the exchange: entries, exits, breakeven moves and reconciliation
all run in simulation against the live market price. Turning that on is a
source change, not a settings toggle, and that is deliberate.

Two controls are the exception, and they are the reason this paragraph is
worded carefully: **"Close position" and "Close all" send real reduce-only
market orders in every build.** They are manual, they sit behind a two-step
confirmation in the UI, and they exist so you can flatten an account the desk
is watching without leaving the app. They are the only path from this
application to a real order, and they are yours to press.

---

## What this is not

**Not an exchange client you log into.** There is no Aleph Edge account holding
your funds. The desk talks to your exchange with your own API keys, from your
own machine.

**Not a signal service with a track record to sell you.** This repository makes
no win-rate claim, and you should be suspicious of the ones that do — see
[Honesty](#honesty) below.

**Not tested in the field.** Nobody has run this against a real account for a
sustained period — not us. The simulation path has been exercised, the live
order path has been reviewed and unit-tested, and that is all. Treat every
build here as software that has never met a real market day. If you point it at
an exchange key, point it at one that can lose nothing you mind losing.

**Not audited by anyone independent.** It went through one adversarial review
(2026-09-20) that found four real defects; the ones that could move money were
fixed before this repository was published, and the rest are in the open. It
handles exchange credentials. Read the vault code before you trust it with a
key that can trade.

---

## Honesty

The system this desk consumes has been measured on its own history, and the
measurements are not flattering. They are published here because a reader will
find them anyway, and finding them in the README is better than finding them in
a backtest.

**Entry selection carries no measurable information.** Three independent tests,
three nulls:

| test | what it asks | result |
|---|---|---|
| AUC | does the score separate winners from losers? | **0.496** (0.5 = coin flip) |
| Information coefficient | does the score rank one signal above another? | **−0.002** |
| Drift-aware placebo | does entry *timing* beat a random entry on the same tape? | **−1.6pp** |

The placebo is the one that matters: 1,335 real signals against 6,581
random-timed entries on the same symbols and the same geometry. Ours won 47.9%
of the time, random timing won 49.5%.

**The published win rate is mostly geometry and drift, not selection.** At a
2.5×ATR stop and the target used, a driftless entry earns roughly 62% by the
barrier law alone. Any headline number has to be read against that baseline,
not against 50%.

**The one positive result is position management, not entry.** Managing an open
position — where the stop sits, when to scratch, when to let it run — measured
better than not managing it. That is what this desk implements, and it is why
the desk exists as a separate product from the signal feed.

**Failures are real and visible.** Average win and average loss are asymmetric:
wins land near +2.3%, losses near −3.8%. A run of consecutive stop-outs is the
expected behaviour of a system with no entry edge, not a malfunction.

---

## Requirements

- **Node.js** ≥ 18 and npm
- **Rust** (stable) + Cargo — https://rustup.rs
- **Tauri prerequisites** — https://tauri.app/start/prerequisites
  - Windows: Microsoft C++ Build Tools + WebView2 (bundled on Windows 11)

## Build

```bash
npm install
```

```bash
npm run tauri dev
```

Front end alone, in a browser, against local mock data:

```bash
npm run dev
```

Production bundle:

```bash
npm run tauri build
```

## Configuration

Every host the application *calls* is configurable. Copy `.env.example` to
`.env` and edit:

```
ALEPH_EDGE_API_BASE             which Sentinel deployment to talk to
ALEPH_EDGE_RELAY_URL            phone relay; leave empty to derive it from the API base
ALEPH_EDGE_VAULT_IDLE_MINUTES   vault auto-lock budget, default 30, clamped to 1-1440
```

The vault locks itself once it has gone that long without being used: unlocked,
read, added to, removed from or listed. Time spent unlocked does not count.
While a real position is open the engine reads the credential every tick to
manage it, so the vault stays available for as long as money is at risk.

The relay is *derived* from the API base rather than configured separately —
two knobs for one deployment is how a desk ends up authenticating against one
host and streaming from another. The front end carries no host at all; it asks
the Rust side.

Two things are not configurable and are pinned in `src-tauri/tauri.conf.json`:
the CSP `connect-src`, and the update feed with the public key that signs it.
Pointing the desk at your own Sentinel deployment therefore means editing that
file as well — the network calls follow `.env`, the browser policy and the
updater do not.

---

## Security surfaces

If you are reading this repository with an attacker's eye, these are the places
worth your time, in the order they are worth it:

**The vault** (`src-tauri/src/vault/`) — exchange API keys live here. Key
derivation, on-disk format, how long plaintext stays in memory, what happens
while the vault is unlocked. The salt is held in the OS keychain, so the vault
file alone is not portable.

**The pairing protocol** (`crates/aleph-link`, `crates/aleph-desk-link`) — the
phone remote control pairs by QR, and that QR carries a credential. HMAC
verification, replay protection, TTL enforcement, and whether the single-use
claim actually holds. The desk half is here; the phone half (`aleph-phone-link`)
lives in the mobile app, which is not published, so you can audit what the desk
accepts but not yet what the phone sends.

**The relay trust boundary** — the relay is supposed to carry messages, not
originate them. Assume you own the relay: what can you do?

**The automated-trading gate** — `live_trading_allowed()` in
`src-tauri/src/bot/engine/live.rs` requires four things at once: the
compile-time `LIVE_TRADING_ENABLED`, a per-bot `live` flag, a futures bot, and
Binance as the exchange. A position's own flag is read through
`OpenPosition::is_live()`, which ands it with the same master switch, because
positions are persisted as JSON and a file on disk is not a second deliberate
flip. Try to reach an automated order without all four.

**The manual close commands** — `exchange_close_position` and
`exchange_close_all` in `src-tauri/src/exchange/commands.rs` bypass that gate
by design and send real orders. Anything able to run script in the webview can
invoke them while the vault is unlocked. That is the sharpest edge in this
repository; if you find a way to reach them without the user pressing the
button, say so.

---

## Licence

PolyForm Perimeter 1.0.1. See [LICENSE](LICENSE).

In plain words: use it, change it, run it on your own account, keep your
changes private or publish them. The one thing the licence does not allow is
using it to offer a product that competes with this one — including for free.
If you want that, ask: <dev@ribqa.com>.

## Contact

Questions and bug reports: <support@ribqa.com>.
Security reports: <support@ribqa.com> — please include "security" in the
subject, and give us a chance to fix it before publishing.
