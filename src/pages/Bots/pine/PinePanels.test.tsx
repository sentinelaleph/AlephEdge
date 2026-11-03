// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import type { BotConfig } from "@/lib/ipc/bot/bot";
import type { PineResult, PineStatus } from "@/lib/ipc/pine/pine";
import { defaultSettings } from "@/lib/ipc/pine/pine.mock";
import { configFromForm, formError, formFromConfig, mappedTriggers, savedInputs, savedTriggers } from "./pineBotModel";
import { inputInfos, inputProblem, numericTitles } from "./pineInputs";
import { pineCodeText, triggerLabel } from "./pineText";
import { PineResults } from "./PineResults";
import { PineStatusPanel } from "./PineStatusPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
window.matchMedia ??= ((query: string) =>
  ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, onchange: null, dispatchEvent: () => false }) as MediaQueryList);
vi.mock("@/lib/ipc/app/about", () => ({ openHelpLink: vi.fn(() => Promise.resolve()) }));

const HASH = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const cfg: BotConfig = {
  kind: "pine",
  exchangeId: "binance",
  maxPositions: 3,
  capital: 33.33,
  leverage: 3,
  mode: "live",
  live: true,
  pine: { ...defaultSettings(), script: "x", scriptHash: HASH, symbols: ["BTCUSDT", "ETHUSDT"], triggers: { "alert#0": "buy" } },
};

let root: Root | null = null;
let host: HTMLDivElement | null = null;
async function render(node: React.ReactNode) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(node));
  return host;
}
const t = i18n.t.bind(i18n);
const exists = (k: string) => i18n.exists(k);

