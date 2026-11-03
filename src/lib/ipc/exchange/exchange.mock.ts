/**
 * DEV-ONLY browser preview for the exchange IPC (no Rust in `npm run dev`).
 * Reached through `devMock`, so the released bundle drops this module. The
 * preview has no exchange behind it: no balance or position is invented.
 */

import type { ExchangeInfo, FuturesAccount } from "./exchange";

type O = NonNullable<NonNullable<ExchangeInfo["venue"]>["orders"]>;
const order = (over: Partial<O> = {}, margin = "USDT", live: "verified" | "notVerified" = "notVerified"): ExchangeInfo["venue"] => ({
  margin,
  live,
  orders: { keyFields: "keySecret", stop: "reduceOnlyFullSize", stopMove: "placeThenCancel", isolated: "setMode", testnet: true, ...over },
});
const paper = (margin = "USDT"): ExchangeInfo["venue"] => ({ margin, live: "paperOnly", orders: null });
const ex = (id: string, name: string, venue?: ExchangeInfo["venue"], supportsSpot = true): ExchangeInfo => ({ id, name, supportsFutures: true, supportsSpot, venue });

/** Mirrors src-tauri exchange/venue/caps.rs + providers::CATALOG (dev preview only). */
export const CATALOG: ExchangeInfo[] = [
  ex("binance", "Binance", order({ stop: "closesPosition", stopMove: "stopBeside" }, "USDT", "verified")),
  ex("bybit", "Bybit", order({ stop: "closesPosition", stopMove: "inPlace" })),
  ex("okx", "OKX", order({ keyFields: "keySecretPassphrase", stop: "closesPosition", stopMove: "inPlace", isolated: "perOrder" })),
  ex("bitget", "Bitget", order({ keyFields: "keySecretPassphrase", stop: "closesPosition" })),
  ex("gate", "Gate", order({ isolated: "viaLeverage" })),
  ex("kucoinfutures", "KuCoin Futures", paper()),
  ex("htx", "HTX", paper()),
  ex("bingx", "BingX", order({ stop: "closesPosition" })),
  ex("phemex", "Phemex", paper(), false),
  ex("coinex", "CoinEx", paper()),
  ex("woo", "WOO X", order({ isolated: "perOrder" })),
  ex("hyperliquid", "Hyperliquid", paper("USDC"), false),
  ex("krakenfutures", "Kraken Futures", paper("USD"), false),
  ex("deribit", "Deribit", order({ isolated: "notOffered" }, "USDC"), false),
  ex("bitfinex", "Bitfinex", paper()),
  ex("mexc", "MEXC"),
  { id: "coindcx", name: "CoinDCX", supportsFutures: false, supportsSpot: true },
  { id: "bitso", name: "Bitso", supportsFutures: false, supportsSpot: true },
];

export function account(): Promise<FuturesAccount> {
  // The screenshot harness (?shot=, every screen labelled "Sample data")
  // shows a sample account so the FAQ pictures the panel as it looks in use.
  if (new URLSearchParams(window.location.search).has("shot")) {
    return Promise.resolve({
      totalWalletBalance: 1000,
      availableBalance: 912.4,
      totalUnrealizedPnl: 1.86,
      positions: [{ symbol: "ETHUSDT", positionAmt: 0.04, entryPrice: 2190.5, unrealizedPnl: 1.86 }],
    });
  }
  return Promise.reject("No exchange in the browser preview.");
}

export function symbols(exchangeId: string): Promise<string[]> {
  return Promise.resolve(
    exchangeId === "binance" ? ["ADAUSDT", "BNBUSDT", "BTCUSDT", "DOGEUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT"] : [],
  );
}
