import { describe, expect, it } from "vitest";
import type { CredentialMeta } from "@/lib/ipc/vault/vault";
import { DRY_RUN_PASSED, liveTargetKey, liveVenueAllowed, tradeVenues, venueNotDryRun } from "./venues";

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
