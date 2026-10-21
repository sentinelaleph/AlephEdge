import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Link, navigate } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { Chip, LiveChip, PaperChip } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn, type PriorityBreakpoints } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { StatusChip, type StatusKind } from "@/components/ui/StatusChip/StatusChip";
import { localizeError } from "@/lib/errorText";
import { formatLatency, formatNumber, formatPrice, formatSignedPercent, formatTableTime, NO_VALUE } from "@/lib/format";
import { botMode } from "@/lib/ipc/bot/bot";
import { errorMessage } from "@/lib/ipc/bridge";
import {
  tvCreateUrl,
  tvRevoke,
  tvSendTest,
  tvSetNotice,
  tvSetPrefs,
  tvResults,
  tvStatus,
  onTvResult,
  type TvAlertResult,
  type TvCreatedUrl,
  type TvPrefs,
  type TvStatus,
} from "@/lib/ipc/tvlink/tvlink";
import { tradeRefusalText } from "@/pages/Positions/NewTrade";
import { SwitchRow } from "./TelegramPanel";
import { TvMessageBuilder } from "./TvMessageBuilder";

const POLL_MS = 10_000;
/**
 * The results table sits in the Settings content column (about 500-800 px
 * wide): its columns are short numbers, so they show from narrower widths
 * than the app default, and the fit loop folds the rest.
 */
export const RESULTS_BREAKPOINTS: PriorityBreakpoints = { p2: 560, p3: 720 };
export const TV_BOT_PATH = "/bots/tradingview";

type Phase = "signedOut" | "unknown" | "none" | "active";

function phaseOf(s: TvStatus): Phase {
  if (!s.signedIn) return "signedOut";
  if (!s.server) return "unknown";
  return s.server.active ? "active" : "none";
}

const CHIP: Record<Phase, StatusKind> = { signedOut: "notConfigured", unknown: "error", none: "needsSetup", active: "connected" };

const DONE = new Set(["received", "opened", "closed", "flipped", "tvTestOnly"]);

/** An alert's outcome in the user's language: this panel's own codes first, then New trade's chain. */
export function tvOutcomeText(t: TFunction, i18nExists: (key: string) => boolean, raw: string): string {
  const [head, ...rest] = raw.split("|");
  const own = `settings.tv.outcome.${head.trim()}`;
  // The symbol's smallest order: symbol | minimum | size asked | max size.
  if (head.trim() === "tvBelowSymbolMinimum" && i18nExists(own)) {
    const [symbol = "", min = "", size = "", max = ""] = rest.map((x) => x.trim());
    return t(own, { symbol, min, size, max });
  }
  if (i18nExists(own)) {
    const detail = rest.join("|").trim();
    // A wrapper code (a pending order, a flip leg) names the code it wraps.
    if (detail && ["livePending", "tvFlipCloseFailed", "tvFlipOpenRefused"].includes(head.trim())) {
      return `${t(own)} · ${tvOutcomeText(t, i18nExists, detail)}`;
    }
    // A bot kind names the holder in words.
    const holder = ["tvCloseNotOwn", "tvSymbolHeldBy"].includes(head.trim())
      ? detail === "strategy"
        ? t("settings.tv.holderStrategy")
        : t(`bots.kind.${detail}`, { defaultValue: detail })
      : detail;
    return t(own, { detail: holder });
  }
  return tradeRefusalText(t, i18nExists, raw);
}

/**
 * The bot an alert row names: the bot that handled it, else the bot it was
 * for. A futures alert is always the TradingView bot's, never the Futures
 * bot's (alerts never trade on the signal bots). Null when unreadable.
 */
export function alertBotLabel(t: TFunction, r: Pick<TvAlertResult, "bot" | "botName">): string | null {
  if (r.botName) return r.botName === "spot" ? t("bots.kind.spot") : r.botName;
  if (!r.bot) return null;
  return t(r.bot === "spot" ? "bots.kind.spot" : "bots.kind.tradingview");
}

type Copied = { what: string; ok: boolean } | null;

/**
 * Settings, TradingView: the webhook URL on Sentinel's relay, the link's
 * online state, the two switches, the server's offline notice, the message
 * builder and the last alerts the app read. Every figure comes from the
 * relay or from the Rust reader; nothing is filled in.
 */
