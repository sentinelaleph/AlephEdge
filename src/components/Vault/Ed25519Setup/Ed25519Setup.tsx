import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import { SegmentedControl } from "@/components/ui/SegmentedControl/SegmentedControl";
import { errorMessage } from "@/lib/ipc/bridge";
import { vaultEd25519Generate, vaultEd25519Pending, type KeyType } from "@/lib/ipc/vault/vault";
import "./Ed25519Setup.css";

/** Exchanges that accept a self-generated Ed25519 key (Binance only). */
export const ED25519_EXCHANGES = new Set(["binance"]);

/** The key-type switch shown for Binance in the add and renew forms. */
export function KeyTypeSwitch({ value, onChange, disabled }: { value: KeyType; onChange: (k: KeyType) => void; disabled?: boolean }) {
  const { t } = useTranslation();
  return (
    <SegmentedControl
      label={t("vault.keyType")}
      size="sm"
      wrap
      value={value}
      onChange={onChange}
      disabled={disabled}
      options={[
        { value: "hmac", label: t("vault.keyTypeHmac") },
        { value: "ed25519", label: t("vault.keyTypeEd25519") },
      ]}
    />
  );
}

interface Ed25519SetupProps {
  busy: boolean;
  /** Fires with whether a key pair is waiting for its API key. */
  onReady: (ready: boolean) => void;
}

const STEPS = ["step1", "step2", "step3", "step4"] as const;

/**
 * Binance "Self-generated" key: Rust makes an Ed25519 key pair and hands back
 * only the PUBLIC key, which the user registers on Binance. The private key
 * never reaches this component; it is sealed into the vault when the form
 * that holds this is submitted with `keyType: "ed25519"`.
 */
export function Ed25519Setup({ busy, onReady }: Ed25519SetupProps) {
  const { t } = useTranslation();
  const [pem, setPem] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<"ok" | "fail" | null>(null);

  // A pair made earlier in this session (form closed and reopened) is shown
  // again rather than silently replaced: its public key may already be on
  // Binance.
  useEffect(() => {
    let live = true;
    vaultEd25519Pending()
      .then((p) => {
        if (!live) return;
        setPem(p);
        onReady(Boolean(p));
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [onReady]);

  async function create() {
    setCreating(true);
    setError(null);
    setCopied(null);
    try {
      const next = await vaultEd25519Generate();
      setPem(next);
      onReady(true);
    } catch (e) {
      setError(errorMessage(e, t("vault.error")));
    } finally {
      setCreating(false);
    }
  }

  async function copy() {
    if (!pem) return;
    try {
      await navigator.clipboard.writeText(pem);
      setCopied("ok");
    } catch {
      setCopied("fail");
    }
  }

  return (
    <div className="ae-edsetup">
      <p className="ae-subtle">{t("vault.ed.intro")}</p>
      {pem ? (
        <>
          <label className="ae-field">
            <span className="ae-field__label">{t("vault.ed.publicKey")}</span>
            <textarea className="ae-field__input ae-edsetup__pem" readOnly rows={4} value={pem} spellCheck={false} onFocus={(e) => e.currentTarget.select()} />
          </label>
          <div className="ae-edsetup__actions">
            <Button variant="secondary" size="sm" onClick={() => void copy()} disabled={busy}>
              {copied === "ok" ? t("vault.ed.copied") : t("vault.ed.copy")}
            </Button>
            <Button variant="ghost" size="sm" onClick={() => void create()} disabled={busy || creating}>
              {creating ? t("vault.ed.creating") : t("vault.ed.recreate")}
            </Button>
          </div>
          {copied === "fail" ? <p className="ae-error" role="alert">{t("vault.ed.copyFailed")}</p> : null}
          <ol className="ae-edsetup__steps">
            {STEPS.map((s) => (
              <li key={s}>{t(`vault.ed.${s}`)}</li>
            ))}
          </ol>
          <p className="ae-subtle">{t("vault.ed.private")}</p>
        </>
      ) : (
        <div className="ae-edsetup__actions">
          <Button size="sm" onClick={() => void create()} disabled={busy || creating}>
            {creating ? t("vault.ed.creating") : t("vault.ed.create")}
          </Button>
        </div>
      )}
      {error ? <p className="ae-error" role="alert">{error}</p> : null}
    </div>
  );
}
