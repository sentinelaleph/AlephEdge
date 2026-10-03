import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { AccountChip } from "@/components/Membership/AccountChip/AccountChip";
import type { MembershipController } from "@/components/Membership/useMembership";
import { Cockpit } from "@/components/Shell/Cockpit/Cockpit";
import { LanguageSelect } from "@/components/ui/LanguageSelect/LanguageSelect";
import { ThemeToggle } from "@/components/ui/ThemeToggle/ThemeToggle";
import { getEndpoints } from "@/lib/ipc/app/endpoints";
import { EdgeMark } from "./EdgeMark";
import "./TopBar.css";

interface TopBarProps {
  membership: MembershipController;
  /** Exchange the cockpit exchange-check probes, or null when none is vaulted. */
  cockpitExchange: string | null;
}

/**
 * The gate bar, shown above the sign-in, membership and vault screens: brand
 * left, cockpit checks centre, account + language + theme right. Past the
 * gates the desk uses its own sidebar, header and status bar.
 */
export function TopBar({ membership, cockpitExchange }: TopBarProps) {
  const { t } = useTranslation();
  const nonProdBase = useNonProductionBinanceBase();

  return (
    <nav className="ae-topbar">
      <div className="ae-brand">
        <span className="ae-brand__mark" aria-hidden="true">
          <EdgeMark />
        </span>
        <span className="ae-brand__name">{t("app.name")}</span>
        <span className="ae-brand__beta">{t("app.betaBadge")}</span>
        {/* Persistent, never a one-off toast: a testnet/custom order endpoint
            is exactly the kind of fact a trader must never have to remember
            after seeing it once. */}
        {nonProdBase ? (
          <span
            className="ae-brand__testnet"
            title={t("app.testnetTooltip", { host: nonProdBase })}
          >
            {t("app.testnetBadge")}
          </span>
        ) : null}
      </div>
      <Cockpit exchangeId={cockpitExchange} />
      <div className="ae-topbar__actions">
        <AccountChip membership={membership} />
        <LanguageSelect />
        <ThemeToggle />
      </div>
    </nav>
  );
}

/**
 * Resolves to the non-production Binance futures base (e.g. the testnet
 * host) once the real endpoints are known, or `null` on production / while
 * still loading. Fetched once — the endpoint is fixed for the process
 * lifetime (set at build/launch time, never changes at runtime).
 */
function useNonProductionBinanceBase(): string | null {
  const [base, setBase] = useState<string | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    getEndpoints()
      .then((e) => {
        if (alive.current && !e.binanceIsProduction) {
          setBase(e.binanceFuturesBase);
        }
      })
      .catch(() => undefined);
    return () => {
      alive.current = false;
    };
  }, []);

  return base;
}
