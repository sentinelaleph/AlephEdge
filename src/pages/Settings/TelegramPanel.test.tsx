// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
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

describe("Settings, Notifications, Telegram panel", () => {
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
