import { useEffect, useState, type FormEvent } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { TextField } from "@/components/ui/TextField/TextField";
import { localeForLanguage } from "@/i18n";
import { exchangeName } from "@/lib/ipc/exchange/exchange";
import { useExchangeCatalog } from "@/lib/ipc/exchange/useExchangeCatalog";
import { collapseTransition, SPRING } from "@/lib/motion";
import type { AddCredentialInput, CredentialMeta } from "@/lib/ipc/vault/vault";
import "./KeyList.css";

/** Longest label the vault accepts (`vault::MAX_LABEL_CHARS`). */
export const MAX_LABEL_CHARS = 40;

interface KeyListProps {
  credentials: CredentialMeta[];
  busy: boolean;
  /** Last vault error, shown under the row being edited. */
  error?: string | null;
  onRemove: (exchangeId: string, label: string) => void;
  onRenew?: (input: AddCredentialInput) => Promise<void>;
  onRename?: (exchangeId: string, label: string, newLabel: string) => Promise<void>;
}

type Edit = { key: string; mode: "renew" | "rename" } | null;

/** Stored credentials: metadata only; secrets never leave the Rust core. */
export function KeyList({ credentials, busy, error, onRemove, onRenew, onRename }: KeyListProps) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.language);
  const { exchanges } = useExchangeCatalog(t("common.exchangeListFailed"));
  const [edit, setEdit] = useState<Edit>(null);

  if (credentials.length === 0) {
    return <EmptyState title={t("vault.noKeys")} detail={t("accounts.noKeys")} />;
  }

  return (
    <ul className="ae-keylist">
      <AnimatePresence initial={false}>
        {credentials.map((c) => {
          const key = `${c.exchangeId}:${c.label}`;
          const editing = edit?.key === key ? edit.mode : null;
          const toggle = (mode: "renew" | "rename") => setEdit(editing === mode ? null : { key, mode });
          return (
            <motion.li
              key={key}
              className="ae-keylist__row"
              layout
              initial={{ opacity: 0, y: -6 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, scale: 0.97 }}
              transition={SPRING.snappy}
            >
              <div className="ae-keylist__main">
                <div className="ae-keylist__info">
                  <div className="ae-keylist__head">
                    <span className="ae-keylist__exchange">{exchangeName(c.exchangeId, exchanges)}</span>
                    <span className="ae-keylist__label">{c.label}</span>
                  </div>
                  <div className="ae-keylist__meta">
                    <span className="ae-keylist__perm" data-perm={c.permission}>
                      {t(`vault.perm.${c.permission}`)}
                    </span>
                    <span className="ae-keylist__date">
                      {c.updatedAt
                        ? t("vault.renewedOn", { date: new Date(c.updatedAt).toLocaleDateString(locale) })
                        : t("vault.addedOn", { date: new Date(c.addedAt).toLocaleDateString(locale) })}
                    </span>
                  </div>
                  {c.permission === "withdrawEnabled" ? (
                    <span className="ae-keylist__warn">{t("vault.withdrawStored")}</span>
                  ) : null}
                </div>
                <div className="ae-keylist__actions">
                  {onRenew ? (
                    <Button variant="secondary" size="xs" disabled={busy} aria-expanded={editing === "renew"} onClick={() => toggle("renew")}>
                      {t("vault.renew")}
                    </Button>
                  ) : null}
                  {onRename ? (
                    <Button variant="ghost" size="xs" disabled={busy} aria-expanded={editing === "rename"} onClick={() => toggle("rename")}>
                      {t("vault.rename")}
                    </Button>
                  ) : null}
                  <RemoveButton busy={busy} onConfirm={() => onRemove(c.exchangeId, c.label)} />
                </div>
              </div>
              <AnimatePresence initial={false}>
                {editing ? (
                  <motion.div
                    key={editing}
                    className="ae-keylist__edit"
                    initial={{ height: 0, opacity: 0 }}
                    animate={{ height: "auto", opacity: 1 }}
                    exit={{ height: 0, opacity: 0 }}
                    transition={collapseTransition()}
                  >
                    {editing === "renew" && onRenew ? (
                      <RenewForm cred={c} busy={busy} error={error} onCancel={() => setEdit(null)} onRenew={onRenew} onDone={() => setEdit(null)} />
                    ) : null}
                    {editing === "rename" && onRename ? (
                      <RenameForm cred={c} busy={busy} error={error} onCancel={() => setEdit(null)} onRename={onRename} onDone={() => setEdit(null)} />
                    ) : null}
                  </motion.div>
                ) : null}
              </AnimatePresence>
            </motion.li>
          );
        })}
      </AnimatePresence>
    </ul>
  );
}