export function TradingViewTab() {
  const { t, i18n } = useTranslation();
  const locale = i18n.language;
  const { desk } = useDeskContext();
  const tvBot = desk.loaded ? (desk.status.tradingview ?? null) : null;
  const tvLive = botMode(tvBot) === "live";
  const [st, setSt] = useState<TvStatus | null>(null);
  const [created, setCreated] = useState<TvCreatedUrl | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<Copied>(null);
  const [testSent, setTestSent] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setSt(await tvStatus());
    } catch (e) {
      setError(errorMessage(e, t("settings.tv.failed")));
    }
  }, [t]);

  useEffect(() => {
    void refresh();
    const id = window.setInterval(() => void refresh(), POLL_MS);
    return () => window.clearInterval(id);
  }, [refresh]);

  // Each handled alert: the rows at once, from the app's own store (no relay
  // round trip), and the status (online, last read) right after.
  useEffect(() => {
    let alive = true;
    const pending = onTvResult(() => {
      void tvResults()
        .then((results) => alive && setSt((prev) => (prev ? { ...prev, results } : prev)))
        .catch(() => undefined);
    });
    return () => {
      alive = false;
      void pending.then((unlisten) => unlisten());
    };
  }, []);

  const run = async (call: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    setTestSent(false);
    try {
      await call();
    } catch (e) {
      setError(errorMessage(e, t("settings.tv.failed")));
    } finally {
      setBusy(false);
    }
  };

  const create = () =>
    run(async () => {
      setCreated(null);
      setCreated(await tvCreateUrl());
      setSt(await tvStatus());
    });
  const revoke = () =>
    run(async () => {
      setCreated(null);
      setSt(await tvRevoke());
    });
  const sendTest = () =>
    run(async () => {
      setSt(await tvSendTest());
      setTestSent(true);
    });
  const setPrefs = (p: TvPrefs) =>
    run(async () => {
      setSt(await tvSetPrefs(p));
    });
  const setNotice = (on: boolean) =>
    run(async () => {
      setSt(await tvSetNotice(on));
    });

  const copy = async (what: string, text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied({ what, ok: true });
    } catch {
      setCopied({ what, ok: false });
    }
  };
  const copyLabel = (what: string) =>
    copied?.what === what ? (copied.ok ? t("settings.tv.copied") : t("settings.tv.copyFailed")) : t("settings.tv.copy");

  if (!st) {
    return (
      <Panel title={t("settings.tv.urlTitle")}>
        {error ? (
          <p className="ae-error" role="alert">
            {error}
          </p>
        ) : (
          <p className="ae-muted">{t("settings.tv.loading")}</p>
        )}
      </Panel>
    );
  }

  const phase = phaseOf(st);
  const server = st.server;
  const exists = (k: string) => i18n.exists(k);
  const num = (v: number | null | undefined, digits = 6) =>
    v === null || v === undefined ? NO_VALUE : formatNumber(v, locale, { maximumFractionDigits: digits });

  const columns: DataColumn<TvAlertResult>[] = [
    {
      id: "at",
      header: t("settings.tv.col.time"),
      cell: (r) => formatTableTime(r.receivedAt ?? r.handledAt, locale),
      keep: true,
    },
    {
      id: "action",
      header: t("settings.tv.col.action"),
      cell: (r) => (
        <>
          {r.strategy ?? (r.action ? t(`settings.tv.action.${r.action}`) : NO_VALUE)}
          {r.test ? (
            <>
              {" "}
              <Chip tone="warn">{t("settings.tv.testChip")}</Chip>
            </>
          ) : null}
        </>
      ),
    },
    {
      id: "symbol",
      header: t("settings.tv.col.symbol"),
      cell: (r) => r.symbol ?? NO_VALUE,
      keep: true,
    },
    {
      id: "bot",
      header: t("settings.tv.col.bot"),
      priority: 3,
      cell: (r) =>
        r.botName ? (
          <span className="ae-namecell">
            {alertBotLabel(t, r)}
            {r.mode === "live" ? <LiveChip /> : r.mode === "paper" ? <PaperChip /> : null}
          </span>
        ) : (
          (alertBotLabel(t, r) ?? NO_VALUE)
        ),
    },
    {
      id: "outcome",
      header: t("settings.tv.col.outcome"),
      cell: (r) => tvOutcomeText(t, exists, r.outcome),
      tone: (r) => (DONE.has(r.outcome) ? undefined : "down"),
      wrap: true,
      keep: true,
    },
    {
      id: "qty",
      header: t("settings.tv.col.qty"),
      headerTip: t("settings.tv.col.qtyTip"),
      numeric: true,
      priority: 3,
      cell: (r) => (r.requestedQty == null && r.filledQty == null ? NO_VALUE : `${num(r.requestedQty)} / ${num(r.filledQty)}`),
    },
    {
      id: "fill",
      header: t("settings.tv.col.fill"),
      numeric: true,
      priority: 2,
      cell: (r) => (r.avgFill == null ? NO_VALUE : formatPrice(r.avgFill, locale)),
    },
    {
      id: "slip",
      header: t("settings.tv.col.slippage"),
      headerTip: t("settings.tv.col.slippageTip"),
      numeric: true,
      cell: (r) => (r.slippagePct == null ? NO_VALUE : formatSignedPercent(r.slippagePct, locale, 2)),
      tone: (r) => (r.slippagePct != null && r.slippagePct > 0 ? "down" : undefined),
    },
    {
      id: "latency",
      header: t("settings.tv.col.latency"),
      headerTip: t("settings.tv.col.latencyTip"),
      numeric: true,
      cell: (r) => (r.latencyMs == null ? NO_VALUE : formatLatency(r.latencyMs, locale)),
    },
  ];

  const linkState = !st.prefs.receive ? "off" : st.online ? "online" : "offline";
  const notice = st.notice ?? null;

  return (
    <>
      <Panel title={t("settings.tv.urlTitle")} aside={<StatusChip status={CHIP[phase]} label={t(`settings.tv.state.${phase}`)} />}>
        {phase === "signedOut" ? (
          <>
            <p className="ae-muted">{t("settings.tv.signedOut")}</p>
            <div className="ae-tg__actions">
              <Button size="sm" onClick={() => navigate("/account")}>
                {t("settings.tv.signIn")}
              </Button>
            </div>
          </>
        ) : phase === "unknown" ? (
          <p className="ae-error" role="alert">
            {localizeError(st.serverError ?? "tvBadResponse")}
          </p>
        ) : (
          <>
            {server && server.active ? (
              <FactList
                rows={[
                  { label: t("settings.tv.created"), value: server.createdAt ? formatTableTime(server.createdAt, locale) : NO_VALUE },
                  {
                    label: t("settings.tv.lastReceived"),
                    value: server.lastReceivedAt ? formatTableTime(server.lastReceivedAt, locale) : t("settings.tv.never"),
                  },
                  { label: t("settings.tv.received24h"), value: formatNumber(server.received24h, locale) },
                  { label: t("settings.tv.pending"), value: formatNumber(server.pending, locale) },
                ]}
              />
            ) : (
              <p className="ae-muted">{t("settings.tv.noUrl")}</p>
            )}

            {created ? (
              <div className="ae-tv__once" role="status">
                <code className="ae-tg__code ae-tv__url">{created.url}</code>
                <Button variant="secondary" size="sm" onClick={() => void copy("url", created.url)}>
                  {copyLabel("url")}
                </Button>
                <p className="ae-subtle">{t("settings.tv.shownOnce")}</p>
              </div>
            ) : null}

            <div className="ae-tg__actions">
              <Button size="sm" variant={server?.active ? "secondary" : "primary"} disabled={busy} onClick={() => void create()}>
                {server?.active ? t("settings.tv.rotate") : t("settings.tv.create")}
              </Button>
              {server?.active ? (
                <Button size="sm" variant="danger" disabled={busy} onClick={() => void revoke()}>
                  {t("settings.tv.revoke")}
                </Button>
              ) : null}
            </div>
            {server?.active ? <p className="ae-subtle">{t("settings.tv.rotateNote")}</p> : null}
            {server?.active ? (
              <>
                <div className="ae-tg__actions">
                  <Button size="sm" variant="secondary" disabled={busy || !st.prefs.receive} onClick={() => void sendTest()}>
                    {t("settings.tv.sendTest")}
                  </Button>
                </div>
                {!st.prefs.receive ? <p className="ae-subtle">{t("settings.tv.testNeedsReceive")}</p> : null}
                {testSent ? (
                  <p className="ae-subtle" role="status">
                    {t("settings.tv.testSent")}
                  </p>
                ) : null}
              </>
            ) : null}
            <p className="ae-subtle">{t("settings.tv.paidPlan")}</p>
          </>
        )}
        {error ? (
          <p className="ae-error" role="alert">
            {error}
          </p>
        ) : null}
      </Panel>

      <Panel title={t("settings.tv.alertsTitle")} aside={tvBot ? tvLive ? <LiveChip /> : <PaperChip /> : null}>
        <FactList
          rows={[
            {
              label: t("settings.tv.bot"),
              value: tvBot ? (
                <Link to={TV_BOT_PATH} className="ae-link">
                  {[tvBot.tv?.name ?? t("bots.kind.tradingview"), t(desk.status.tradingviewRunning ? "settings.tv.botRunning" : "settings.tv.botStopped")].join(" · ")}
                </Link>
              ) : (
                <Link to={TV_BOT_PATH} className="ae-link">
                  {t("settings.tv.botNone")}
                </Link>
              ),
              tone: tvBot ? undefined : "warn",
            },
            {
              label: t("settings.tv.link"),
              value: [
                t(`settings.tv.linkState.${linkState}`),
                st.transport ? t(`settings.tv.transport.${st.transport}`) : null,
                st.transport === "poll" && st.streamError ? t("settings.tv.streamFailed", { reason: localizeError(st.streamError) }) : null,
              ]
                .filter(Boolean)
                .join(" · "),
              tone: linkState === "offline" ? "danger" : linkState === "off" ? "muted" : undefined,
            },
            {
              label: t("settings.tv.lastSeen"),
              value: server?.lastSeenAt ? formatTableTime(server.lastSeenAt, locale) : t("settings.tv.never"),
              info: t("settings.tv.lastSeenInfo"),
            },
            { label: t("settings.tv.lastRead"), value: st.lastPollAt ? formatTableTime(st.lastPollAt, locale) : t("settings.tv.never") },
            ...((st.pending ?? 0) > 0 ? [{ label: t("settings.tv.pendingOrders"), value: formatNumber(st.pending ?? 0, locale), tone: "danger" as const }] : []),
            ...(st.lastError ? [{ label: t("settings.tv.lastError"), value: localizeError(st.lastError), tone: "danger" as const }] : []),
          ]}
        />
        <ul className="ae-notify">
          <SwitchRow
            id="tv-receive"
            name={t("settings.tv.receive")}
            desc={t("settings.tv.receiveDesc")}
            checked={st.prefs.receive}
            disabled={busy || !st.signedIn}
            onChange={(on) => void setPrefs({ receive: on, execute: on ? st.prefs.execute : false })}
          />
          <SwitchRow
            id="tv-execute"
            name={t("settings.tv.execute")}
            desc={t(tvLive ? "settings.tv.executeDescLive" : "settings.tv.executeDesc")}
            checked={st.prefs.execute}
            disabled={busy || !st.signedIn}
            onChange={(on) => void setPrefs({ receive: on ? true : st.prefs.receive, execute: on })}
          />
          <SwitchRow
            id="tv-notice"
            name={t("settings.tv.notice")}
            desc={
              notice?.telegram === "linked"
                ? t("settings.tv.noticeDesc", { s: notice.offlineAfterS })
                : t("settings.tv.noticeNoTelegram")
            }
            checked={notice?.notifyOffline === true}
            disabled={busy || !notice || notice.telegram !== "linked"}
            onChange={(on) => void setNotice(on)}
          />
        </ul>
        {tvLive && notice && !st.noticeTouched && notice.telegram === "linked" ? <p className="ae-subtle">{t("settings.tv.noticeDefault")}</p> : null}
      </Panel>

      <Panel title={t("settings.tv.howTitle")}>
        <ol className="ae-tg__steps">
          <li>{t("settings.tv.step1")}</li>
          <li>{t("settings.tv.step2")}</li>
          <li>{t("settings.tv.step3")}</li>
        </ol>
        <p className="ae-subtle">{t("settings.tv.twoFactor")}</p>
        <ul className="ae-setlist">
          {(["action", "symbol", "levels", "pct", "bot", "size", "stale"] as const).map((k) => (
            <li key={k}>{t(`settings.tv.field.${k}`, { ticker: "{{ticker}}" })}</li>
          ))}
        </ul>
      </Panel>

      <TvMessageBuilder onCopy={(text) => void copy("built", text)} copyLabel={copyLabel("built")} />

      <Panel title={t("settings.tv.resultsTitle")}>
        {/* Narrow: headers wrap and the bot / quantity / fill columns fold
            into the row details first, so slippage and latency stay in view. */}
        <div className="ae-tv__results">
          <DataTable
            label={t("settings.tv.resultsTitle")}
            columns={columns}
            rows={st.results}
            rowKey={(r) => r.eventId}
            bare
            compact
            breakpoints={RESULTS_BREAKPOINTS}
            empty={<EmptyState title={t("settings.tv.resultsEmpty")} />}
          />
        </div>
      </Panel>
    </>
  );
}
