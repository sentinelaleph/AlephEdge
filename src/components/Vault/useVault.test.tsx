// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { CredentialMeta, VaultStatus } from "@/lib/ipc/vault/vault";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const mocks = vi.hoisted(() => ({
  status: { state: "unlocked", credentialCount: 1, idleTimeoutMinutes: 30 } as VaultStatus,
  list: vi.fn(),
}));

vi.mock("@/lib/ipc/vault/vault", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/vault/vault")>();
  return {
    ...actual,
    vaultStatus: vi.fn(async () => mocks.status),
    vaultListCredentials: mocks.list,
    onVaultAutoLocked: vi.fn(async () => () => undefined),
  };
});

import { useVault, type VaultController } from "./useVault";

const KEY: CredentialMeta = { exchangeId: "binance", label: "v2", permission: "tradeOnly", hasPassphrase: false, addedAt: 0, inUse: true };

let root: Root | null = null;
let seen: VaultController | null = null;

function Probe() {
  seen = useVault();
  return null;
}

async function mount() {
  const host = document.createElement("div");
  root = createRoot(host);
  await act(async () => root!.render(<Probe />));
  await act(async () => undefined);
}

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  seen = null;
  mocks.list.mockReset();
});

describe("useVault on load", () => {
  it("lists the keys of a vault that is already unlocked (a webview reload)", async () => {
    mocks.status = { state: "unlocked", credentialCount: 1, idleTimeoutMinutes: 30 };
    mocks.list.mockResolvedValue([KEY]);
    await mount();
    expect(seen?.status.state).toBe("unlocked");
    expect(seen?.credentials).toEqual([KEY]);
  });

  it("asks for no keys while the vault is locked", async () => {
    mocks.status = { state: "locked", credentialCount: 1, idleTimeoutMinutes: 30 };
    mocks.list.mockResolvedValue([KEY]);
    await mount();
    expect(mocks.list).not.toHaveBeenCalled();
    expect(seen?.credentials).toEqual([]);
  });
});
