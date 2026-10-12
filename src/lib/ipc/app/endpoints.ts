/**
 * Endpoints and desk identity, owned by Rust: the UI carries no host and no
 * desk id. The rules live in `app::endpoints` (src-tauri/src/app/endpoints.rs).
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
  if (inTauri()) return invoke<Endpoints>("get_endpoints");
  // The placeholder is for `npm run dev` only; a production bundle outside
  // Tauri has no endpoints to show and says so.
  if (import.meta.env.DEV) return DEV;
  throw new Error("getEndpoints: not running inside Tauri");
}

/** The desktop build's version (Cargo package version). */
export async function appVersion(): Promise<string> {
  if (!inTauri()) return "dev";
  return invoke<string>("app_version");
}
