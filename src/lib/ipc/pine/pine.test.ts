import { describe, expect, it } from "vitest";
import { PINE_CODES, PINE_LIMITS } from "./pine";
import { defaultSettings, mock, settingsRefusal } from "./pine.mock";

const RUST = import.meta.glob(["/src-tauri/src/bot/pine_settings.rs", "/src-tauri/src/bot/pinebot/*.rs", "/src-tauri/src/bot/engine/user_entry.rs"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const SETTINGS_RS = "/src-tauri/src/bot/pine_settings.rs";

/** `pub const NAME: T = <digits, '.', '*', '_'>;` read from the Rust source. */
function rustConsts(src: string): Record<string, number> {
  const out: Record<string, number> = {};
  for (const m of src.matchAll(/pub const (\w+): \w+ = ([\d._ *]+);/g)) {
    out[m[1]] = m[2]
      .split("*")
      .map((x) => Number(x.trim().replace(/_/g, "")))
      .reduce((a, b) => a * b, 1);
  }
  return out;
}

describe("Pine bot IPC", () => {
  it("states the same limits Rust checks", () => {
    const src = RUST[SETTINGS_RS];
    expect(src, `${SETTINGS_RS} not found`).toBeTypeOf("string");
    const c = rustConsts(src);
    expect({
      maxSymbols: c.PINE_MAX_SYMBOLS,
      maxScriptKb: c.PINE_MAX_SCRIPT_BYTES / 1024,
      warmupMin: c.PINE_WARMUP_MIN,
      warmupMax: c.PINE_WARMUP_MAX,
      warmupDefault: c.PINE_WARMUP_DEFAULT,
      opensMax: c.PINE_OPENS_MAX,
      opensDefault: c.PINE_OPENS_DEFAULT,
      staleMin: c.PINE_STALE_MIN_S,
      staleMax: c.PINE_STALE_MAX_S,
      staleDefault: c.PINE_STALE_DEFAULT_S,
      guardMax: c.PINE_PRICE_GUARD_MAX_PCT,
      guardDefault: c.PINE_PRICE_GUARD_DEFAULT_PCT,
      stopMin: c.PINE_STOP_MIN_PCT,
      stopMax: c.PINE_STOP_MAX_PCT,
      targetMin: c.PINE_TARGET_MIN_PCT,
      targetMax: c.PINE_TARGET_MAX_PCT,
      sizeMin: PINE_LIMITS.sizeMin,
    }).toEqual(PINE_LIMITS);
  });

  it("lists every pine code the Rust side returns, for the screens' i18n", () => {
    const found = new Set<string>();
    for (const src of Object.values(RUST)) {
      for (const m of src.matchAll(/"(pine[A-Z]\w*)/g)) found.add(m[1]);
    }
    expect(found.size).toBeGreaterThan(20);
    const listed = new Set<string>(PINE_CODES);
    expect([...found].filter((c) => !listed.has(c)).sort()).toEqual([]);
  });

  it("the preview has no script runtime and never trades", async () => {
    const info = await mock.compile("indicator('x')");
    expect(info.ok).toBe(false);
    expect(info.error?.message).toBe("pineRuntimeMissing");
    expect(await mock.results()).toEqual([]);
    await expect(mock.start()).rejects.toBe("botNotConfigured");
    const cfg = await mock.defaultConfig();
    expect(cfg.kind).toBe("pine");
    await expect(mock.saveConfig(cfg)).rejects.toBe("pineScriptEmpty");
    const saved = await mock.saveConfig({ ...cfg, pine: { ...defaultSettings(), script: "indicator('x')", symbols: [" btcusdt", "BTCUSDT"] } });
    expect(saved.pine?.symbols).toEqual(["BTCUSDT"]);
    const s = await mock.start();
    expect(s.running && s.symbols.every((x) => x.lastRunAt === null && x.lastFired.length === 0)).toBe(true);
    expect(settingsRefusal({ ...defaultSettings(), script: "x", symbols: ["BTCUSDT"], stopPct: null })).toBe("pineStopRequired");
  });
});
