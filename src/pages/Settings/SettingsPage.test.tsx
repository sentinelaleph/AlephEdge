// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import { SAMPLE_DESK_CONTEXT } from "@/dev/shot/shot.mock";
import type { AppInfo } from "@/lib/ipc/app/about";
import { ThemeProvider } from "@/theme/ThemeProvider";
import { SettingsPage } from "./SettingsPage";
import { resetAppInfoForTests } from "./settingsData";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
window.matchMedia ??= ((query: string) =>
  ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, onchange: null, dispatchEvent: () => false }) as MediaQueryList);

vi.mock("@/app/DeskProvider", () => ({ useDeskContext: () => SAMPLE_DESK_CONTEXT }));

const about = vi.hoisted(() => ({
  info: null as AppInfo | null,
  open: vi.fn(() => Promise.resolve()),
  openDir: vi.fn(() => Promise.resolve()),
}));
vi.mock("@/lib/ipc/app/about", async (orig) => ({
  ...(await orig<object>()),
  appInfo: () => Promise.resolve(about.info),
  openHelpLink: about.open,
  openDataDir: about.openDir,
}));

const INFO: AppInfo = {
  version: "0.3.0",
  os: "windows",
  arch: "x86_64",
  tauri: "2.11.5",
  webview: "141.0.3537.71",
  liveBuild: false,
  identifier: "com.sentinelaleph.edge",
  dataDir: "C:\\Users\\someone\\AppData\\Roaming\\com.sentinelaleph.edge",
};

let root: Root | null = null;
let host: HTMLDivElement | null = null;

async function mount() {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () =>
    root!.render(
      <ThemeProvider>
        <SettingsPage />
      </ThemeProvider>,
    ),
  );
  return host;
}

const click = async (el: Element) => act(async () => (el as HTMLElement).click());
const navItem = (el: ParentNode, label: string) =>
  [...el.querySelectorAll<HTMLButtonElement>(".ae-setnav__item")].find((b) => b.querySelector(".ae-setnav__label")?.textContent === label)!;
const button = (el: ParentNode, text: string) => [...el.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === text)!;

beforeAll(async () => {
  await i18n.changeLanguage("en");
});
beforeEach(() => {
  about.info = { ...INFO };
  resetAppInfoForTests();
  about.open.mockClear();
  about.openDir.mockClear();
  localStorage.clear();
  window.location.hash = "#/settings";
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("Settings", () => {
  it("lists every section with its state and opens one from the list", async () => {
    const el = await mount();
    const labels = [...el.querySelectorAll(".ae-setnav__label")].map((n) => n.textContent);
    expect(labels).toEqual(["General", "Notifications", "Devices", "Exchange keys", "About"]);
    expect(navItem(el, "General").getAttribute("aria-current")).toBe("page");
    expect(navItem(el, "Notifications").textContent).toContain("8 of 8 shown");

    await click(navItem(el, "About"));
    expect(window.location.hash).toBe("#/settings?tab=about");
    expect(navItem(el, "About").getAttribute("aria-current")).toBe("page");
    expect(el.querySelector(".ae-settings__title")?.textContent).toBe("About");
  });

  it("keeps the old deep links working", async () => {
    window.location.hash = "#/settings?tab=keys";
    const el = await mount();
    expect(el.querySelector(".ae-settings__title")?.textContent).toBe("Exchange keys");
    expect(el.textContent).toContain("Which key to add");
  });

  it("hides an alert category from the bell and says the bots still act on it", async () => {
    window.location.hash = "#/settings?tab=notifications";
    const el = await mount();
    const kill = el.querySelector<HTMLInputElement>("#notify-killSwitch")!;
    expect(kill.checked).toBe(true);
    expect(kill.getAttribute("role")).toBe("switch");
    await click(kill);
    expect(JSON.parse(localStorage.getItem("aleph-edge-notify") ?? "{}").killSwitch).toBe(false);
    expect(kill.closest("li")?.textContent).toContain("The bots still act on it");
    expect(navItem(el, "Notifications").textContent).toContain("7 of 8 shown");

    await click(button(el, "Show all"));
    expect(kill.checked).toBe(true);
  });

  it("shows this installation from the running app, and opens only the fixed links", async () => {
    window.location.hash = "#/settings?tab=about";
    const el = await mount();
    const text = el.textContent ?? "";
    expect(text).toContain("Windows x64");
    expect(text).toContain("141.0.3537.71");
    expect(text).toContain("Paper build");
    expect(text).not.toContain("Testnet");

    await click(button(el, "Source code"));
    expect(about.open).toHaveBeenCalledWith("source");
    await click(button(el, "Open folder"));
    expect(about.openDir).toHaveBeenCalledTimes(1);
  });

  it("marks a live or testnet build", async () => {
    about.info = { ...INFO, liveBuild: true, identifier: "com.sentinelaleph.edge.testnet" };
    window.location.hash = "#/settings?tab=about";
    const el = await mount();
    const chips = [...el.querySelectorAll(".ae-about .ae-chip")].map((c) => c.textContent);
    expect(chips).toContain("Live build");
    expect(chips).toContain("Testnet");
  });

  it("copies diagnostics without the data folder path", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.assign(navigator, { clipboard: { writeText } });
    window.location.hash = "#/settings?tab=about";
    const el = await mount();
    await click(button(el, "Copy diagnostics"));
    const copied = (writeText.mock.calls[0] as unknown as [string])[0];
    expect(copied).toContain("Aleph Edge 0.3.0");
    expect(copied).toContain("Windows x64");
    expect(copied).not.toContain("someone");
    expect(copied).not.toContain(SAMPLE_DESK_CONTEXT.endpoints?.deskId ?? "desk-");
  });

  it("previews each theme and switches on click", async () => {
    const el = await mount();
    const dark = [...el.querySelectorAll<HTMLButtonElement>(".ae-themecard")].find((b) => b.textContent?.startsWith("Dark"))!;
    await click(dark);
    expect(dark.getAttribute("aria-pressed")).toBe("true");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
});
