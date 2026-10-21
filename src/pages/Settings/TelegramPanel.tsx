import { useCallback, useEffect, useState, type FormEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import { FactList, Panel, type Fact } from "@/components/ui/Panel/Panel";
import { SegmentedControl } from "@/components/ui/SegmentedControl/SegmentedControl";
import { StatusChip, type StatusKind } from "@/components/ui/StatusChip/StatusChip";
import { TextField } from "@/components/ui/TextField/TextField";
import { errorMessage } from "@/lib/ipc/bridge";
import {
  TELEGRAM_CATEGORIES,
  telegramLinkCode,
  telegramOfficialLink,
  telegramOfficialOpen,
  telegramOfficialUnlink,
  telegramRemove,
  telegramSetMode,
  telegramSetPrefs,
  telegramSetToken,
  telegramStartOver,
  telegramStatus,
  telegramTest,
  type TelegramMode,
  type TelegramPrefs,
  type TelegramStatus,
} from "@/lib/ipc/telegram/telegram";
import { localizeError } from "@/lib/errorText";
import { formatTableTime } from "@/lib/format";

/** While a code waits for /start the panel re-reads quickly; otherwise slowly. */
const POLL_LINKING_MS = 2_000;
/** Official bot: the server is asked every 3 s while its code is valid. */
const POLL_OFFICIAL_LINKING_MS = 3_000;
const POLL_IDLE_MS = 15_000;

type Phase = "none" | "noChat" | "waiting" | "linked";

function phaseOf(s: TelegramStatus): Phase {
  if (!s.tokenSet) return "none";
  if (s.linked) return "linked";
  return s.linking ? "waiting" : "noChat";
}

const CHIP: Record<Phase, StatusKind> = { none: "notConfigured", noChat: "needsSetup", waiting: "connecting", linked: "connected" };

type OfficialPhase = "signedOut" | "unavailable" | "blocked" | "linked" | "waiting" | "none";

function officialPhaseOf(s: TelegramStatus): OfficialPhase {
  const o = s.official;
  if (o.signedIn === false) return "signedOut";
  if (o.available === false || o.unavailable) return "unavailable";
  if (o.blocked) return "blocked";
  if (o.linked) return "linked";
  return o.linking ? "waiting" : "none";
}

const OFFICIAL_CHIP: Record<OfficialPhase, StatusKind> = {
  signedOut: "paused",
  unavailable: "down",
  blocked: "disconnected",
  linked: "connected",
  waiting: "connecting",
  none: "needsSetup",
};

/**
 * Settings, Notifications: Telegram. Two ways to send: Sentinel's official
 * bot (signed-in session, through ribqa.com, no token) or the user's own bot
 * (token in the keychain through Rust, never read back, straight from this
 * computer). Either chat is linked by a one-time code.
 */
export function TelegramPanel() {
  const { t, i18n } = useTranslation();
  const [st, setSt] = useState<TelegramStatus | null>(null);
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setSt(await telegramStatus());
    } catch (e) {
      setError(errorMessage(e, t("settings.telegram.failed")));
    }
  }, [t]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const official = st?.mode === "official";
  const waiting = st !== null && (official ? st.official.linking !== null && !st.official.linked : phaseOf(st) === "waiting");
  const tokenSet = st?.tokenSet ?? false;
  const polls = official || tokenSet;
  useEffect(() => {
    if (!polls) return;
    const fast = official ? POLL_OFFICIAL_LINKING_MS : POLL_LINKING_MS;
    const id = window.setInterval(() => void refresh(), waiting ? fast : POLL_IDLE_MS);
    return () => window.clearInterval(id);
  }, [refresh, polls, official, waiting]);

  const run = async (call: () => Promise<TelegramStatus>, done?: string): Promise<boolean> => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      setSt(await call());
      if (done) setNotice(done);
      return true;
    } catch (e) {
      setError(errorMessage(e, t("settings.telegram.failed")));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const saveToken = async (e: FormEvent) => {
    e.preventDefault();
    if (await run(() => telegramSetToken(token))) setToken("");
  };

  const setPrefs = (patch: Partial<TelegramPrefs>) => {
    if (!st) return;
    void run(() => telegramSetPrefs({ ...st.prefs, ...patch }));
  };

  const setMode = (mode: TelegramMode) => {
    if (!st || mode === st.mode) return;
    void run(() => telegramSetMode(mode));
  };

  if (!st) {
    return (
      <Panel title={t("settings.telegram.title")}>
        {error ? <p className="ae-error" role="alert">{error}</p> : <p className="ae-muted">{t("settings.telegram.loading")}</p>}
      </Panel>
    );
  }

  const locale = i18n.language;
  const officialBot = `@${st.official.bot}`;
  const chip = official ? (
    <StatusChip status={OFFICIAL_CHIP[officialPhaseOf(st)]} label={t(`settings.telegram.officialState.${officialPhaseOf(st)}`)} />
  ) : (
    <StatusChip status={CHIP[phaseOf(st)]} label={t(`settings.telegram.state.${phaseOf(st)}`)} />
  );

  const categoryRows = (
    <>
      <SwitchRow
        id="tg-enabled"
        name={t("settings.telegram.notifications")}
        desc={t("settings.telegram.notificationsDesc")}
        checked={st.prefs.enabled}
        disabled={busy}
        onChange={(on) => setPrefs({ enabled: on })}
      />
      {TELEGRAM_CATEGORIES.map((c) => (
        <SwitchRow
          key={c}
          id={`tg-${c}`}
          name={t(`alerts.category.${c}`)}
          desc={t(`settings.telegram.desc.${c}`)}
          checked={st.prefs.categories[c]}
          disabled={busy || !st.prefs.enabled}
          onChange={(on) => setPrefs({ categories: { ...st.prefs.categories, [c]: on } })}
        />
      ))}
    </>
  );

  return (
    <Panel title={t("settings.telegram.title")} aside={chip}>
      {st.settingsUnreadable !== undefined ? (
        <div className="ae-banner" data-tone="danger" role="alert">
          <span>
            {t(st.settingsUnreadable ? "liveMode.telegramUnreadable.kept" : "liveMode.telegramUnreadable.notKept", { key: st.settingsUnreadable })}
          </span>
          <Button size="xs" disabled={busy} onClick={() => void run(() => telegramStartOver())}>
            {t("liveMode.telegramUnreadable.startOver")}
          </Button>
        </div>
      ) : null}
      <div className="ae-tg__mode">
        <SegmentedControl
          label={t("settings.telegram.mode.label")}
          value={st.mode}
          disabled={busy}
          fill
          wrap
          size="sm"
          onChange={setMode}
          options={[
            { value: "official", label: t("settings.telegram.mode.official", { bot: officialBot }) },
            { value: "own", label: t("settings.telegram.mode.own") },
          ]}
        />
      </div>

      {official ? (
        <OfficialBody st={st} busy={busy} locale={locale} run={run} categoryRows={categoryRows} />
      ) : (
        <OwnBody
          st={st}
          busy={busy}
          locale={locale}
          token={token}
          setToken={setToken}
          saveToken={saveToken}
          run={run}
          setPrefs={setPrefs}
          categoryRows={categoryRows}
        />
      )}

      {error ? <p className="ae-error" role="alert">{error}</p> : null}
      {notice ? <p className="ae-subtle" role="status">{notice}</p> : null}

      {official ? (
        st.official.linked ? (
          <div className="ae-tg__actions">
            <Button
              variant="secondary"
              size="sm"
              disabled={busy || st.official.signedIn === false}
              onClick={() => void run(telegramTest, t("settings.telegram.testSent"))}
            >
              {t("settings.telegram.sendTest")}
            </Button>
            <Button variant="danger" size="sm" disabled={busy || st.official.signedIn === false} onClick={() => void run(telegramOfficialUnlink)}>
              {t("settings.telegram.disconnect")}
            </Button>
          </div>
        ) : null
      ) : phaseOf(st) !== "none" ? (
        <div className="ae-tg__actions">
          {phaseOf(st) === "linked" ? (
            <Button variant="secondary" size="sm" disabled={busy} onClick={() => void run(telegramTest, t("settings.telegram.testSent"))}>
              {t("settings.telegram.sendTest")}
            </Button>
          ) : null}
          <Button variant="danger" size="sm" disabled={busy} onClick={() => void run(telegramRemove)}>
            {t("settings.telegram.remove")}
          </Button>
        </div>
      ) : null}
    </Panel>
  );
}

