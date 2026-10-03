// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { CredentialMeta } from "@/lib/ipc/vault/vault";
import { KeyList } from "./KeyList";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
// jsdom has no matchMedia; the collapse motion asks it for reduced motion.
window.matchMedia ??= ((query: string) =>
  ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, onchange: null, dispatchEvent: () => false }) as MediaQueryList);

vi.mock("@/lib/ipc/exchange/useExchangeCatalog", () => ({
  useExchangeCatalog: () => ({ exchanges: [{ id: "binance", name: "Binance" }], error: null, retry: () => undefined }),
}));

const KEY: CredentialMeta = { exchangeId: "binance", label: "main", permission: "tradeOnly", hasPassphrase: false, addedAt: 0 };

let root: Root | null = null;
let host: HTMLDivElement | null = null;

async function mount(node: React.ReactNode) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(node));
  return host;
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const button = (el: HTMLElement, text: string) =>
  [...el.querySelectorAll("button")].find((b) => b.textContent === text) as HTMLButtonElement;

async function type(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  await act(async () => {
    setter.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("KeyList", () => {
  it("renews a key under the same label, trade-only, with both fields required", async () => {
    const onRenew = vi.fn().mockResolvedValue(undefined);
    const el = await mount(<KeyList credentials={[KEY]} busy={false} onRemove={() => undefined} onRenew={onRenew} onRename={vi.fn()} />);
    await act(async () => button(el, "Renew").click());
    const inputs = el.querySelectorAll<HTMLInputElement>(".ae-keylist__form input");
    expect(inputs.length).toBe(2);
    const submit = button(el, "Save new key");
    expect(submit.disabled).toBe(true);
    await type(inputs[0], " new-key ");
    expect(submit.disabled).toBe(true);
    await type(inputs[1], "new-secret");
    expect(submit.disabled).toBe(false);
    await act(async () => submit.click());
    expect(onRenew).toHaveBeenCalledWith({
      exchangeId: "binance",
      label: "main",
      apiKey: "new-key",
      apiSecret: "new-secret",
      passphrase: undefined,
      permission: "tradeOnly",
    });
  });

  it("asks for the passphrase again when the stored key has one", async () => {
    const el = await mount(<KeyList credentials={[{ ...KEY, hasPassphrase: true }]} busy={false} onRemove={() => undefined} onRenew={vi.fn()} />);
    await act(async () => button(el, "Renew").click());
    expect(el.querySelectorAll(".ae-keylist__form input").length).toBe(3);
  });

  it("renames only to a different, non-blank label", async () => {
    const onRename = vi.fn().mockResolvedValue(undefined);
    const el = await mount(<KeyList credentials={[KEY]} busy={false} onRemove={() => undefined} onRename={onRename} />);
    await act(async () => button(el, "Rename").click());
    const input = el.querySelector<HTMLInputElement>(".ae-keylist__form input")!;
    const save = button(el, "Save label");
    expect(save.disabled).toBe(true);
    await type(input, "   ");
    expect(save.disabled).toBe(true);
    await type(input, " primary ");
    await act(async () => save.click());
    expect(onRename).toHaveBeenCalledWith("binance", "main", "primary");
  });

  it("flags a stored withdraw-enabled key for renewal", async () => {
    const el = await mount(<KeyList credentials={[{ ...KEY, permission: "withdrawEnabled" }]} busy={false} onRemove={() => undefined} />);
    expect(el.textContent).toContain("Withdrawals enabled: renew");
  });
});
