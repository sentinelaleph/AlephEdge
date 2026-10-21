import { describe, expect, it } from "vitest";
import { buildMessage, DEFAULT_FORM, type BuilderForm } from "./tvMessage";

const build = (patch: Partial<BuilderForm>) => buildMessage({ ...DEFAULT_FORM, ...patch });

describe("TradingView message builder", () => {
  it("builds the manual buy the Rust reader expects (message.rs builder fixtures)", () => {
    expect(build({}).json).toBe('{"action":"buy","symbol":"{{ticker}}","tp_pct":3,"sl_pct":1.5,"price":"{{close}}","id":"{{timenow}}"}');
    expect(build({ action: "sell", size: "40", id: "none" }).json).toBe(
      '{"action":"sell","symbol":"{{ticker}}","tp_pct":3,"sl_pct":1.5,"size_usdt":40,"price":"{{close}}"}',
    );
  });

  it("a close carries no levels or size", () => {
    expect(build({ action: "close", size: "40", tp: "", sl: "" }).json).toBe('{"action":"close","symbol":"{{ticker}}","price":"{{close}}","id":"{{timenow}}"}');
  });

  it("strategy mode sends the order action and the position after it", () => {
    const out = build({ mode: "strategy", action: "close", price: true, id: "none" });
    expect(out.json).toBe(
      '{"action":"{{strategy.order.action}}","market_position":"{{strategy.market_position}}","symbol":"{{ticker}}","tp_pct":3,"sl_pct":1.5,"price":"{{close}}"}',
    );
    // Pine's alert_message is a JSON object TradingView pastes in: unquoted.
    const withMsg = build({ mode: "strategy", alertMessage: true, id: "none", price: false }).json!;
    expect(withMsg.endsWith(',"alert_message":{{strategy.order.alert_message}}}')).toBe(true);
    // Manual mode never adds it.
    expect(build({ alertMessage: true }).json).not.toContain("alert_message");
  });

  it("prices may be numbers or TradingView placeholders", () => {
    expect(build({ levels: "price", tp: "70000", sl: "{{plot_1}}", price: false, id: "none" }).json).toBe(
      '{"action":"buy","symbol":"{{ticker}}","tp":70000,"sl":"{{plot_1}}"}',
    );
    // A percent is never a placeholder.
    expect(build({ tp: "{{plot_0}}" }).error).toBe("levels");
  });

  it("passphrase, custom id and spot are written as given", () => {
    const json = build({ passphrase: "  pw1 ", id: "custom", customId: "trend-a", market: "spot" }).json!;
    expect(JSON.parse(json)).toEqual({
      action: "buy",
      symbol: "{{ticker}}",
      bot: "spot",
      tp_pct: 3,
      sl_pct: 1.5,
      price: "{{close}}",
      passphrase: "pw1",
      id: "trend-a",
    });
  });

  it("refuses what the reader would refuse", () => {
    expect(build({ tp: "" }).error).toBe("levels");
    expect(build({ sl: "0" }).error).toBe("pct");
    expect(build({ sl: "100" }).error).toBe("pct");
    expect(build({ levels: "price", tp: "0" }).error).toBe("levels");
    expect(build({ size: "4" }).error).toBe("size");
    expect(build({ size: "lots" }).error).toBe("size");
    expect(build({ market: "spot", action: "sell" }).error).toBe("spotShort");
    expect(build({ id: "custom", customId: "" }).error).toBe("customId");
    expect(build({ id: "custom", customId: 'a"b' }).error).toBe("customId");
    expect(build({ tp: "" }).json).toBeNull();
  });
});
