// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import { mock } from "@/lib/ipc/telegram/telegram.mock";
import { TelegramPanel } from "./TelegramPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// Built at run time so no token-shaped literal sits in the source.
const FAKE = ["123456789", "fakeTOKENpart_-".repeat(3)].join(":");

let root: Root | null = null;
let host: HTMLDivElement | null = null;

/** Lets the mock's promises and the state updates settle. */
const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)));

async function mount() {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(<TelegramPanel />));
  await settle();
  return host;
}

const button = (el: ParentNode, text: string) => [...el.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === text);
const chip = (el: ParentNode) => el.querySelector(".ae-status")?.textContent;
const click = async (el: Element) => {
  await act(async () => (el as HTMLElement).click());
  await settle();
};

async function typeInto(input: HTMLInputElement, value: string) {
  const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  await act(async () => {
    set.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

beforeAll(async () => {
  await i18n.changeLanguage("en");
});
beforeEach(() => mock.reset());
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("Settings, Notifications, Telegram panel: own bot", () => {
  beforeEach(async () => {
    await mock.setMode("own");
  });

  it("not set up: shows the steps and a password field, and refuses a malformed token", async () => {
    const el = await mount();
    expect(chip(el)).toBe("Not set up");
    expect(el.querySelectorAll(".ae-tg__steps li")).toHaveLength(3);
    const input = el.querySelector<HTMLInputElement>(".ae-tg__token input")!;
    expect(input.type).toBe("password");
    expect(button(el, "Save token")!.disabled).toBe(true);
    await typeInto(input, "not-a-token");
    await click(button(el, "Save token")!);
    expect(el.querySelector('[role="alert"]')?.textContent).toContain("This is not a bot token");
    expect(button(el, "Send test")).toBeUndefined();
  });

  it("saving a token clears the field and asks for a chat; a code waits for /start", async () => {
    const el = await mount();
    await typeInto(el.querySelector<HTMLInputElement>(".ae-tg__token input")!, FAKE);
    await click(button(el, "Save token")!);
    expect(chip(el)).toBe("No chat linked");
    expect(el.querySelector(".ae-tg__token")).toBeNull();
    expect(el.textContent).not.toContain(FAKE);
    expect(el.textContent).toContain("@aleph_preview_bot");

    await click(button(el, "Get a code")!);
    expect(chip(el)).toBe("Waiting for /start");
    expect(el.querySelector(".ae-tg__code")?.textContent).toBe("/start K7M2Q9XR");
    expect(button(el, "New code")).toBeDefined();
  });

  it("linked: toggles per category, commands off by default, test and remove", async () => {
    await mock.setToken(FAKE);
    mock.link("@ann");
    const el = await mount();
    expect(chip(el)).toBe("Linked");
    expect(el.textContent).toContain("@ann");
    expect(el.textContent).toContain("Never");
    expect(el.querySelector<HTMLInputElement>("#tg-enabled")!.checked).toBe(true);
    for (const c of ["killSwitch", "botError", "stopLoss", "dealClosed"]) {
      expect(el.querySelector<HTMLInputElement>(`#tg-${c}`)!.checked, c).toBe(true);
    }
    const commands = el.querySelector<HTMLInputElement>("#tg-commands")!;
    expect(commands.checked).toBe(false);
    expect(el.textContent).toContain("open positions stay open and managed");

    await click(el.querySelector("#tg-dealClosed")!);
    expect((await mock.status()).prefs.categories.dealClosed).toBe(false);
    await click(commands);
    expect((await mock.status()).prefs.commandsEnabled).toBe(true);

    await click(button(el, "Send test")!);
    expect(el.querySelector('[role="status"]')?.textContent).toBe("Test message sent.");
    expect(el.textContent).not.toContain("Never");

    await click(button(el, "Remove")!);
    expect(chip(el)).toBe("Not set up");
    expect((await mock.status()).tokenSet).toBe(false);
  });

  it("the last error is shown in the user's language", async () => {
    await mock.setToken(FAKE);
    mock.link("Ann");
    mock.setError("telegramChatUnreachable");
    const el = await mount();
    expect(el.textContent).toContain("The bot cannot write to the linked chat.");
  });

  it("turning categories off waits for notifications to be on", async () => {
    await mock.setToken(FAKE);
    mock.link("Ann");
    const el = await mount();
    await click(el.querySelector("#tg-enabled")!);
    expect((await mock.status()).prefs.enabled).toBe(false);
    expect(el.querySelector<HTMLInputElement>("#tg-killSwitch")!.disabled).toBe(true);
  });
});

describe("Settings, Notifications, Telegram panel: official bot", () => {
  const radio = (el: ParentNode, name: string) =>
    [...el.querySelectorAll<HTMLButtonElement>('[role="radio"]')].find((b) => b.textContent === name);

  it("is the default: the official choice is selected, no token field, the privacy line is shown", async () => {
    const el = await mount();
    expect(radio(el, "Official bot (@SentinelAleph_bot)")!.getAttribute("aria-checked")).toBe("true");
    expect(radio(el, "Your own bot")!.getAttribute("aria-checked")).toBe("false");
    expect(chip(el)).toBe("Not connected");
    expect(el.querySelector(".ae-tg__token")).toBeNull();
    expect(el.textContent).toContain("Notification text passes through ribqa.com to Telegram.");
    expect(button(el, "Connect @SentinelAleph_bot")).toBeDefined();
    expect(button(el, "Send test")).toBeUndefined();
  });

  it("switching modes shows the own-bot setup and back", async () => {
    const el = await mount();
    await click(radio(el, "Your own bot")!);
    expect((await mock.status()).mode).toBe("own");
    expect(chip(el)).toBe("Not set up");
    expect(el.querySelector(".ae-tg__token input")).not.toBeNull();
    expect(el.textContent).not.toContain("passes through ribqa.com");
    await click(radio(el, "Official bot (@SentinelAleph_bot)")!);
    expect((await mock.status()).mode).toBe("official");
    expect(el.querySelector(".ae-tg__token")).toBeNull();
  });

  it("connect: the code alone is the main path, shown large with Copy and the three steps; the t.me link is secondary", async () => {
    const el = await mount();
    await click(button(el, "Connect @SentinelAleph_bot")!);
    expect(chip(el)).toBe("Waiting for the code");
    // The bare code, no "/start": the bot takes the code sent on its own.
    const code = el.querySelector(".ae-tg__code--big");
    expect(code?.textContent).toBe("Q4RT8WXM");
    expect(el.textContent).not.toContain("/start");
    expect([...el.querySelectorAll(".ae-tg__steps li")].map((li) => li.textContent)).toEqual([
      "Open Telegram and find @SentinelAleph_bot.",
      "Send this code to the bot as a message.",
      "This panel updates when connected.",
    ]);
    // Copy is the primary button; opening Telegram is a secondary one.
    const copy = button(el, "Copy code")!;
    expect(copy.className).toContain("ae-btn--primary");
    const open = button(el, "Open in Telegram with the code")!;
    expect(open.className).toContain("ae-btn--secondary");
    await click(open);
    expect(el.querySelector('[role="alert"]')).toBeNull();

    // The user sends the code; the next read sees the link.
    mock.officialLinked("@ann");
    await act(async () => root!.unmount());
    host!.remove();
    const again = await mount();
    expect(chip(again)).toBe("Linked");
    expect(again.textContent).toContain("@ann");
    expect(again.querySelector(".ae-tg__code")).toBeNull();
    expect(again.querySelector<HTMLInputElement>("#tg-enabled")!.checked).toBe(true);
  });

  it("Copy code puts the bare code on the clipboard and says so; a refused copy says to select it", async () => {
    const writeText = vi.fn<(text: string) => Promise<void>>().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const el = await mount();
    await click(button(el, "Connect @SentinelAleph_bot")!);
    await click(button(el, "Copy code")!);
    expect(writeText).toHaveBeenCalledWith("Q4RT8WXM");
    expect(button(el, "Copied")).toBeDefined();

    writeText.mockRejectedValueOnce(new Error("denied"));
    await click(button(el, "Copied")!);
    expect(button(el, "Copy failed. Select the code and copy it.")).toBeDefined();
  });

  it("linked: the commands switch is disabled with its reason; test and disconnect", async () => {
    mock.officialLinked("@ann");
    const el = await mount();
    expect(chip(el)).toBe("Linked");
    const commands = el.querySelector<HTMLInputElement>("#tg-commands")!;
    expect(commands.disabled).toBe(true);
    expect(commands.checked).toBe(false);
    expect(el.textContent).toContain("Only with your own bot. The official bot does not pass commands back to this app.");

    await click(el.querySelector("#tg-dealClosed")!);
    expect((await mock.status()).prefs.categories.dealClosed).toBe(false);

    await click(button(el, "Send test")!);
    expect(el.querySelector('[role="status"]')?.textContent).toBe("Test message sent.");

    await click(button(el, "Disconnect")!);
    expect(chip(el)).toBe("Not connected");
    expect((await mock.status()).official.linked).toBe(false);
    expect(el.querySelector("#tg-enabled")).toBeNull();
  });

  it("signed out: paused, no connect button", async () => {
    mock.setOfficial({ signedIn: false });
    const el = await mount();
    expect(chip(el)).toBe("Signed out");
    expect(el.textContent).toContain("Signed out · notifications are paused.");
    expect(button(el, "Connect @SentinelAleph_bot")).toBeUndefined();
  });

  it("blocked and unavailable are named, with the last error in the user's language", async () => {
    mock.officialLinked("@ann");
    mock.setOfficial({ blocked: true });
    mock.setError("tgBlocked");
    const el = await mount();
    expect(chip(el)).toBe("Not connected");
    expect(el.textContent).toContain("The chat blocked the official bot.");

    await act(async () => root!.unmount());
    host!.remove();
    mock.setOfficial({ blocked: false, unavailable: true, queued: 2 });
    mock.setError("tgUnavailable");
    const down = await mount();
    expect(chip(down)).toBe("Official bot unavailable");
    expect(down.textContent).toContain("Messages wait up to 10 minutes, then are dropped.");
    expect(down.textContent).toContain("Waiting to send");
  });
});
