/**
 * Endpoints and desk identity, owned by Rust.
 *
 * The UI deliberately carries NO host. Every value here used to be a string
 * literal in a component, which is survivable in a private tree and not in a
 * public one: a reader who clones the repo would be silently pointed at one
 * particular deployment, and moving it would mean editing source instead of
 * setting a variable.
 *
 * The desk id matters more than it looks. It was a literal `desk-1`, which
 * would have handed every user on earth the same desk identity — for a pairing
 * system that is not a cosmetic problem. Rust now generates one per
 * installation and persists it, so the phone finds the same desk tomorrow.
 */

import { inTauri, invoke } from "../bridge";

export interface Endpoints {
  apiBase: string;
  relayUrl: string;
  deskId: string;
  /** Where live futures orders go. Display only: whether it is production
   *  is Rust's call (`binanceIsProduction`), never a host compared here. */
  binanceFuturesBase: string;
  /** False when live orders would go to a non-production Binance (the testnet). */
  binanceIsProduction: boolean;
}

/** Browser-only placeholder, clearly marked as such so it cannot be mistaken
 *  for a real deployment during `npm run dev`. */
const DEV: Endpoints = {
  apiBase: "http://localhost:8080",
  relayUrl: "ws://localhost:8080",
  deskId: "desk-dev",
  binanceFuturesBase: "http://localhost:8080",
  binanceIsProduction: true,
};

export async function getEndpoints(): Promise<Endpoints> {
  if (!inTauri()) return DEV;
  return invoke<Endpoints>("get_endpoints");
}

/** The desktop build's version (Cargo package version). */
export async function appVersion(): Promise<string> {
  if (!inTauri()) return "dev";
  return invoke<string>("app_version");
}
