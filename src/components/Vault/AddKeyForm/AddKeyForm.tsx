import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import { Panel } from "@/components/ui/Panel/Panel";
import { TextField } from "@/components/ui/TextField/TextField";
import { exchangeName as catalogName } from "@/lib/ipc/exchange/exchange";
import { useExchangeCatalog } from "@/lib/ipc/exchange/useExchangeCatalog";
import type { AddCredentialInput, CredentialPermission } from "@/lib/ipc/vault/vault";
import { MAX_LABEL_CHARS } from "../KeyList/KeyList";
import "./AddKeyForm.css";

interface AddKeyFormProps {
  onAdd: (input: AddCredentialInput) => Promise<void>;
  busy: boolean;
  error: string | null;
  /** Folds the form away; absent while the vault is empty (the form is the only content). */
  onCancel?: () => void;
}

/** Exchanges whose key permissions the vault can verify (vault/commands.rs). */
const VERIFIED_EXCHANGES = new Set(["binance"]);

const EMPTY = {
  exchangeId: "binance",
  label: "",
  apiKey: "",
  apiSecret: "",
  passphrase: "",
  // Withdraw-enabled keys are refused by the vault (owner decision
  // 2026-10-03), so there is nothing to declare: every key is trade-only.
  permission: "tradeOnly" as CredentialPermission,
};

/** Add one exchange credential to the unlocked vault. Secrets are write-only. */
export function AddKeyForm({ onAdd, busy, error, onCancel }: AddKeyFormProps) {
  const { t } = useTranslation();
  const [form, setForm] = useState(EMPTY);
  const catalog = useExchangeCatalog(t("common.exchangeListFailed"));

  const set = <K extends keyof typeof EMPTY>(key: K, value: (typeof EMPTY)[K]) =>
    setForm((f) => ({ ...f, [key]: value }));

  // Only the credential itself is required. The label is a convenience name;
  // if left blank it defaults to the exchange's display name so a missing label
  // never blocks adding a key (it only needs to disambiguate multiple keys).
  // The catalog must have loaded: the exchange picker IS the catalog.
  const canSubmit = form.apiKey.trim() && form.apiSecret.trim() && !busy && catalog.exchanges !== null;

  const exchangeName = catalogName(form.exchangeId, catalog.exchanges);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!canSubmit) return;
    try {
      await onAdd({
        exchangeId: form.exchangeId,
        label: form.label.trim() || exchangeName,
        apiKey: form.apiKey.trim(),
        apiSecret: form.apiSecret.trim(),
        passphrase: form.passphrase.trim() || undefined,
        permission: form.permission,
      });
      setForm(EMPTY);
    } catch {
      /* error surfaced via prop */
    }
  }

  return (
    <Panel
      title={t("vault.addKeyTitle")}
      aside={
        onCancel ? (
          <Button variant="ghost" size="sm" onClick={onCancel} disabled={busy}>
            {t("vault.closeAddKey")}
          </Button>
        ) : null
      }
    >
      <form className="ae-addkey" onSubmit={handleSubmit}>
        <div className="ae-addkey__row">
          <label className="ae-field">
            <span className="ae-field__label">{t("vault.exchange")}</span>
            <select
              className="ae-field__input"
              value={form.exchangeId}
              onChange={(e) => set("exchangeId", e.target.value)}
              disabled={busy || catalog.exchanges === null}
            >
              {(catalog.exchanges ?? []).filter((x) => VERIFIED_EXCHANGES.has(x.id)).map((x) => (
                <option key={x.id} value={x.id}>
                  {x.name}
                </option>
              ))}
            </select>
          </label>
          <TextField
            label={t("vault.label")}
            placeholder={t("vault.labelPlaceholder")}
            maxLength={MAX_LABEL_CHARS}
            value={form.label}
            onChange={(e) => set("label", e.target.value)}
            disabled={busy}
          />
        </div>

        <TextField
          label={`${t("vault.apiKey")} *`}
          autoComplete="off"
          spellCheck={false}
          value={form.apiKey}
          onChange={(e) => set("apiKey", e.target.value)}
          disabled={busy}
        />
        <TextField
          label={`${t("vault.apiSecret")} *`}
          type="password"
          autoComplete="off"
          value={form.apiSecret}
          onChange={(e) => set("apiSecret", e.target.value)}
          disabled={busy}
        />
        <TextField
          label={`${t("vault.passphrase")} ${t("vault.passphraseOptional")}`}
          type="password"
          autoComplete="off"
          value={form.passphrase}
          onChange={(e) => set("passphrase", e.target.value)}
          disabled={busy}
        />

        <p className="ae-subtle">{t("vault.tradeOnlyRule")}</p>

        {catalog.error ? (
          <>
            <p className="ae-addkey__error" role="alert">{catalog.error}</p>
            <Button variant="secondary" size="sm" onClick={catalog.retry}>
              {t("common.retry")}
            </Button>
          </>
        ) : null}
        {error ? <p className="ae-addkey__error">{error}</p> : null}

        <div className="ae-addkey__actions">
          <Button type="submit" size="sm" disabled={!canSubmit}>
            {busy ? t("vault.adding") : t("vault.addKey")}
          </Button>
        </div>
      </form>
    </Panel>
  );
}
