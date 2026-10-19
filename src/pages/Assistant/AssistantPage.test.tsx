// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import i18n from "@/i18n";
import { reset } from "@/lib/ipc/assistant/assistant.mock";
import { AssistantPage } from "./AssistantPage";
import { costText, filterModels, toolSummary } from "./assistantModel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
window.matchMedia ??= ((query: string) =>
  ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, onchange: null, dispatchEvent: () => false }) as MediaQueryList);
Element.prototype.scrollIntoView ??= function () {};

let root: Root | null = null;
let host: HTMLDivElement | null = null;
const settle = () => act(async () => new Promise((r) => setTimeout(r, 20)));

async function mount() {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(<AssistantPage />));
  await settle();
  await settle();
  return host;
}

const button = (el: ParentNode, text: string) => [...el.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === text);
const click = async (el: Element) => act(async () => (el as HTMLElement).click());

/** Sets a React-controlled textarea / input value. */
async function type(el: HTMLTextAreaElement | HTMLInputElement, value: string) {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, value);
  await act(async () => el.dispatchEvent(new Event("input", { bubbles: true })));
}

beforeAll(async () => {
  await i18n.changeLanguage("en");
});
beforeEach(() => {
  window.history.replaceState({}, "", "/#/assistant");
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  document.querySelectorAll(".ae-dialog").forEach((d) => d.remove());
});

describe("assistantModel", () => {
  it("shows the cost only as OpenRouter reported it", () => {
    const t = (k: string, o?: Record<string, unknown>) => i18n.t(k, o);
    expect(costText(t, { promptTokens: 1, completionTokens: 1, costUsd: 0.0013 }, "en")).toBe("Cost 0.0013 USD (reported by OpenRouter)");
    expect(costText(t, { promptTokens: 1, completionTokens: 1, costUsd: null }, "en")).toBe("Cost not reported by OpenRouter");
    expect(costText(t, null, "en")).toBe("Cost not reported by OpenRouter");
  });

  it("separates the tools read from the calls refused", () => {
    expect(
      toolSummary([
        { name: "list_bots", outcome: "ok" },
        { name: "list_bots", outcome: "ok" },
        { name: "draft_bot", outcome: "draft" },
        { name: "start_bot", outcome: "unknownTool" },
      ]),
    ).toEqual({ read: ["list_bots", "draft_bot"], refused: ["start_bot (unknownTool)"] });
  });

  it("filters models and keeps the chosen one listed", () => {
    const m = [
      { id: "a/x", name: "Alpha", contextLength: null, promptPerMtok: null, completionPerMtok: null },
      { id: "b/y", name: "Beta", contextLength: null, promptPerMtok: null, completionPerMtok: null },
    ];
    expect(filterModels(m, "bet", null).map((x) => x.id)).toEqual(["b/y"]);
    expect(filterModels(m, "bet", "a/x").map((x) => x.id)).toEqual(["a/x", "b/y"]);
  });
});

describe("Assistant page", () => {
  it("without a key: explains where to get one and who bills it", async () => {
    reset(false);
    const el = await mount();
    expect(el.textContent).toContain("Add your OpenRouter key to use the Assistant");
    expect(el.textContent).toContain("openrouter.ai/keys");
    expect(el.textContent).toContain("OpenRouter bills every message to your own OpenRouter account");
    expect(el.textContent).toContain("never to ribqa.com");
    expect(button(el, "Open openrouter.ai/keys")).toBeTruthy();
    expect(el.querySelector('textarea[placeholder="Ctrl+Enter sends"]')).toBeNull();
  });

  it("refuses a pasted value that is not an OpenRouter key", async () => {
    reset(false);
    const el = await mount();
    await type(el.querySelector<HTMLInputElement>('input[type="password"]')!, "sk-ant-not-this-one");
    await click(button(el, "Save key")!);
    await settle();
    expect(el.textContent).toContain("That is not an OpenRouter key");
  });

  it("sends a message and shows the answer with its cost line and a draft to open in New bot", async () => {
    reset(true);
    const el = await mount();
    await type(el.querySelector<HTMLTextAreaElement>('textarea[placeholder="Ctrl+Enter sends"]')!, "make me a dca bot");
    await click(button(el, "Send")!);
    await settle();
    expect(el.querySelectorAll('[data-testid="assistant-msg-user"]')).toHaveLength(1);
    expect(el.querySelectorAll('[data-testid="assistant-msg-assistant"]')).toHaveLength(1);
    expect(el.querySelector('[data-testid="assistant-cost"]')!.textContent).toContain("Cost not reported by OpenRouter");
    const draft = el.querySelector('[data-testid="assistant-draft"]')!;
    expect(draft.textContent).toContain("Nothing was created.");
    await click(button(draft, "Open in New bot")!);
    expect(window.location.hash).toBe("#/bots/dca/new?preset=dca_long_classic");
  });

  it("the master prompt is saved as a version, reset to default, and the guard is always shown", async () => {
    reset(true);
    const el = await mount();
    expect(el.textContent).toContain("Fixed rules sent first (not editable)");
    const prompt = [...el.querySelectorAll<HTMLTextAreaElement>("textarea")].find((t) => t.value.startsWith("Answer briefly"))!;
    await type(prompt, "Answer in one line.");
    await click(button(el, "Save version")!);
    await settle();
    expect(el.textContent).toContain("Saved versions (1 of 10)");
    expect(el.textContent).toContain("Edited");
    await click(button(el, "Reset to default")!);
    await settle();
    expect(el.textContent).toContain("Saved versions (2 of 10)");
    expect(el.textContent).toContain("Default");
  });

  it("in Turkish the fixed rules are said in Turkish; the English text sent to the model is one click further", async () => {
    reset(true);
    await i18n.changeLanguage("tr");
    try {
      const el = await mount();
      const rules = el.querySelector('[data-testid="assistant-guard-rules"]')!;
      expect(rules.querySelectorAll("li")).toHaveLength(5);
      expect(rules.textContent).toContain("Taslak yalnız bir öneridir");
      expect(rules.textContent).not.toMatch(/(the|cannot|never)/);
      const exact = el.querySelector(".ae-assist__guardexact")!;
      expect(exact.querySelector("summary")!.textContent).toBe("Modele gönderilen tam metin (İngilizce)");
      expect(exact.querySelector("pre")!.getAttribute("lang")).toBe("en");
      expect(el.textContent).toContain("Varsayılan talimatlar modele İngilizce gönderilir");
    } finally {
      await i18n.changeLanguage("en");
    }
  });

  it("deletes the conversation after a confirmation", async () => {
    reset(true);
    const el = await mount();
    await type(el.querySelector<HTMLTextAreaElement>('textarea[placeholder="Ctrl+Enter sends"]')!, "hello");
    await click(button(el, "Send")!);
    await settle();
    await click(button(el, "Delete conversation")!);
    const dialog = document.querySelector(".ae-dialog")!;
    expect(dialog.textContent).toContain("2 messages are removed from this computer");
    await click(button(dialog, "Delete conversation")!);
    await settle();
    expect(el.textContent).toContain("No messages");
  });
});
