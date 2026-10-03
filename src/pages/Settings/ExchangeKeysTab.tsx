import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Button } from "@/components/ui/Button/Button";
import { Chip } from "@/components/ui/Chip/Chip";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { SegmentedControl } from "@/components/ui/SegmentedControl/SegmentedControl";
import { StatusChip } from "@/components/ui/StatusChip/StatusChip";
import { TextField } from "@/components/ui/TextField/TextField";
import { VaultManager } from "@/components/Vault/VaultManager/VaultManager";
import { localeForLanguage } from "@/i18n";
import { errorMessage } from "@/lib/ipc/bridge";
import { cockpitExchange, cockpitSentinel, type PingResult } from "@/lib/ipc/cockpit/cockpit";
import { exchangeName } from "@/lib/ipc/exchange/exchange";
import { IDLE_MINUTE_CHOICES } from "@/lib/ipc/vault/vault";
import { localizeError } from "@/lib/errorText";
import "./ExchangeKeysTab.css";

const RESET_WORD = "RESET";
/** Same rule as `vault::commands::MIN_VAULT_PASSWORD_CHARS`. */
const MIN_PASSWORD_CHARS = 8;

interface Probe {
  state: "idle" | "checking" | "done";
  result?: PingResult;
  at?: number;
}

/**
 * Settings, exchange keys: the local vault. Keys (add, renew, rename,
 * remove), the vault itself (auto-lock, password, lock), connectivity and
 * the reset escape hatch. The app is only reachable with the vault open, so
 * everything here acts on an unlocked vault.
 */
export function ExchangeKeysTab() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { vault, catalog, endpoints } = useDeskContext();
  const [probes, setProbes] = useState<Record<string, Probe>>({});
  const [resetOpen, setResetOpen] = useState(false);
  const keyed = [...new Set(vault.credentials.map((c) => c.exchangeId))];

  const run = async (id: string, probe: () => Promise<PingResult>) => {
    setProbes((p) => ({ ...p, [id]: { state: "checking" } }));
    let result: PingResult;
    try {
      result = await probe();
    } catch (e) {
      result = { ok: false, detail: errorMessage(e, t("cockpit.error")) };
    }
    setProbes((p) => ({ ...p, [id]: { state: "done", result, at: Date.now() } }));
  };

  const probeRow = (id: string, label: string, probe: () => Promise<PingResult>) => {
    const p = probes[id];
    return (
      <li key={id} className="ae-proberow">
        <span className="ae-proberow__name">{label}</span>
        {p?.state === "done" && p.result ? (
          <StatusChip
            status={p.result.ok ? "ok" : "error"}
            label={`${p.result.ok ? t("accounts.probeOk") : t("accounts.probeFail")}${
              p.result.latencyMs != null ? ` · ${p.result.latencyMs} ms` : ""
            }`}
          />
        ) : null}
        <Button variant="secondary" size="xs" disabled={p?.state === "checking"} onClick={() => void run(id, probe)}>
          {p?.state === "checking" ? t("cockpit.checking") : t("accounts.testConnection")}
        </Button>
        {p?.state === "done" && p.result && !p.result.ok ? (
          <span className="ae-error ae-proberow__detail">
            {localizeError(p.result.detail)}
            {p.at ? ` · ${new Date(p.at).toLocaleTimeString(locale)}` : ""}
          </span>
        ) : null}
      </li>
    );
  };

  const idle = String(vault.status.idleTimeoutMinutes);
  const idleOptions = IDLE_MINUTE_CHOICES.map((m) => ({ value: String(m), label: t("vault.minutes", { minutes: m }) }));

  return (
    <div className="ae-keystab">
      <VaultManager vault={vault} />

      <Panel
        title={t("accounts.vault")}
        aside={
          <Button variant="secondary" size="sm" onClick={() => void vault.lock()} disabled={vault.busy}>
            {vault.busy ? t("vault.locking") : t("vault.lock")}
          </Button>
        }
      >
        <FactList
          rows={[
            { label: t("accounts.keys"), value: vault.credentials.length },
            { label: t("accounts.storage"), value: t("accounts.storageFact") },
          ]}
        />
        <div className="ae-keystab__idle">
          <SegmentedControl
            label={t("accounts.autoLock")}
            size="sm"
            wrap
            value={idleOptions.some((o) => o.value === idle) ? idle : null}
            onChange={(v) => void vault.setIdleMinutes(Number(v)).catch(() => undefined)}
            disabled={vault.busy}
            options={idleOptions}
          />
          <p className="ae-subtle">{t("vault.autoLockHint", { minutes: vault.status.idleTimeoutMinutes })}</p>
        </div>
      </Panel>

      <ChangePasswordPanel />

      <Panel title={t("accounts.connectivity")}>
        <ul className="ae-list ae-probelist">
          {keyed.map((id) => probeRow(id, exchangeName(id, catalog.exchanges), () => cockpitExchange(id)))}
          {probeRow("sentinel", "Sentinel", cockpitSentinel)}
        </ul>
        {keyed.length === 0 ? <p className="ae-subtle">{t("accounts.noKeyProbe")}</p> : null}
      </Panel>

      {endpoints ? (
        <Panel
          title={t("accounts.endpoints")}
          aside={
            <Chip tone={endpoints.binanceIsProduction ? "neutral" : "warn"}>
              {endpoints.binanceIsProduction ? t("accounts.production") : t("app.testnetBadge")}
            </Chip>
          }
        >
          <FactList
            rows={[
              { label: t("accounts.binanceOrders"), value: endpoints.binanceFuturesBase },
              { label: t("accounts.apiBase"), value: endpoints.apiBase },
            ]}
          />
        </Panel>
      ) : null}

      <Panel title={t("accounts.dangerZone")} tone="danger">
        <p className="ae-muted">{t("vault.resetWarning")}</p>
        <div>
          <Button variant="secondary" size="sm" disabled={vault.busy} onClick={() => setResetOpen(true)}>
            {t("accounts.resetVault")}
          </Button>
        </div>
      </Panel>
      <ConfirmDialog
        open={resetOpen}
        title={t("accounts.resetVault")}
        body={t("vault.resetWarning")}
        word={RESET_WORD}
        confirmLabel={t("accounts.resetVault")}
        danger
        busy={vault.busy}
        onCancel={() => setResetOpen(false)}
        onConfirm={() => {
          setResetOpen(false);
          void vault.reset().catch(() => undefined);
        }}
      />
    </div>
  );
}

