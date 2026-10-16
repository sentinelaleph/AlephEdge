import { useCallback, useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { StatusChip, type StatusKind } from "@/components/ui/StatusChip/StatusChip";
import { TextField } from "@/components/ui/TextField/TextField";
import { errorMessage } from "@/lib/ipc/bridge";
import {
  TELEGRAM_CATEGORIES,
  telegramLinkCode,
  telegramRemove,
  telegramSetPrefs,
  telegramSetToken,
  telegramStatus,
  telegramTest,
  type TelegramPrefs,
  type TelegramStatus,
} from "@/lib/ipc/telegram/telegram";
import { localizeError } from "@/lib/errorText";
import { formatTableTime } from "@/lib/format";

/** While a code waits for /start the panel re-reads quickly; otherwise slowly. */
const POLL_LINKING_MS = 2_000;
const POLL_IDLE_MS = 15_000;

type Phase = "none" | "noChat" | "waiting" | "linked";

function phaseOf(s: TelegramStatus): Phase {
  if (!s.tokenSet) return "none";
  if (s.linked) return "linked";
  return s.linking ? "waiting" : "noChat";
}

const CHIP: Record<Phase, StatusKind> = { none: "notConfigured", noChat: "needsSetup", waiting: "connecting", linked: "connected" };

/**
 * Settings, Notifications: Telegram. The token goes to the keychain through
 * Rust and is never read back; the chat is linked by a one-time code.
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

  const waiting = st !== null && phaseOf(st) === "waiting";
  const tokenSet = st?.tokenSet ?? false;
  useEffect(() => {
    if (!tokenSet) return;
    const id = window.setInterval(() => void refresh(), waiting ? POLL_LINKING_MS : POLL_IDLE_MS);
    return () => window.clearInterval(id);
  }, [refresh, tokenSet, waiting]);

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

  if (!st) {
    return (
      <Panel title={t("settings.telegram.title")}>
        {error ? <p className="ae-error" role="alert">{error}</p> : <p className="ae-muted">{t("settings.telegram.loading")}</p>}
      </Panel>
    );
  }

  const phase = phaseOf(st);
  const bot = st.botUsername ? `@${st.botUsername}` : "—";
  const locale = i18n.language;

  return (
    <Panel title={t("settings.telegram.title")} aside={<StatusChip status={CHIP[phase]} label={t(`settings.telegram.state.${phase}`)} />}>
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

      {error ? <p className="ae-error" role="alert">{error}</p> : null}
      {notice ? <p className="ae-subtle" role="status">{notice}</p> : null}

      {phase !== "none" ? (
        <div className="ae-tg__actions">
          {phase === "linked" ? (
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
