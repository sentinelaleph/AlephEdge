import { beforeAll, describe, expect, it } from "vitest";
import i18n from "@/i18n";
import en from "@/i18n/locales/en/en.json";
import type { SignalHealthInfo } from "./signal";
import {
  STREAM_ALERT_AFTER_SECS,
  streamAlertSeverity,
  streamErrorText,
  streamLabelKey,
  streamTone,
} from "./streamStatus";

type Phase = SignalHealthInfo["phase"];

const h = (phase: Phase, extra: Partial<SignalHealthInfo> = {}): SignalHealthInfo => ({
  connected: phase === "live",
  phase,
  ...extra,
});

beforeAll(async () => {
  await i18n.changeLanguage("en");
});

describe("streamAlertSeverity", () => {
  it("live and idle never alert, however long", () => {
    expect(streamAlertSeverity(h("live"))).toBeNull();
    expect(streamAlertSeverity(h("live", { notLiveSecs: 10_000 }))).toBeNull();
    expect(streamAlertSeverity(h("idle" as Phase, { notLiveSecs: 10_000 }))).toBeNull();
  });

  it("down always alerts as danger, even at 0 s", () => {
    expect(streamAlertSeverity(h("down"))).toBe("danger");
    expect(streamAlertSeverity(h("down", { notLiveSecs: 0 }))).toBe("danger");
  });

  it("retrying alerts (danger) only past the threshold", () => {
    expect(streamAlertSeverity(h("retrying"))).toBeNull();
    expect(streamAlertSeverity(h("retrying", { notLiveSecs: 30 }))).toBeNull();
    expect(streamAlertSeverity(h("retrying", { notLiveSecs: STREAM_ALERT_AFTER_SECS }))).toBeNull();
    expect(streamAlertSeverity(h("retrying", { notLiveSecs: STREAM_ALERT_AFTER_SECS + 1 }))).toBe("danger");
  });

  it("connecting alerts (warn) only past the threshold", () => {
    expect(streamAlertSeverity(h("connecting"))).toBeNull();
    expect(streamAlertSeverity(h("connecting", { notLiveSecs: STREAM_ALERT_AFTER_SECS }))).toBeNull();
    expect(streamAlertSeverity(h("connecting", { notLiveSecs: 61 }))).toBe("warn");
  });

  it("the threshold is 60 s", () => {
    expect(STREAM_ALERT_AFTER_SECS).toBe(60);
  });
});

describe("streamTone", () => {
  it("muted until loaded", () => {
    expect(streamTone(h("live"), false)).toBe("muted");
    expect(streamTone(h("down"), false)).toBe("muted");
  });

  it("follows the phase", () => {
    expect(streamTone(h("live"))).toBe("success");
    expect(streamTone(h("down"))).toBe("danger");
    expect(streamTone(h("connecting", { notLiveSecs: 500 }))).toBe("warn");
    expect(streamTone(h("retrying", { notLiveSecs: 5 }))).toBe("warn");
    expect(streamTone(h("retrying", { notLiveSecs: 61 }))).toBe("danger");
    expect(streamTone(h("idle" as Phase))).toBe("muted");
  });
});

describe("streamLabelKey", () => {
  const statusbar = (en as unknown as { statusbar: Record<string, string> }).statusbar;
  it.each(["live", "connecting", "retrying", "down", "idle"] as Phase[])("%s maps to an existing en key", (phase) => {
    const key = streamLabelKey(h(phase));
    expect(key.startsWith("statusbar.")).toBe(true);
    expect(typeof statusbar[key.slice("statusbar.".length)], key).toBe("string");
  });
});

describe("streamErrorText", () => {
  it("only while retrying or down, and only with an error", () => {
    expect(streamErrorText(h("live", { lastError: "noAccessToken" }))).toBeUndefined();
    expect(streamErrorText(h("connecting", { lastError: "noAccessToken" }))).toBeUndefined();
    expect(streamErrorText(h("retrying"))).toBeUndefined();
    expect(streamErrorText(h("down", { lastError: "" }))).toBeUndefined();
  });

  it("localizes a code and passes raw text through", () => {
    const errors = (en as unknown as { errors: Record<string, string> }).errors;
    expect(streamErrorText(h("down", { lastError: "noAccessToken" }))).toBe(errors.noAccessToken);
    expect(streamErrorText(h("retrying", { lastError: "tls handshake eof" }))).toBe("tls handshake eof");
  });
});