/** Current password, new password twice. The vault re-seals under the new one. */
function ChangePasswordPanel() {
  const { t } = useTranslation();
  const { vault } = useDeskContext();
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [again, setAgain] = useState("");
  const [tried, setTried] = useState(false);
  const [done, setDone] = useState(false);

  const tooShort = next.length > 0 && next.length < MIN_PASSWORD_CHARS;
  const mismatch = again.length > 0 && next !== again;
  const can = current.length > 0 && next.length >= MIN_PASSWORD_CHARS && next === again && !vault.busy;

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!can) return;
    setTried(true);
    setDone(false);
    try {
      await vault.changePassword(current, next);
      setCurrent("");
      setNext("");
      setAgain("");
      setDone(true);
    } catch {
      /* error shown below */
    }
  }

  return (
    <Panel title={t("vault.changePassword")}>
      <form className="ae-keystab__pw" onSubmit={submit}>
        <TextField label={t("vault.currentPassword")} type="password" autoComplete="current-password" value={current} onChange={(e) => setCurrent(e.target.value)} disabled={vault.busy} />
        <TextField
          label={t("vault.newPassword")}
          type="password"
          autoComplete="new-password"
          value={next}
          onChange={(e) => setNext(e.target.value)}
          disabled={vault.busy}
          hint={tooShort ? <span className="ae-warntext">{t("vault.passwordHint")}</span> : t("vault.passwordHint")}
        />
        <TextField
          label={t("vault.confirmPassword")}
          type="password"
          autoComplete="new-password"
          value={again}
          onChange={(e) => setAgain(e.target.value)}
          disabled={vault.busy}
          hint={mismatch ? <span className="ae-warntext">{t("vault.passwordMismatch")}</span> : undefined}
        />
        <p className="ae-subtle">{t("vault.noRecoveryWarning")}</p>
        {tried && vault.error ? <p className="ae-error" role="alert">{vault.error}</p> : null}
        {done ? <p className="ae-subtle" role="status">{t("vault.passwordChanged")}</p> : null}
        <div className="ae-keystab__pwactions">
          <Button type="submit" size="sm" disabled={!can}>
            {vault.busy ? t("vault.changingPassword") : t("vault.changePassword")}
          </Button>
        </div>
      </form>
    </Panel>
  );
}
