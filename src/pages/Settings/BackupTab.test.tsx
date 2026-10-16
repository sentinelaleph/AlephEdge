// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import type { ImportReport } from "@/lib/ipc/app/backup";
import { reset } from "@/lib/ipc/app/backup.mock";
import { BackupTab, sizeText } from "./BackupTab";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// The browser-preview mock (devMock) answers every call; one test swaps the
// import result for a desk that created bots.
const hooks = vi.hoisted(() => ({ report: null as ImportReport | null }));
vi.mock("@/lib/ipc/app/backup", async (orig) => {
  const real = await orig<typeof import("@/lib/ipc/app/backup")>();
  return {
    ...real,
    settingsImport: (json: string) => (hooks.report ? Promise.resolve(hooks.report) : real.settingsImport(json)),
  };
});

let root: Root | null = null;
let host: HTMLDivElement | null = null;

async function mount() {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(<BackupTab />));
  return host;
}

const button = (el: ParentNode, text: string) => [...el.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === text)!;
const click = async (el: Element) => act(async () => (el as HTMLElement).click());

async function pick(el: ParentNode, body: string) {
  const input = el.querySelector<HTMLInputElement>('input[type="file"]')!;
  const file = new File([body], "bots.json", { type: "application/json" });
  Object.defineProperty(input, "files", { value: [file], configurable: true });
  await act(async () => {
    input.dispatchEvent(new Event("change", { bubbles: true }));
  });
  // file.text() and the IPC resolve on later ticks.
  await act(async () => new Promise((r) => setTimeout(r, 0)));
}

const FILE = JSON.stringify({
  format: "aleph-edge-bot-settings",
  formatVersion: 1,
  appVersion: "0.3.14",
  exportedAtMs: 0,
  signalBots: [],
  strategyBots: [{ name: "DCA BTC", symbol: "BTCUSDT", params: { kind: "dca" } }],
});

beforeAll(async () => {
  await i18n.changeLanguage("en");
});
beforeEach(() => {
  reset();
  hooks.report = null;
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("BackupTab", () => {
  it("starts empty and lists a backup taken with Back up now", async () => {
    const el = await mount();
    expect(el.textContent).toContain("No backups yet");
    expect(el.textContent).toContain("One is taken before every update install.");
    await click(button(el, "Back up now"));
    expect(el.textContent).not.toContain("No backups yet");
    expect(el.textContent).toContain("Manual");
    expect(el.textContent).toContain("0 B");
  });

  it("states what an update keeps", async () => {
    const el = await mount();
    expect(el.textContent).toContain("What an update keeps");
    expect(el.textContent).toContain("If the backup fails, the update does not install.");
    expect(el.textContent).toContain("real money needs LIVE typed again");
    expect(el.textContent).toContain("A bot on real money comes back stopped; its open cycle stays managed.");
  });

  it("refuses a file that is not a settings file", async () => {
    const el = await mount();
    await pick(el, JSON.stringify({ format: "other", formatVersion: 1 }));
    expect(el.querySelector('[role="alert"]')?.textContent).toBe("This is not an Aleph Edge bot settings file.");
    await pick(el, "{not json");
    expect(el.querySelector('[role="alert"]')?.textContent).toBe("This file is not readable JSON.");
  });

  it("reports each bot of the file; the preview saves none", async () => {
    const el = await mount();
    await pick(el, FILE);
    expect(el.textContent).toContain("0 of 1 applied");
    expect(el.textContent).toContain("DCA · DCA BTC · BTCUSDT");
    expect(el.textContent).toContain("Refused: Not saved: the browser preview has no data folder.");
  });

  it("shows created and refused rows with their reasons", async () => {
    hooks.report = {
      signalBots: [{ kind: "futures", name: null, symbol: null, ok: true, code: null, id: null }],
      strategyBots: [
        { kind: "grid", name: "Grid ETH", symbol: "ETHUSDT", ok: true, code: null, id: "sb_x" },
        { kind: "dca", name: "DCA SOL", symbol: "SOLUSDT", ok: false, code: "maxBots", id: null },
      ],
    };
    const el = await mount();
    await pick(el, FILE);
    expect(el.textContent).toContain("2 of 3 applied");
    expect(el.textContent).toContain("Futures signal bot");
    expect(el.textContent).toContain("Settings replaced, simulated");
    expect(el.textContent).toContain("Added, stopped, simulated");
    expect(el.textContent).toContain(`Refused: ${i18n.t("strategy.errors.maxBots")}`);
  });

  it("exports through a download link", async () => {
    const created = vi.fn(() => "blob:x");
    Object.assign(URL, { createObjectURL: created, revokeObjectURL: vi.fn() });
    const el = await mount();
    await click(button(el, "Export settings"));
    expect(created).toHaveBeenCalledTimes(1);
    expect(el.textContent).toMatch(/Download started: aleph-edge-bots-\d{8}-\d{4}\.json/);
  });
});

describe("sizeText", () => {
  it("uses binary units", () => {
    expect(sizeText(0, "en")).toBe("0 B");
    expect(sizeText(1536, "en")).toBe("1.5 KB");
    expect(sizeText(5 * 1024 * 1024, "en")).toBe("5 MB");
  });
});