type Run = (call: () => Promise<TelegramStatus>, done?: string) => Promise<boolean>;

function OfficialBody(props: { st: TelegramStatus; busy: boolean; locale: string; run: Run; categoryRows: ReactNode }) {
  const { st, busy, locale, run, categoryRows } = props;
  const { t } = useTranslation();
  const o = st.official;
  const bot = `@${o.bot}`;
  const signedOut = o.signedIn === false;
  const unavailable = o.available === false || o.unavailable;

  const facts: Fact[] = [
    { label: t("settings.telegram.bot"), value: bot },
    {
      label: t("settings.telegram.chat"),
      value: o.linked && !o.blocked ? (o.chatLabel ?? t("settings.telegram.linkedNoName")) : t("settings.telegram.notConnected"),
      tone: o.linked && !o.blocked ? undefined : "warn",
    },
  ];
  if (o.linked && o.linkedAt) facts.push({ label: t("settings.telegram.linkedAt"), value: formatTableTime(o.linkedAt, locale) });
  if (o.linked) {
    facts.push({ label: t("settings.telegram.lastSent"), value: st.lastSentAt ? formatTableTime(st.lastSentAt, locale) : t("settings.telegram.never") });
  }
  if (o.queued > 0) facts.push({ label: t("settings.telegram.queued"), value: String(o.queued), tone: "warn" });
  if (st.lastError) facts.push({ label: t("settings.telegram.lastError"), value: localizeError(st.lastError), tone: "danger" });
  if (o.checkError) facts.push({ label: t("settings.telegram.checkError"), value: localizeError(o.checkError), tone: "danger" });

  return (
    <>
      <p className="ae-muted">{t("settings.telegram.officialLede", { bot })}</p>
      <p className="ae-subtle ae-tg__privacy">{t("settings.telegram.privacy")}</p>

      {signedOut ? (
        <p className="ae-tg__note" role="status">
          {t("settings.telegram.signedOut")}
        </p>
      ) : null}
      {!signedOut && unavailable ? (
        <p className="ae-tg__note" role="status">
          {t("settings.telegram.unavailable")}
        </p>
      ) : null}

      <FactList rows={facts} />

      {!o.linked && !signedOut ? (
        <div className="ae-tg__link">
          {o.linking ? (
            <>
              {/* The code on its own is the main path: the t.me link often drops it when the chat already exists. */}
              <ol className="ae-tg__steps">
                <li>{t("settings.telegram.codeStep1", { bot: `@${o.linking.bot}` })}</li>
                <li>{t("settings.telegram.codeStep2")}</li>
                <li>{t("settings.telegram.codeStep3")}</li>
              </ol>
              <div className="ae-tg__codebox">
                <code className="ae-tg__code ae-tg__code--big" aria-label={t("settings.telegram.codeLabel")}>
                  {o.linking.code}
                </code>
                <CopyCode code={o.linking.code} />
              </div>
              <p className="ae-subtle">{t("settings.telegram.officialExpires", { time: formatTableTime(o.linking.expiresAtMs, locale) })}</p>
              <div className="ae-tg__actions ae-tg__actions--tight">
                <Button variant="secondary" size="sm" disabled={busy} onClick={() => void run(telegramOfficialOpen)}>
                  {t("settings.telegram.openWithCode")}
                </Button>
                <Button variant="ghost" size="sm" disabled={busy} onClick={() => void run(telegramOfficialLink)}>
                  {t("settings.telegram.newCode")}
                </Button>
              </div>
            </>
          ) : (
            <Button size="sm" disabled={busy || o.available === false} onClick={() => void run(telegramOfficialLink)}>
              {t("settings.telegram.connect", { bot })}
            </Button>
          )}
        </div>
      ) : null}

      {o.linked ? (
        <ul className="ae-notify">
          {categoryRows}
          <SwitchRow
            id="tg-commands"
            name={t("settings.telegram.commands")}
            desc={t("settings.telegram.commandsOfficial")}
            checked={false}
            disabled
            onChange={() => undefined}
          />
        </ul>
      ) : null}
    </>
  );
}