interface EditProps {
  cred: CredentialMeta;
  busy: boolean;
  error?: string | null;
  onCancel: () => void;
  onDone: () => void;
}

/**
 * New key and secret under the same label. Nothing of the old key is shown:
 * the vault never hands a secret back. The old key stays until Rust accepts
 * and seals the new one.
 */
function RenewForm({ cred, busy, error, onCancel, onDone, onRenew }: EditProps & { onRenew: (input: AddCredentialInput) => Promise<void> }) {
  const { t } = useTranslation();
  const [apiKey, setApiKey] = useState("");
  const [apiSecret, setApiSecret] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [tried, setTried] = useState(false);
  const can = apiKey.trim() !== "" && apiSecret.trim() !== "" && (!cred.hasPassphrase || passphrase.trim() !== "") && !busy;

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!can) return;
    setTried(true);
    try {
      await onRenew({
        exchangeId: cred.exchangeId,
        label: cred.label,
        apiKey: apiKey.trim(),
        apiSecret: apiSecret.trim(),
        passphrase: passphrase.trim() || undefined,
        permission: "tradeOnly",
      });
      onDone();
    } catch {
      /* error shown below */
    }
  }

  return (
    <form className="ae-keylist__form" onSubmit={submit}>
      <p className="ae-subtle">{t("vault.renewHint")}</p>
      <TextField label={`${t("vault.apiKey")} *`} autoComplete="off" spellCheck={false} value={apiKey} onChange={(e) => setApiKey(e.target.value)} disabled={busy} />
      <TextField label={`${t("vault.apiSecret")} *`} type="password" autoComplete="off" value={apiSecret} onChange={(e) => setApiSecret(e.target.value)} disabled={busy} />
      {cred.hasPassphrase ? (
        <TextField label={`${t("vault.passphrase")} *`} type="password" autoComplete="off" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} disabled={busy} />
      ) : null}
      {tried && error ? <p className="ae-error" role="alert">{error}</p> : null}
      <div className="ae-keylist__formactions">
        <Button variant="ghost" size="sm" onClick={onCancel} disabled={busy}>
          {t("states.cancel")}
        </Button>
        <Button type="submit" size="sm" disabled={!can}>
          {busy ? t("vault.renewing") : t("vault.renewSubmit")}
        </Button>
      </div>
    </form>
  );
}

function RenameForm({ cred, busy, error, onCancel, onDone, onRename }: EditProps & { onRename: (exchangeId: string, label: string, newLabel: string) => Promise<void> }) {
  const { t } = useTranslation();
  const [label, setLabel] = useState(cred.label);
  const [tried, setTried] = useState(false);
  const next = label.trim();
  const can = next !== "" && next !== cred.label && !busy;

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!can) return;
    setTried(true);
    try {
      await onRename(cred.exchangeId, cred.label, next);
      onDone();
    } catch {
      /* error shown below */
    }
  }

  return (
    <form className="ae-keylist__form" onSubmit={submit}>
      <TextField label={t("vault.label")} maxLength={MAX_LABEL_CHARS} value={label} onChange={(e) => setLabel(e.target.value)} disabled={busy} autoFocus />
      {tried && error ? <p className="ae-error" role="alert">{error}</p> : null}
      <div className="ae-keylist__formactions">
        <Button variant="ghost" size="sm" onClick={onCancel} disabled={busy}>
          {t("states.cancel")}
        </Button>
        <Button type="submit" size="sm" disabled={!can}>
          {t("vault.renameSubmit")}
        </Button>
      </div>
    </form>
  );
}

/**
 * Two-step confirm, mirroring the account panel's close controls: a first
 * click arms it, a second click within a few seconds fires. Removing a key
 * is destructive (the vault holds no copy once it's gone), so it never fires
 * on a single stray click. Auto-disarms on a timeout so a forgotten armed
 * button can't be triggered much later.
 */
function RemoveButton({ busy, onConfirm }: { busy: boolean; onConfirm: () => void }) {
  const { t } = useTranslation();
  const [armed, setArmed] = useState(false);

  useEffect(() => {
    if (!armed) return;
    const id = window.setTimeout(() => setArmed(false), 4000);
    return () => window.clearTimeout(id);
  }, [armed]);

  return (
    <Button
      variant={armed ? "danger" : "secondary"}
      size="xs"
      disabled={busy}
      onClick={() => {
        if (armed) {
          setArmed(false);
          onConfirm();
        } else {
          setArmed(true);
        }
      }}
    >
      {armed ? t("vault.confirmRemove") : t("vault.remove")}
    </Button>
  );
}
