import { describe, expect, it } from "vitest";
import { RISK_LIMITS_TABLE, type RiskLevel } from "./risk";

const MODEL_RS = "/src-tauri/src/risk/model.rs";
const RUST = import.meta.glob("/src-tauri/src/risk/model.rs", { query: "?raw", import: "default", eager: true }) as Record<
  string,
  string
>;

const camel = (s: string) => s.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());

/** `RiskLevel::limits()` read from the Rust source, keyed by the `RiskLimits::new` parameter names. */
function rustLimitsTable(): Record<string, Record<string, number>> {
  const src = RUST[MODEL_RS];
  expect(src, `${MODEL_RS} not found`).toBeTypeOf("string");
  const sig = /const fn new\(([^)]*)\)/.exec(src);
  expect(sig, "RiskLimits::new not found").not.toBeNull();
  const params = sig![1]
    .split(",")
    .map((p) => p.split(":")[0].trim())
    .filter((p) => p !== "" && p !== "level")
    .map(camel);
  const table: Record<string, Record<string, number>> = {};
  for (const m of src.matchAll(/RiskLevel::(\w+)\s*=>\s*RiskLimits::new\(self,([^)]*)\)/g)) {
    const values = m[2].split(",").map((v) => v.trim()).filter((v) => v !== "").map(Number);
    expect(values, `${m[1]} row`).toHaveLength(params.length);
    table[m[1].toLowerCase()] = Object.fromEntries(params.map((p, i) => [p, values[i]]));
  }
  return table;
}

describe("RISK_LIMITS_TABLE", () => {
  it("equals the Rust limits table in risk/model.rs", () => {
    const rust = rustLimitsTable();
    expect(Object.keys(rust).sort()).toEqual((Object.keys(RISK_LIMITS_TABLE) as RiskLevel[]).sort());
    expect(RISK_LIMITS_TABLE).toEqual(rust);
  });
});
