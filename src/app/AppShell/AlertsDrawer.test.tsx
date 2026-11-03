// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import type { DeskAlert } from "@/app/alerts";
import { AlertsList } from "./AlertsDrawer";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const RISK: DeskAlert = { key: "portfolioRisk:paper:sameDirection", category: "portfolioRisk", severity: "danger", title: "Same-direction exposure 100%" };
const CYCLES: DeskAlert = { key: "cycleClosed", category: "dealClosed", severity: "info", title: "DCA/Grid cycles closed today: 10", count: 10 };
const LOCK: DeskAlert = { key: "vaultAutoLock", category: "vaultAutoLock", severity: "info", title: "Vault locked" };

let root: Root | null = null;
let host: HTMLDivElement | null = null;

async function mount(alerts: DeskAlert[], fresh?: ReadonlySet<string>, onNavigate?: () => void) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(<AlertsList alerts={alerts} fresh={fresh} onNavigate={onNavigate} />));
  return host;
}

beforeAll(async () => {
  await i18n.changeLanguage("en");
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  window.location.hash = "";
});

describe("alerts list (owner, 2 Nov 2026)", () => {
  it("the alert the badge counted comes first, marked New", async () => {
    const el = await mount([RISK, CYCLES], new Set(["cycleClosed"]));
    const rows = [...el.querySelectorAll("li")];
    expect(rows[0].textContent).toContain("cycles closed today: 10");
    expect(rows[0].querySelector(".ae-alerts__new")?.textContent).toBe("New");
    expect(rows[1].querySelector(".ae-alerts__new")).toBeNull();
  });

  it("a whole row opens its page and closes the drawer; a row with no page is not a link", async () => {
    const onNavigate = vi.fn();
    const el = await mount([RISK, LOCK], undefined, onNavigate);
    const link = el.querySelector<HTMLAnchorElement>("a.ae-alerts__row")!;
    expect(link.getAttribute("href")).toBe("#/risk");
    expect(link.textContent).toContain("Same-direction exposure 100%");
    await act(async () => link.click());
    expect(onNavigate).toHaveBeenCalledTimes(1);
    expect(window.location.hash).toBe("#/risk");
    expect(el.querySelectorAll("a.ae-alerts__row")).toHaveLength(1);
  });
});
