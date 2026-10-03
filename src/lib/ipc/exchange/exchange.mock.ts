/**
 * DEV-ONLY browser preview for the exchange IPC (no Rust in `npm run dev`).
 * Reached through `devMock`, so the released bundle drops this module. The
 * preview has no exchange behind it: no balance or position is invented.
 */

import type { ExchangeInfo, FuturesAccount } from "./exchange";

export const CATALOG: ExchangeInfo[] = [
  { id: "binance", name: "Binance", supportsFutures: true },
  { id: "okx", name: "OKX", supportsFutures: true },
  { id: "bybit", name: "Bybit", supportsFutures: true },
  { id: "mexc", name: "MEXC", supportsFutures: true },
  { id: "coindcx", name: "CoinDCX", supportsFutures: false },
  { id: "bitso", name: "Bitso", supportsFutures: false },
];

export function account(): Promise<FuturesAccount> {
  return Promise.reject("No exchange in the browser preview.");
}

export function symbols(exchangeId: string): Promise<string[]> {
  return Promise.resolve(
    exchangeId === "binance" ? ["ADAUSDT", "BNBUSDT", "BTCUSDT", "DOGEUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT"] : [],
  );
}
