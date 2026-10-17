// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import { BotNewPage } from "@/pages/Bots/BotNewPage";
import { PresetsPage } from "./PresetsPage";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
window.matchMedia ??= ((query: string) =>
  ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, onchange: null, dispatchEvent: () => false }) as MediaQueryList);

// The New bot page reads the desk; only the fields it touches.
vi.mock("@/app/DeskProvider", () => ({
  useDeskContext: () => ({
    desk: { loaded: true, status: { liveTradingEnabled: false, liveVenues: [], bots: [], openPositions: [] } },
    risk: { state: null },
    strategy: { bots: [], risk: null },
    keyedExchanges: [],
    catalog: { exchanges: [] },
  }),
}));
vi.mock("@/pages/Bots/strategy/useStrategyLive", () => ({ useStrategyLive: () => ({ liveIds: new Set<string>() }) }));
vi.mock("@/pages/Bots/botRows", () => ({ signalBotRows: () => [] }));

let root: Root | null = null;
let host: HTMLDivElement | null = null;

/** The dev mocks (strategy_presets, presets-live) answer on a timer; let them settle. */
const settle = () => act(async () => new Promise((r) => setTimeout(r, 50)));

async function mount(node: React.ReactNode) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(node));
  await settle();
  await settle();
  return host;
}

const buttons = (el: ParentNode, text: string) => [...el.querySelectorAll("button")].filter((b) => b.textContent === text);
const click = async (el: HTMLElement) => act(async () => el.click());

beforeAll(async () => {
  await i18n.changeLanguage("en");
});
beforeEach(() => {
  window.location.hash = "#/presets";
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("Presets page: DCA Long Classic recommended, DCA Long Safe also proven", () => {
  it("lists Classic then Safe with a Proven badge; every other one is Experimental with its TEST mean", async () => {
    const el = await mount(<PresetsPage />);
    const badges = [...el.querySelectorAll<HTMLElement>("tbody [data-evidence]")];
    // The dev mock serves four of the twelve templates: classic, safe, a failed DCA short and a failed grid.
    expect(badges.length).toBe(4);
    for (const b of badges.slice(0, 2)) {
      expect(b.dataset.evidence).toBe("proven");
      expect(b.textContent).toBe("Proven");
    }
    expect(badges[0].closest("tr")?.textContent).toContain("DCA Long Classic");
    expect(badges[1].closest("tr")?.textContent).toContain("DCA Long Safe");
    for (const b of badges.slice(2)) {
      expect(b.dataset.evidence).toBe("experimental");
      expect(b.textContent).toMatch(/^Experimental · TEST [+-]\d+\.\d{2}% per bot-month$/);
    }
    expect(badges.map((b) => b.textContent).slice(2).sort()).toEqual(["Experimental · TEST -2.43% per bot-month", "Experimental · TEST -8.61% per bot-month"]);
  });

  it("recommends Classic on its majors list, names Safe as also proven with its numbers from the data, and opens Classic's form", async () => {
    const el = await mount(<PresetsPage />);
    const pick = el.querySelector<HTMLElement>(".ae-preset__pick")!;
    expect(pick.textContent).toContain("Recommended: DCA Long Classic");
    expect(pick.textContent).toContain("TEST +1.90% per bot-month, 8/8 months positive, worst bot -20.7%.");
    expect(pick.textContent).toContain("on the top 30 pairs the worst bot fell -97.7%");
    expect(pick.textContent).toContain("Also proven: DCA Long Safe, TEST +1.45% per bot-month, 8/8 months positive. Every other template is experimental.");
    expect(buttons(pick, "Use DCA Long Safe")).toHaveLength(0);
    await click(buttons(pick, "Use DCA Long Classic")[0]);
    expect(window.location.hash).toBe("#/bots/dca/new?preset=dca_long_classic");
  });

  it("the Grid list names no proven template", async () => {
    window.location.hash = "#/presets?type=grid";
    const el = await mount(<PresetsPage />);
    expect(el.querySelector(".ae-preset__pick")).toBeNull();
    expect(el.textContent).toContain("No template of this type is proven. Every one here is experimental.");
    expect(el.querySelector('tbody [data-evidence="proven"]')).toBeNull();
  });
});

describe("New bot page: the proven template is the first choice", () => {
  it("the DCA card starts from DCA Long Classic; the Grid card has no proven template", async () => {
    window.location.hash = "#/bots/new";
    const el = await mount(<BotNewPage />);
    const start = buttons(el, "Start with DCA Long Classic");
    expect(start).toHaveLength(1);
    expect(start[0].className).toContain("ae-btn--primary");
    expect(el.textContent).toContain("Presets2 proven · 1 experimental");
    expect(buttons(el, "Start with DCA Long Safe")).toHaveLength(0);
    expect(el.textContent).toContain("Presets1 experimental");
    expect(el.textContent).not.toContain("0 proven");
    expect(el.textContent).toContain("Proven template DCA Long Classic, historical TEST: +1.90% per bot-month");
    expect(el.textContent).toContain("No proven template. Every template of this type is experimental.");
    await click(start[0]);
    expect(window.location.hash).toBe("#/bots/dca/new?preset=dca_long_classic");
  });
});
