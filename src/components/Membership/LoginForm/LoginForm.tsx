import { useState, type FormEvent } from "react";
import { motion } from "framer-motion";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import { TextField } from "@/components/ui/TextField/TextField";
import { enter } from "@/lib/motion";
import "./LoginForm.css";

interface LoginFormProps {
  onSignIn: (email: string, password: string) => Promise<void>;
  busy: boolean;
  error: string | null;
}

/**
 * Sign in to Sentinel membership. Credentials go straight to the Rust core,
 * which talks to ribqa.com — they are never stored by the UI.
 */
export function LoginForm({ onSignIn, busy, error }: LoginFormProps) {
  const { t } = useTranslation();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (busy || !email || !password) return;
    try {
      await onSignIn(email, password);
    } catch {
      /* error surfaced via the `error` prop */
    }
  }

  return (
    <motion.form className="ae-login" onSubmit={handleSubmit} {...enter}>
      <header className="ae-login__head">
        <h1 className="ae-login__title">{t("membership.signInTitle")}</h1>
        <p className="ae-login__subtitle">{t("membership.signInSubtitle")}</p>
      </header>

      <TextField
        label={t("membership.email")}
        type="email"
        name="email"
        autoComplete="username"
        placeholder={t("membership.emailPlaceholder")}
        value={email}
        onChange={(e) => setEmail(e.target.value)}
        disabled={busy}
        required
      />
      <TextField
        label={t("membership.password")}
        type="password"
        name="password"
        autoComplete="current-password"
        placeholder="••••••••"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
        disabled={busy}
        required
      />

      {error ? (
        <p className="ae-login__error" role="alert">
          {error}
        </p>
      ) : null}

      <Button type="submit" disabled={busy || !email || !password}>
        {busy ? t("membership.signingIn") : t("membership.signIn")}
      </Button>
    </motion.form>
  );
}
