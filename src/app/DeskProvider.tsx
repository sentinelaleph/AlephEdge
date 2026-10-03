import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useBotDesk, type BotDeskController } from "@/components/BotDesk/useBotDesk";
import { useHealth } from "@/lib/ipc/health/useHealth";
import type { MembershipController } from "@/components/Membership/useMembership";
import { usePnl, type PnlController } from "@/components/PnlPanel/usePnl";
import { useRisk, type RiskController } from "@/components/RiskSelector/useRisk";
import { useSignalFeed, type SignalFeedState } from "@/components/SignalDesk/useSignalFeed";
import type { VaultController } from "@/components/Vault/useVault";
import { appVersion, getEndpoints, type Endpoints } from "@/lib/ipc/app/endpoints";
import type { ExchangeInfo } from "@/lib/ipc/exchange/exchange";
import { useExchangeCatalog, type ExchangeCatalog } from "@/lib/ipc/exchange/useExchangeCatalog";
import type { HealthSnapshot } from "@/lib/ipc/health/health";
import { useStrategyDesk, type StrategyDeskController } from "@/lib/ipc/strategy/useStrategyDesk";

/**
 * Everything the unlocked desk shares across pages. Owned once, above the
 * router, so switching pages never restarts polling or the signal stream.
 * Page-scoped data (the exchange account, phone pairing) stays in its page.
 */
export interface DeskContextValue {
  membership: MembershipController;
  vault: VaultController;
  risk: RiskController;
  desk: BotDeskController;
  pnl: PnlController;
  /** DCA and Grid bots (paper only): list, strategy risk, notes. */
  strategy: StrategyDeskController;
  catalog: ExchangeCatalog;
  /** The single get_health_snapshot poller (status bar, checklist, dashboard). */
  health: HealthSnapshot | null;
  feed: SignalFeedState;
  endpoints: Endpoints | null;
  version: string | null;
  /** Exchanges the user holds a vaulted key for; null while the catalog loads. */
  keyedExchanges: ExchangeInfo[] | null;
  /** The exchange the account views read (Binance only today), or null. */
  accountExchange: string | null;
}

const DeskContext = createContext<DeskContextValue | null>(null);

export const DeskContextProvider = DeskContext.Provider;

export function useDeskContext(): DeskContextValue {
  const ctx = useContext(DeskContext);
  if (!ctx) throw new Error("useDeskContext must be used inside <DeskProvider>");
  return ctx;
}

interface DeskProviderProps {
  membership: MembershipController;
  vault: VaultController;
  children: ReactNode;
}

/** Mounted only once the membership and vault gates have passed. */
export function DeskProvider({ membership, vault, children }: DeskProviderProps) {
  const { t } = useTranslation();
  const risk = useRisk();
  const desk = useBotDesk();
  // Stats wait for the desk to say whether this is a live build: until then
  // an unscoped call could sum real and simulated money.
  const pnl = usePnl(desk.loaded ? desk.status.liveTradingEnabled : null);
  const strategy = useStrategyDesk();
  const catalog = useExchangeCatalog(t("common.exchangeListFailed"));
  const health = useHealth();
  // Connects the Sentinel stream once for the whole desk (signal_connect).
  const feed = useSignalFeed();
  const [endpoints, setEndpoints] = useState<Endpoints | null>(null);
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    getEndpoints()
      .then((e) => alive && setEndpoints(e))
      .catch(() => undefined);
    appVersion()
      .then((v) => alive && setVersion(v))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);

  const value = useMemo<DeskContextValue>(() => {
    const keyed = new Set(vault.credentials.map((c) => c.exchangeId));
    return {
      membership,
      vault,
      risk,
      desk,
      pnl,
      strategy,
      catalog,
      health,
      feed,
      endpoints,
      version,
      keyedExchanges: catalog.exchanges?.filter((e) => keyed.has(e.id)) ?? null,
      accountExchange: keyed.has("binance") ? "binance" : null,
    };
  }, [membership, vault, risk, desk, pnl, strategy, catalog, health, feed, endpoints, version]);

  return <DeskContext.Provider value={value}>{children}</DeskContext.Provider>;
}