beforeAll(async () => {
  await i18n.changeLanguage("en");
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("Pine bot card", () => {
  const status: PineStatus = {
    configured: true,
    running: false,
    mode: "live",
    armed: true,
    scriptHash: HASH,
    timeframe: "1h",
    halted: "pineHalted|3",
    failedInARow: 3,
    opensToday: 4,
    maxOpensPerDay: 20,
    pending: 1,
    pilotLeft: 2,
    nextBarClose: null,
    symbols: [
      {
        symbol: "BTCUSDT",
        lastRunAt: Date.UTC(2026, 9, 31, 12, 0, 2),
        lastBarTime: Date.UTC(2026, 9, 31, 11),
        barsUsed: 1000,
        runMs: 84,
        lastFired: [{ id: "alert#0", title: "Long entry", action: "buy" }],
        lastError: null,
        lastNote: null,
      },
      {
        symbol: "ETHUSDT",
        lastRunAt: null,
        lastBarTime: null,
        barsUsed: null,
        runMs: null,
        lastFired: [],
        lastError: { kind: "runtime", message: "Division by zero", line: 9, col: 4, barTime: Date.UTC(2026, 9, 31, 11), atMs: 0 },
        lastNote: "pineBarsUnavailable",
      },
    ],
  };

  it("shows the halt with what to do, entries today / max, pilot left, and each pair's last run", async () => {
    const el = await render(<PineStatusPanel cfg={cfg} status={status} running={false} pilotLeft={2} marginUsed={20} />);
    const text = el.textContent!;
    expect(text).toContain("New entries stopped after 3 failed runs in a row Restart the bot to resume");
    expect(text).toContain("4 / 20");
    expect(text).toContain("Pilot entries left2");
    expect(text).toContain(HASH.slice(0, 12));
    expect(text).toContain("1,000");
    expect(text).toContain("84 ms");
    expect(text).toContain("Long entry → Buy");
    expect(text).toContain("Not run yet");
    expect(text).toContain("Run · bar");
    expect(text).toContain("line 9:4:");
    expect(text).toContain("Division by zero");
    expect(text).toContain("Binance bars unavailable · retrying");
    expect(text).toContain("Open on chart");
  });

  it("lists results with trigger, bar close, fill, slippage and the outcome in words", async () => {
    const row: PineResult = {
      key: "pine:h:BTCUSDT:1",
      symbol: "BTCUSDT",
      scriptHash: HASH,
      timeframe: "1h",
      barTime: Date.UTC(2026, 9, 31, 11),
      barCloseMs: Date.UTC(2026, 9, 31, 12),
      barClose: 70000,
      handledAt: Date.UTC(2026, 9, 31, 12, 0, 3),
      triggers: [{ id: "shape#1", title: "", action: "sell" }],
      outcome: "pinePriceMoved|0.80%",
      note: null,
      side: "short",
      botName: "Pine",
      mode: "paper",
      positionId: null,
      requestedQty: null,
      filledQty: null,
      fillPrice: 70070,
      slippagePct: 0.1,
      stopPct: 2,
      targetPct: 4,
    };
    const el = await render(<PineResults rows={[row, { ...row, key: "k2", outcome: "opened", note: "pineConflict" }]} error={null} />);
    const text = el.textContent!;
    expect(text).toContain("plotshape #2 → Sell");
    expect(text).toContain("70,000");
    expect(text).toContain("70,070");
    expect(text).toContain("+0.10%");
    expect(text).toContain("Not executed · price moved 0.80% from the bar's close");
    expect(text).toContain("Opened · Buy and Sell on the same bar · nothing opened");
  });
});

describe("Pine codes and model", () => {
  it("translates every code with its arguments, wrappers and the symbol minimum", () => {
    expect(pineCodeText(t, exists, "pineBelowSymbolMinimum|BTCUSDT|83.00|50.00|50.00")).toBe(
      "Not executed · BTCUSDT needs at least 83.00 USDT per order; this entry was 50.00 USDT (size per entry 50.00 USDT)",
    );
    expect(pineCodeText(t, exists, "pineFlipOpenRefused|pineBudgetFull|12.50")).toBe(
      "Reversal: closed, the new position was refused · The bot's budget is full · 12.50 USDT left",
    );
    expect(pineCodeText(t, exists, "pineSymbolHeldBy|futures")).toContain("held by the Futures bot");
    expect(pineCodeText(t, exists, "livePending|pineOpened")).toContain("never sent again · Opened");
    expect(pineCodeText(t, exists, "flipped")).toBe("Reversed · closed, then opened");
    expect(triggerLabel(t, { id: "alert#2", title: "" })).toBe("alert #3");
  });

  it("drops Ignore and unknown triggers, and inputs equal to their default", () => {
    const tr = [{ id: "alert#0", kind: "alert" as const, title: "A", line: 1 }];
    expect(savedTriggers({ "alert#0": "buy", "alert#1": "sell", x: "ignore" }, tr)).toEqual({ "alert#0": "buy" });
    const infos = inputInfos([{ id: 0, kind: "int", title: "Len", defval: 14, line: 1, col: 1 }]);
    expect(savedInputs({ "0": 14 }, infos)).toEqual({});
    expect(savedInputs({ "0": 20, "7": 1 }, infos)).toEqual({ "0": 20 });
    expect(inputProblem(infos[0], 1.5)?.key).toBe("integer");
    expect(numericTitles(inputInfos([{ id: 0, kind: "float", title: "Stop %", defval: 1 }, { id: 1, kind: "bool", title: "On", defval: true }]))).toEqual(["Stop %"]);
    expect(mappedTriggers(cfg.pine, tr)).toEqual([{ id: "alert#0", title: "A", action: "buy" }]);
  });

  it("stops Save on the first bad field, and Live without a stop", () => {
    const ok = { ok: true, error: null, title: "", overlay: true, inputs: [], triggers: [{ id: "alert#0", kind: "alert" as const, title: "A", line: 1 }], hash: HASH };
    const f = { ...formFromConfig(cfg), symbols: "BTCUSDT" };
    expect(formError(f, ok, 5, false)).toBeNull();
    expect(formError(f, null, 5, false)?.key).toBe("compiling");
    expect(formError(f, { ...ok, triggers: [] }, 5, false)?.key).toBe("triggersNone");
    expect(formError({ ...f, triggers: { "alert#0": "ignore" } }, ok, 5, false)?.key).toBe("triggersNone");
    expect(formError({ ...f, symbols: "" }, ok, 5, false)?.key).toBe("symbolsMin");
    expect(formError({ ...f, warmupBars: 200 }, ok, 5, false)?.key).toBe("warmup");
    expect(formError({ ...f, stopPct: 0 }, ok, 5, false)?.key).toBe("stopRequired");
    expect(formError({ ...f, stopPct: 0, stopInput: "Stop %" }, ok, 5, false)).toBeNull();
    expect(formError({ ...f, stopPct: 0, stopInput: "Stop %" }, ok, 5, true)?.key).toBe("liveStop");
    const sent = configFromForm({ ...f, stopPct: 0, stopInput: "Stop %" }, cfg, ok);
    expect(sent.pine).toMatchObject({ stopPct: null, stopInput: "Stop %", scriptHash: HASH });
    expect(sent.capital).toBe(33.33);
  });
});
