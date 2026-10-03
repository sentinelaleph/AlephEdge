/**
 * Exchange-catalog IPC bridge (mirrors src-tauri/src/exchange). The catalog is
 * the single source of supported exchanges for the key form and the bot's
 * exchange picker, and it comes from Rust (`exchange_list`). The only copy in
 * TypeScript is the dev-only preview in exchange.mock.ts (`npm run dev`).
 */

import { devMock, inTauri, invoke } from "../bridge";

type Mock = typeof import("./exchange.mock");
const mock = <T>(run: (m: Mock) => Promise<T>) => devMock(() => import("./exchange.mock"), run);

export interface ExchangeInfo {
  id: string;
  name: string;
  supportsFutures: boolean;
}

// The catalog is fixed for the process lifetime, so every caller shares one
// request. A failed request is forgotten so the next caller can retry.
let pending: Promise<ExchangeInfo[]> | null = null;

export function exchangeList(): Promise<ExchangeInfo[]> {
  if (!pending) {
    pending = (inTauri() ? invoke<ExchangeInfo[]>("exchange_list") : mock((m) => Promise.resolve(m.CATALOG))).catch(
      (e: unknown) => {
        pending = null;
        throw e;
      },
    );
  }
  return pending;
}

/** Display name for an exchange id, from a loaded catalog; the id itself
 *  while the catalog is not loaded (or the id is unknown). */
export function exchangeName(id: string, catalog: ExchangeInfo[] | null): string {
  return catalog?.find((e) => e.id === id)?.name ?? id;
}

export interface FuturesPosition {
  symbol: string;
  positionAmt: number;
  entryPrice: number;
  unrealizedPnl: number;
}

/** The connected exchange's USDT-M futures wallet: balance + open positions.
 * Read-only; the vaulted key never leaves the device (see src-tauri). */
export interface FuturesAccount {
  totalWalletBalance: number;
  availableBalance: number;
  totalUnrealizedPnl: number;
  positions: FuturesPosition[];
}

/** Fetches the connected exchange's futures account. */
export function exchangeAccount(exchangeId: string): Promise<FuturesAccount> {
  if (inTauri()) return invoke<FuturesAccount>("exchange_account", { exchangeId });
  return mock((m) => m.account());
}

/** Closes ONE open futures position at market (reduce-only). A REAL order on
 * the user's real account — user-initiated, gated behind the vaulted key. The
 * browser fallback is a no-op so `npm run dev` never touches an exchange. */
export function exchangeClosePosition(exchangeId: string, symbol: string): Promise<void> {
  if (inTauri()) return invoke<void>("exchange_close_position", { exchangeId, symbol });
  return mock(() => Promise.resolve());
}

/** Kill switch: closes EVERY open futures position at market. Resolves to the
 * number closed. Real orders (see above); dev fallback returns 0. */
export function exchangeCloseAll(exchangeId: string): Promise<number> {
  if (inTauri()) return invoke<number>("exchange_close_all", { exchangeId });
  return mock(() => Promise.resolve(0));
}

/** The exchange's tradable USDT symbols for one market (Binance today; empty
 * elsewhere or when the fetch fails). Browser fallback: a few majors. */
export function exchangeSymbols(exchangeId: string, futures: boolean): Promise<string[]> {
  if (inTauri()) return invoke<string[]>("exchange_symbols", { exchangeId, futures });
  return mock((m) => m.symbols(exchangeId));
}
