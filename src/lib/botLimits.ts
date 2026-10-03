/**
 * Bot-form bounds that Rust enforces. Kept by hand in lockstep with the Rust
 * constants named below; the server stays the authority and rejects anything
 * outside them.
 */

/** Upper bound on a bot's max concurrent positions (`MAX_BOT_POSITIONS` in bot/model.rs). */
export const MAX_BOT_POSITIONS = 20;
