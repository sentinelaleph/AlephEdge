import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { CATALOG } from "@/lib/ipc/exchange/exchange.mock";
import type { CredentialMeta } from "@/lib/ipc/vault/vault";
import { DRY_RUN_PASSED, liveTargetKey, liveVenueAllowed, ORDER_VENUES, tradeVenues, venueNotDryRun } from "./venues";

/** The venue ids of Rust's capability table, in its order. */
const RUST_VENUES = [
  ...fs
    .readFileSync(path.join(__dirname, "..", "..", "src-tauri", "src", "exchange", "venue", "caps.rs"), "utf8")
    .matchAll(/[.]{2}row\("([a-z]+)", "/g),
].map((m) => m[1]);

describe("order venue list", () => {
  it("is Rust's capability table, all 8 order venues", () => {
    expect(RUST_VENUES).toHaveLength(8);
    expect([...ORDER_VENUES]).toEqual(RUST_VENUES);
  });

  it("the dev catalog shows every venue truthfully: only Binance is live", () => {
    for (const id of ORDER_VENUES) expect(CATALOG.find((x) => x.id === id)?.venue, id).toBeTruthy();
    const live = CATALOG.filter((x) => x.venue?.live === "verified").map((x) => x.id);
    expect(live).toEqual(["binance"]);
    const paper = CATALOG.filter((x) => x.venue?.live === "paperOnly").map((x) => x.id);
    // Hyperliquid: price data / paper only in 0.3.22 (RUSTSEC-2026-0387).
    expect(paper).toEqual(["kucoinfutures", "htx", "phemex", "coinex", "hyperliquid", "krakenfutures", "bitfinex"]);
    expect(ORDER_VENUES).not.toContain("hyperliquid");
    expect(CATALOG.filter((x) => x.venue).length).toBe(15);
    for (const x of CATALOG.filter((x) => x.venue?.live === "paperOnly")) expect(x.venue?.orders, x.id).toBeNull();
    expect(CATALOG.find((x) => x.id === "mexc")?.venue).toBeUndefined();
  });
});

describe("real-money venues", () => {
  it("before the first desk status, only the dry-run venue is offered", () => {
    expect(DRY_RUN_PASSED).toEqual(["binance"]);
    expect(liveVenueAllowed(undefined, "binance")).toBe(true);
    expect(liveVenueAllowed(undefined, "bybit")).toBe(false);
    expect(liveVenueAllowed(undefined, "okx")).toBe(false);
  });

  it("follows the list Rust sends (the sandbox dry run opens every order venue)", () => {
    expect(liveVenueAllowed(["binance"], "okx")).toBe(false);
    expect(liveVenueAllowed(["binance", "bybit", "okx"], "okx")).toBe(true);
    expect(liveVenueAllowed([], "binance")).toBe(false);
  });

  it("names order venues without a dry run, not exchanges without an order path", () => {
    expect(venueNotDryRun(undefined, "bybit")).toBe(true);
    expect(venueNotDryRun(undefined, "okx")).toBe(true);
    expect(venueNotDryRun(undefined, "binance")).toBe(false);
    expect(venueNotDryRun(undefined, "mexc")).toBe(false);
    for (const v of ORDER_VENUES.filter((v) => v !== "binance")) expect(venueNotDryRun(undefined, v), v).toBe(true);
    expect(venueNotDryRun(["binance", "bybit", "okx"], "bybit")).toBe(false);
  });

  it("keys for every order venue still count for the account and close views", () => {
    const key = (exchangeId: string): CredentialMeta =>
      ({ exchangeId, label: exchangeId, permission: "tradeOnly" }) as unknown as CredentialMeta;
    expect(tradeVenues([key("okx"), key("binance"), key("bybit")])).toEqual(["binance", "bybit", "okx"]);
  });

  it("the LIVE confirmation says demo network, not real account, in the sandbox", () => {
    expect(liveTargetKey("bybit", true, false)).toBe("bots.live.targetVenue");
    expect(liveTargetKey("okx", true, undefined)).toBe("bots.live.targetVenue");
    expect(liveTargetKey("bybit", true, true)).toBe("bots.live.targetVenueSandbox");
    expect(liveTargetKey("okx", false, true)).toBe("bots.live.targetVenueSandbox");
    // Binance follows its own endpoint, never the Bybit / OKX sandbox switch.
    expect(liveTargetKey("binance", true, true)).toBe("bots.live.targetProduction");
    expect(liveTargetKey("binance", false, false)).toBe("bots.live.targetTestnet");
  });
});
