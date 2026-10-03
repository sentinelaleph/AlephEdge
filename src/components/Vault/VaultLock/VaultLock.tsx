import { useState, type FormEvent } from "react";
import { motion } from "framer-motion";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import { TextField } from "@/components/ui/TextField/TextField";
import { enter, SPRING } from "@/lib/motion";
import "./VaultLock.css";

interface VaultLockProps {
  /** "create" when no vault exists yet; "unlock" for an existing one. */
  mode: "create" | "unlock";
  busy: boolean;
  error: string | null;
  onSubmit: (password: string) => Promise<void>;
  /** Destroys the vault (unlock mode only) — the no-recovery escape hatch. */
  onReset?: () => Promise<void>;
}

/**
 * The password gate for the local vault. Creating a vault states the honest
 * constraint up front: there is no recovery — a lost password means resetting
 * the vault and re-entering keys.
 */
export function VaultLock({ mode, busy, error, onSubmit, onReset }: VaultLockProps) {
  const { t } = useTranslation();
  const isCreate = mode === "create";
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [confirmReset, setConfirmReset] = useState(false);

  const mismatch = isCreate && confirm.length > 0 && password !== confirm;
  const canSubmit = password.length >= 8 && (!isCreate || password === confirm) && !busy;

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!canSubmit) return;
    try {
      await onSubmit(password);
    } catch {
      /* error surfaced via prop */
    }
  }

  return (
    <motion.form className="ae-vaultlock" onSubmit={handleSubmit} {...enter}>
      <motion.span
        className="ae-vaultlock__glyph"
        aria-hidden="true"
        initial={{ scale: 0.9, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={SPRING.snappy}
      >
        <LockGlyph />
      </motion.span>

      <header className="ae-vaultlock__head">
        <h1 className="ae-vaultlock__title">
          {t(isCreate ? "vault.createTitle" : "vault.unlockTitle")}
        </h1>
        <p className="ae-vaultlock__subtitle">
          {t(isCreate ? "vault.createSubtitle" : "vault.unlockSubtitle")}
        </p>
      </header>

      <TextField
        label={t("vault.password")}
        type="password"
        autoComplete={isCreate ? "new-password" : "current-password"}
        placeholder="••••••••"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
        hint={isCreate ? t("vault.passwordHint") : undefined}
        disabled={busy}
        required
      />
      {isCreate ? (
        <TextField
          label={t("vault.confirmPassword")}
          type="password"
          autoComplete="new-password"
          placeholder="••••••••"
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
          disabled={busy}
          required
        />
      ) : null}

      {mismatch ? <p className="ae-vaultlock__error">{t("vault.passwordMismatch")}</p> : null}
      {error ? <p className="ae-vaultlock__error">{error}</p> : null}

      {isCreate ? (
        <p className="ae-vaultlock__warning">{t("vault.noRecoveryWarning")}</p>
      ) : null}

      <Button type="submit" disabled={!canSubmit}>
        {busy
          ? t(isCreate ? "vault.creating" : "vault.unlocking")
          : t(isCreate ? "vault.create" : "vault.unlock")}
      </Button>

      {!isCreate && onReset ? (
        <div className="ae-vaultlock__reset">
          {confirmReset ? (
            <>
              <p className="ae-vaultlock__warning">{t("vault.resetWarning")}</p>
              <div className="ae-vaultlock__reset-actions">
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  disabled={busy}
                  onClick={() => {
                    void onReset();
                    setConfirmReset(false);
                  }}
                >
                  {t("vault.resetConfirm")}
                </Button>
                <Button type="button" variant="ghost" size="sm" onClick={() => setConfirmReset(false)}>
                  {t("vault.resetCancel")}
                </Button>
              </div>
            </>
          ) : (
            <button
              type="button"
              className="ae-vaultlock__reset-link"
              onClick={() => setConfirmReset(true)}
            >
              {t("vault.forgotPassword")}
            </button>
          )}
        </div>
      ) : null}
    </motion.form>
  );
}

function LockGlyph() {
  return (
    <svg width="26" height="26" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <rect x="4.5" y="10.5" width="15" height="10" rx="2.4" stroke="currentColor" strokeWidth="1.7" />
      <path
        d="M8 10.5V8a4 4 0 0 1 8 0v2.5"
        stroke="currentColor"
        strokeWidth="1.7"
        strokeLinecap="round"
      />
      <circle cx="12" cy="15.2" r="1.5" fill="currentColor" />
    </svg>
  );
}