/** Copies the link code; the label says whether the copy worked. Reset when the code changes. */
function CopyCode({ code }: { code: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState<{ code: string; ok: boolean } | null>(null);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied({ code, ok: true });
    } catch {
      setCopied({ code, ok: false });
    }
  };
  const state = copied?.code === code ? copied : null;
  return (
    <Button variant="primary" size="sm" onClick={() => void copy()}>
      {state ? (state.ok ? t("settings.telegram.codeCopied") : t("settings.telegram.copyFailed")) : t("settings.telegram.copyCode")}
    </Button>
  );
}

function OwnBody(props: {
  st: TelegramStatus;
  busy: boolean;
  locale: string;
  token: string;
  setToken: (v: string) => void;
  saveToken: (e: FormEvent) => void;
  run: Run;
  setPrefs: (patch: Partial<TelegramPrefs>) => void;
  categoryRows: ReactNode;
}) {
  const { st, busy, locale, token, setToken, saveToken, run, setPrefs, categoryRows } = props;
  const { t } = useTranslation();
  const phase = phaseOf(st);
  const bot = st.botUsername ? `@${st.botUsername}` : "—";

  return (
    <>
      <p className="ae-muted">{t("settings.telegram.lede")}</p>

      {phase === "none" ? (
        <>
          <ol className="ae-tg__steps">
            <li>{t("settings.telegram.step1")}</li>
            <li>{t("settings.telegram.step2")}</li>
            <li>{t("settings.telegram.step3")}</li>
          </ol>
          <form className="ae-tg__token" onSubmit={saveToken}>
            <TextField
              label={t("settings.telegram.token")}
              type="password"
              autoComplete="off"
              spellCheck={false}
              value={token}
              onChange={(e) => setToken(e.target.value)}
              disabled={busy}
              hint={t("settings.telegram.tokenHint")}
            />
            <Button type="submit" size="sm" disabled={busy || token.trim() === ""}>
              {t("settings.telegram.saveToken")}
            </Button>
          </form>
        </>
      ) : (
        <FactList
          rows={[
            { label: t("settings.telegram.bot"), value: bot },
            { label: t("settings.telegram.chat"), value: st.linked ? (st.chatLabel ?? t("settings.telegram.linkedNoName")) : t("settings.telegram.noChat") },
            ...(st.linked
              ? [
                  {
                    label: t("settings.telegram.lastSent"),
                    value: st.lastSentAt ? formatTableTime(st.lastSentAt, locale) : t("settings.telegram.never"),
                  },
                ]
              : []),
            ...(st.lastError ? [{ label: t("settings.telegram.lastError"), value: localizeError(st.lastError), tone: "danger" as const }] : []),
          ]}
        />
      )}

      {phase === "noChat" || phase === "waiting" ? (
        <div className="ae-tg__link">
          {st.linking ? (
            <>
              <p>{t("settings.telegram.sendThis", { bot })}</p>
              <code className="ae-tg__code">/start {st.linking.code}</code>
              <p className="ae-subtle">
                {t("settings.telegram.codeExpires", { time: formatTableTime(st.linking.expiresAtMs, locale) })}
              </p>
            </>
          ) : (
            <p className="ae-muted">{t("settings.telegram.step3")}</p>
          )}
          <Button variant="secondary" size="sm" disabled={busy} onClick={() => void run(telegramLinkCode)}>
            {st.linking ? t("settings.telegram.newCode") : t("settings.telegram.getCode")}
          </Button>
        </div>
      ) : null}

      {phase === "linked" ? (
        <ul className="ae-notify">
          {categoryRows}
          <SwitchRow
            id="tg-commands"
            name={t("settings.telegram.commands")}
            desc={t("settings.telegram.commandsDesc")}
            checked={st.prefs.commandsEnabled}
            disabled={busy}
            onChange={(on) => setPrefs({ commandsEnabled: on })}
          />
        </ul>
      ) : null}
    </>
  );
}

export function SwitchRow(props: { id: string; name: string; desc: string; checked: boolean; disabled: boolean; onChange: (on: boolean) => void }) {
  const { id, name, desc, checked, disabled, onChange } = props;
  return (
    <li className="ae-notify__row ae-tg__row" data-off={checked ? undefined : true}>
      <span className="ae-notify__text">
        <label htmlFor={id} className="ae-notify__name">
          {name}
        </label>
        <span className="ae-notify__desc" id={`${id}-desc`}>
          {desc}
        </span>
      </span>
      <input
        id={id}
        type="checkbox"
        role="switch"
        className="ae-switch"
        aria-describedby={`${id}-desc`}
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
      />
    </li>
  );
}
