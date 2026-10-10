/**
 * Exchanges with a real-money order path (src-tauri exchange/orders.rs
 * `has_order_path`), and which of them real money may be switched on for.
 */

import type { CredentialMeta } from "@/lib/ipc/vault/vault";

export const ORDER_VENUES = ["binance", "bybit", "okx"] as const;

/**
 * Venues whose order path passed an end-to-end run on the exchange's own
 * test network (Rust `DRY_RUN_PASSED`). Rust is the authority: the desk
 * status carries the live list (`liveVenues`, sandbox included); this is
 * only the fallback before the first status arrives.
 */
export const DRY_RUN_PASSED: readonly string[] = ["binance"];

/**
 * Whether real money can be switched on for `venue` now. Anywhere else the
 * key still reads the account and closes positions; LIVE is refused by Rust
 * (`liveVenueNotDryRun`) whatever the UI shows.
 */
export function liveVenueAllowed(liveVenues: readonly string[] | undefined, venue: string): boolean {
  return (liveVenues ?? DRY_RUN_PASSED).includes(venue);
}

/** An order venue real money is not available on yet (no dry run passed). */
export function venueNotDryRun(liveVenues: readonly string[] | undefined, venue: string): boolean {
  return (ORDER_VENUES as readonly string[]).includes(venue) && !liveVenueAllowed(liveVenues, venue);
}

/**
 * The i18n key saying where a LIVE switch sends orders: Binance production or
 * its testnet, a Bybit / OKX real account, or (`venueSandbox`, Rust
 * `ALEPH_EDGE_VENUE_SANDBOX=1`) their demo / test networks.
 */
export function liveTargetKey(venue: string, binanceIsProduction: boolean, venueSandbox: boolean | undefined): string {
  if (venue === "binance") return binanceIsProduction ? "bots.live.targetProduction" : "bots.live.targetTestnet";
  return venueSandbox ? "bots.live.targetVenueSandbox" : "bots.live.targetVenue";
}

/** Order venues with a verified trade-only key, in ORDER_VENUES order. */
export function tradeVenues(credentials: CredentialMeta[]): string[] {
  return ORDER_VENUES.filter((v) => credentials.some((c) => c.exchangeId === v && c.permission === "tradeOnly"));
}
