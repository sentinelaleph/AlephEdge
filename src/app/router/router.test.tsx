// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RouterSnapshot } from "./router";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type RouterModule = typeof import("./router");

/** Fresh router module (its INITIAL_HASH is read at import) on a given starting hash. */
async function loadRouter(startHash: string): Promise<RouterModule> {
  window.history.replaceState(null, "", "/" + startHash);
  vi.resetModules();
  return import("./router");
}

let root: Root | null = null;
let host: HTMLDivElement | null = null;
let seen: RouterSnapshot | null = null;

async function mount(mod: RouterModule) {
  function Probe() {
    seen = mod.useRouter();
    return null;
  }
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(<Probe />));
}

/** What the browser does on back/forward or a hand-edited hash. */
async function popTo(hash: string) {
  await act(async () => {
    window.history.replaceState(window.history.state, "", hash);
    window.dispatchEvent(new PopStateEvent("popstate", { state: window.history.state }));
  });
}

beforeEach(() => {
  seen = null;
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("ensureInitialRoute", () => {
  it.each(["", "#", "#/"])("start hash %j lands on the dashboard", async (start) => {
    const mod = await loadRouter(start);
    await mount(mod);
    await act(async () => mod.ensureInitialRoute());
    expect(window.location.hash).toBe("#/dashboard");
    expect(seen?.route.id).toBe("dashboard");
  });

  it("keeps a deep link the user arrived with", async () => {
    const mod = await loadRouter("#/bots/dca?preset=p1");
    await mount(mod);
    await act(async () => mod.ensureInitialRoute());
    expect(window.location.hash).toBe("#/bots/dca?preset=p1");
    expect(seen?.route.id).toBe("dcaBots");
    expect(seen?.route.query.get("preset")).toBe("p1");
  });

  it("replays a deep link dropped by the sign-in gate", async () => {
    const mod = await loadRouter("#/presets/abc");
    window.history.replaceState(null, "", "/"); // a gate cleared the hash
    await mount(mod);
    await act(async () => mod.ensureInitialRoute());
    expect(window.location.hash).toBe("#/presets/abc");
    expect(seen?.route).toMatchObject({ id: "presetDetail", params: { presetId: "abc" } });
  });
});

describe("navigate", () => {
  it("pushes a hash entry and records where the user came from", async () => {
    const mod = await loadRouter("#/dashboard");
    await mount(mod);
    const before = window.history.length;
    await act(async () => {
      expect(mod.navigate("/bots/grid")).toBe(true);
    });
    expect(window.location.hash).toBe("#/bots/grid");
    expect(window.history.length).toBe(before + 1);
    expect(seen?.route.id).toBe("gridBots");
    expect(seen?.from).toBe("/dashboard");
    expect(seen?.kind).toBe("push");
  });

  it("adds the leading slash and replaces without a new entry", async () => {
    const mod = await loadRouter("#/dashboard");
    await mount(mod);
    const before = window.history.length;
    await act(async () => {
      mod.navigate("signals?tab=closed", { replace: true });
    });
    expect(window.location.hash).toBe("#/signals?tab=closed");
    expect(window.history.length).toBe(before);
    expect(seen?.route.query.get("tab")).toBe("closed");
  });

  it("a guard that says no keeps the user, and proceed() finishes later", async () => {
    const mod = await loadRouter("#/bots/dca/new");
    let held: (() => void) | null = null;
    function Guarded() {
      mod.useNavigationGuard(true, (proceed) => {
        held = proceed;
        return false;
      });
      seen = mod.useRouter();
      return null;
    }
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    await act(async () => root!.render(<Guarded />));

    await act(async () => {
      expect(mod.navigate("/history")).toBe(false);
    });
    expect(window.location.hash).toBe("#/bots/dca/new");
    expect(held).not.toBeNull();

    // Unmount the guard (the form was discarded), then proceed.
    await act(async () => root!.render(null));
    await act(async () => held!());
    expect(window.location.hash).toBe("#/history");
  });
});

describe("popstate", () => {
  it("a bare #/ goes to the dashboard, not Page not found", async () => {
    const mod = await loadRouter("#/signals");
    await mount(mod);
    await popTo("#/");
    expect(window.location.hash).toBe("#/dashboard");
    expect(seen?.route.id).toBe("dashboard");
    expect(seen?.kind).toBe("pop");
  });

  it("an unknown hash shows notFound", async () => {
    const mod = await loadRouter("#/signals");
    await mount(mod);
    await popTo("#/does-not-exist");
    expect(seen?.route.id).toBe("notFound");
  });

  it("back to a param route parses its params", async () => {
    const mod = await loadRouter("#/signals");
    await mount(mod);
    await popTo("#/backtest/r-42");
    expect(seen?.route).toMatchObject({ id: "backtestReport", params: { runId: "r-42" } });
  });
});
