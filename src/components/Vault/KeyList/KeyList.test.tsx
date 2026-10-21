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

const ed = vi.hoisted(() => ({ pending: vi.fn(), generate: vi.fn() }));
vi.mock("@/lib/ipc/vault/vault", async (orig) => ({
  ...(await orig<typeof import("@/lib/ipc/vault/vault")>()),
  vaultEd25519Pending: ed.pending,
  vaultEd25519Generate: ed.generate,
}));
/** A public key's shape only; the test never needs a real one. */
const PEM = "-----BEGIN PUBLIC KEY-----\nTEST-PUBLIC-KEY\n-----END PUBLIC KEY-----";

const KEY: CredentialMeta ={ exchangeId: "binance", label: "main", permission: "tradeOnly", hasPassphrase: false, addedAt: 0 };

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
      keyType: "hmac",
    });
  });

  it("renews with an Ed25519 key made in Rust: public key shown, no secret field, nothing secret sent", async () => {
    ed.pending.mockResolvedValue(null);
    ed.generate.mockResolvedValue(PEM);
    const onRenew = vi.fn().mockResolvedValue(undefined);
    const el = await mount(<KeyList credentials={[KEY]} busy={false} onRemove={() => undefined} onRenew={onRenew} />);
    await act(async () => button(el, "Renew").click());
    await act(async () => button(el, "Ed25519 key made here").click());
    const submit = button(el, "Save new key");
    expect(submit.disabled).toBe(true);
    await act(async () => button(el, "Create key pair").click());
    expect(ed.generate).toHaveBeenCalledTimes(1);
    expect(el.querySelector<HTMLTextAreaElement>(".ae-edsetup__pem")?.value).toBe(PEM);
    // Only the API key field: the private key never reaches the UI.
    const inputs = el.querySelectorAll<HTMLInputElement>(".ae-keylist__form input");
    expect(inputs.length).toBe(1);
    await type(inputs[0], "ed-api-key");
    expect(submit.disabled).toBe(false);
    await act(async () => submit.click());
    expect(onRenew).toHaveBeenCalledWith({
      exchangeId: "binance",
      label: "main",
      apiKey: "ed-api-key",
      apiSecret: "",
      passphrase: undefined,
      permission: "tradeOnly",
      keyType: "ed25519",
    });
  });

  it("shows the IP and age warnings with the Binance guide, and nothing for a safe key", async () => {
    const risky: CredentialMeta = {
      ...KEY,
      ipRestricted: false,
      safety: { noIpRestriction: true, ageDays: 400, ageFrom: "exchange", rotate: true },
    };
    const el = await mount(<KeyList credentials={[risky]} busy={false} onRemove={() => undefined} />);
    expect(el.querySelector('[data-testid="key-safety-ip"]')?.textContent).toContain("No IP restriction");
    expect(el.querySelector('[data-testid="key-safety-ip"]')?.textContent).toContain("Restrict access to trusted IPs only");
    expect(el.querySelector('[data-testid="key-safety-age"]')?.textContent).toBe("Created 400 days ago: older than 90 days, rotate it (Renew).");
    await act(async () => root?.unmount());
    host?.remove();
    const safe = await mount(
      <KeyList credentials={[{ ...KEY, safety: { noIpRestriction: false, ageDays: 10, ageFrom: "vault", rotate: false } }]} busy={false} onRemove={() => undefined} />,
    );
    expect(safe.querySelector('[data-testid^="key-safety-"]')).toBeNull();
  });

  it("checks a Binance key again on demand", async () => {
    const onRecheck = vi.fn().mockResolvedValue(undefined);
    const el = await mount(<KeyList credentials={[{ ...KEY, inUse: true }]} busy={false} onRemove={() => undefined} onRecheck={onRecheck} />);
    await act(async () => button(el, "Check again").click());
    expect(onRecheck).toHaveBeenCalledWith("binance", "main");
    expect(el.textContent).toContain("Checked with the exchange.");
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

  it("says which of two keys for one exchange the app signs with", async () => {
    const el = await mount(
      <KeyList
        credentials={[
          { ...KEY, label: "old", inUse: true },
          { ...KEY, label: "v2", inUse: false },
        ]}
        busy={false}
        onRemove={() => undefined}
      />,
    );
    expect(el.querySelectorAll('[data-testid="key-in-use"]')).toHaveLength(1);
    expect(el.querySelector('[data-testid="key-not-used"]')?.textContent).toBe(
      'Not used: the app signs with "old". Renew that key or remove one of them.',
    );
  });

  it("marks nothing when an exchange has one key", async () => {
    const el = await mount(<KeyList credentials={[{ ...KEY, inUse: true }]} busy={false} onRemove={() => undefined} />);
    expect(el.querySelector('[data-testid="key-in-use"]')).toBeNull();
    expect(el.querySelector('[data-testid="key-not-used"]')).toBeNull();
  });
});
