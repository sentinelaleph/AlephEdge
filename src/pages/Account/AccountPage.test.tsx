// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import type { DeskContextValue } from "@/app/DeskProvider";
import { SAMPLE_DESK_CONTEXT, SAMPLE_MEMBERSHIP } from "@/dev/shot/shot.mock";
import type { MembershipView } from "@/lib/ipc/membership/membership";
import { AccountPage, tierName } from "./AccountPage";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
window.matchMedia ??= ((query: string) =>
  ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, onchange: null, dispatchEvent: () => false }) as MediaQueryList);

const ctx = vi.hoisted(() => ({ value: null as unknown as DeskContextValue }));
vi.mock("@/app/DeskProvider", () => ({ useDeskContext: () => ctx.value }));
const open = vi.hoisted(() => vi.fn(() => Promise.resolve()));
vi.mock("@/lib/ipc/app/about", async (orig) => ({ ...(await orig<object>()), openHelpLink: open }));

const signOut = vi.fn(() => Promise.resolve());

function withView(patch: Partial<MembershipView>) {
  ctx.value = {
    ...SAMPLE_DESK_CONTEXT,
    membership: { ...SAMPLE_MEMBERSHIP, signOut, view: { ...SAMPLE_MEMBERSHIP.view, ...patch } },
  };
}

let root: Root | null = null;
let host: HTMLDivElement | null = null;

async function mount() {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(<AccountPage />));
  return host;
}

const click = async (el: Element) => act(async () => (el as HTMLElement).click());
const buttons = (text: string) => [...document.querySelectorAll<HTMLButtonElement>("button")].filter((b) => b.textContent === text);
const fact = (el: ParentNode, label: string) =>
  [...el.querySelectorAll(".ae-facts__row")].find((r) => r.querySelector("dt")?.textContent === label)?.querySelector("dd")?.textContent;

beforeAll(async () => {
  await i18n.changeLanguage("en");
});
beforeEach(() => {
  signOut.mockClear();
  open.mockClear();
  withView({});
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("Account", () => {
  it("shows the membership the app read, and when it read it", async () => {
    withView({ checkedAtMs: Date.UTC(2026, 9, 10, 12, 0), currentPeriodEnd: undefined, billingStatus: "active" });
    const el = await mount();
    expect(fact(el, "Plan")).toBe("Aleph");
    expect(fact(el, "Subscription")).toBe("Active");
    expect(fact(el, "Valid until")).toBe("No end date");
    expect(fact(el, "Last checked")).toContain("2026");
    expect(fact(el, "Bots may trade")).toBe("Yes");
  });

  it("says when the membership was never read, and marks an admin account", async () => {
    withView({ checkedAtMs: undefined, billingStatus: "none", admin: true });
    const el = await mount();
    expect(fact(el, "Last checked")).toBe("Not read yet");
    expect(fact(el, "Subscription")).toBe("Not applicable");
    expect(fact(el, "Valid until")).toBe("Not applicable");
    expect(el.textContent).toContain("Admin accounts have full access");
  });

  it("an inactive membership says the bots may not trade", async () => {
    withView({ active: false, state: "inactive" });
    const el = await mount();
    expect(fact(el, "Bots may trade")).toBe("No");
  });

  it("asks before signing out and says what stops", async () => {
    await mount();
    const [signOutButton] = buttons("Sign out");
    await click(signOutButton);
    expect(signOut).not.toHaveBeenCalled();
    const dialog = document.querySelector('[role="dialog"], [role="alertdialog"]');
    expect(dialog?.textContent).toContain("signal bots get no new signals");
    const confirm = [...dialog!.querySelectorAll("button")].find((b) => b.textContent === "Sign out")!;
    await click(confirm);
    expect(signOut).toHaveBeenCalledTimes(1);
  });

  it("opens the website's account pages through the fixed link list", async () => {
    await mount();
    await click(buttons("Profile and password")[0]);
    await click(buttons("Billing")[0]);
    expect(open.mock.calls.map((c) => (c as unknown as [string])[0])).toEqual(["webAccount", "webBilling"]);
  });

  it("names known plans and shows an unknown one as sent", () => {
    const t = i18n.getFixedT("en");
    expect(tierName(t, "aleph")).toBe("Aleph");
    expect(tierName(t, "enterprise")).toBe("enterprise");
  });
});
