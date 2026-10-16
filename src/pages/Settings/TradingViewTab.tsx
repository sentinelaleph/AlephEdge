import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { navigate } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { PaperChip } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { StatusChip, type StatusKind } from "@/components/ui/StatusChip/StatusChip";
import { localizeError } from "@/lib/errorText";
import { formatNumber, formatTableTime } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/bridge";
import {
  TV_TEMPLATES,
  tvCreateUrl,
  tvRevoke,
  tvSetPrefs,
  tvStatus,
  type TvAlertResult,
  type TvCreatedUrl,
  type TvPrefs,
  type TvStatus,
  type TvTemplate,
} from "@/lib/ipc/tvlink/tvlink";
import { tradeRefusalText } from "@/pages/Positions/NewTrade";
import { SwitchRow } from "./TelegramPanel";

const POLL_MS = 10_000;

type Phase = "signedOut" | "unknown" | "none" | "active";

function phaseOf(s: TvStatus): Phase {
  if (!s.signedIn) return "signedOut";
  if (!s.server) return "unknown";
  return s.server.active ? "active" : "none";
}

const CHIP: Record<Phase, StatusKind> = { signedOut: "notConfigured", unknown: "error", none: "needsSetup", active: "connected" };

const DONE = new Set(["received", "opened", "closed"]);

/** An alert's outcome in the user's language: this panel's own codes first, then New trade's chain. */
export function tvOutcomeText(t: TFunction, i18nExists: (key: string) => boolean, raw: string): string {
  const head = raw.split("|")[0].trim();
  const own = `settings.tv.outcome.${head}`;
  if (i18nExists(own)) return t(own);
  return tradeRefusalText(t, i18nExists, raw);
}

type Copied = { what: string; ok: boolean } | null;

/**
 * Settings, TradingView: the webhook URL on Sentinel's relay, the two
 * switches, the message format and the last alerts the app read. Every
 * figure comes from the relay or from the Rust reader; nothing is filled in.
 */
export function TradingViewTab() {
  const { t, i18n } = useTranslation();
  const locale = i18n.language;
  const [st, setSt] = useState<TvStatus | null>(null);
  const [created, setCreated] = useState<TvCreatedUrl | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<Copied>(null);

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

  const run = async (call: () => Promise<void>) => {
    setBusy(true);
    setError(null);
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
  const setPrefs = (p: TvPrefs) =>
    run(async () => {
      setSt(await tvSetPrefs(p));
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

  const columns: DataColumn<TvAlertResult>[] = [
    {
      id: "at",
      header: t("settings.tv.col.time"),
      cell: (r) => formatTableTime(r.receivedAt ?? r.handledAt, locale),
      keep: true,
    },
    { id: "action", header: t("settings.tv.col.action"), cell: (r) => (r.action ? t(`settings.tv.action.${r.action}`) : "—") },
    {
      id: "symbol",
      header: t("settings.tv.col.symbol"),
      cell: (r) => (r.symbol ? [r.symbol, r.bot ? t(`bots.kind.${r.bot}`, { defaultValue: r.bot }) : null].filter(Boolean).join(" · ") : "—"),
      keep: true,
    },
    {
      id: "outcome",
      header: t("settings.tv.col.outcome"),
      cell: (r) => tvOutcomeText(t, exists, r.outcome),
      tone: (r) => (DONE.has(r.outcome) ? undefined : "down"),
      wrap: true,
      keep: true,
    },
  ];

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
                  { label: t("settings.tv.created"), value: server.createdAt ? formatTableTime(server.createdAt, locale) : "—" },
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
          </>
        )}
        {error ? (
          <p className="ae-error" role="alert">
            {error}
          </p>
        ) : null}
      </Panel>

      <Panel title={t("settings.tv.alertsTitle")} aside={<PaperChip />}>
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
            desc={t("settings.tv.executeDesc")}
            checked={st.prefs.execute}
            disabled={busy || !st.signedIn}
            onChange={(on) => void setPrefs({ receive: on ? true : st.prefs.receive, execute: on })}
          />
        </ul>
        <FactList
          rows={[
            { label: t("settings.tv.lastRead"), value: st.lastPollAt ? formatTableTime(st.lastPollAt, locale) : t("settings.tv.never") },
            ...(st.lastError ? [{ label: t("settings.tv.lastError"), value: localizeError(st.lastError), tone: "danger" as const }] : []),
          ]}
        />
      </Panel>

      <Panel title={t("settings.tv.howTitle")}>
        <ol className="ae-tg__steps">
          <li>{t("settings.tv.step1")}</li>
          <li>{t("settings.tv.step2")}</li>
          <li>{t("settings.tv.step3")}</li>
        </ol>
        <p className="ae-subtle">{t("settings.tv.twoFactor")}</p>
        <ul className="ae-tv__templates">
          {(Object.keys(TV_TEMPLATES) as TvTemplate[]).map((k) => (
            <li key={k}>
              <span className="ae-tv__tname">{t(`settings.tv.action.${k}`)}</span>
              <code className="ae-tg__code ae-tv__tpl">{TV_TEMPLATES[k]}</code>
              <Button variant="secondary" size="sm" onClick={() => void copy(k, TV_TEMPLATES[k])}>
                {copyLabel(k)}
              </Button>
            </li>
          ))}
        </ul>
        <ul className="ae-setlist">
          {(["action", "symbol", "levels", "pct", "bot", "size", "stale"] as const).map((k) => (
            <li key={k}>{t(`settings.tv.field.${k}`, { ticker: "{{ticker}}" })}</li>
          ))}
        </ul>
      </Panel>

      <Panel title={t("settings.tv.resultsTitle")}>
        <DataTable
          label={t("settings.tv.resultsTitle")}
          columns={columns}
          rows={st.results}
          rowKey={(r) => r.eventId}
          bare
          compact
          empty={<EmptyState title={t("settings.tv.resultsEmpty")} />}
        />
      </Panel>
    </>
  );
}
