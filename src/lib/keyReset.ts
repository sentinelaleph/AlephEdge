/**
 * A live row opened at or before a vault reset made while real money was in
 * play (Rust `reconcile::opened_before_reset`). The key now stored may be
 * another account's: the app sends no order for the row, and its Close
 * removes it from the app's record only.
 */
export function isKeyResetHeld(p: { live: boolean; openedAt: number }, resetAt: number | undefined): boolean {
  return p.live && resetAt !== undefined && resetAt > 0 && p.openedAt <= resetAt;
}

/** The halt a vault reset puts on a live DCA / Grid bot (Rust `HELD_BY_KEY_RESET`). */
export const KEY_RESET_HALT = "liveKeyReplacedHeld";
