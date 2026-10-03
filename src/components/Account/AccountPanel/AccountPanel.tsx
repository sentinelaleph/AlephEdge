import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import { Chip } from "@/components/ui/Chip/Chip";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import { Panel } from "@/components/ui/Panel/Panel";
import { Section } from "@/components/ui/Section/Section";
import { localeForLanguage } from "@/i18n";
import { formatPrice } from "@/lib/format";
import { CLOSE_CONFIRMATION, exchangeName } from "@/lib/ipc/exchange/exchange";
import { useExchangeCatalog } from "@/lib/ipc/exchange/useExchangeCatalog";
import { useAccount } from "../useAccount";
import "./AccountPanel.css";

interface AccountPanelProps {
  /** The exchange whose account to show, or null when no key is connected. */
  exchangeId: string | null;
}

/** USDT amounts (balances, PnL): always 2 decimals. Prices use formatPrice. */
const usd = (n: number) =>
  n.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });

/**
 * The connected exchange's USDT-M futures account: wallet balance, available
 * margin, unrealized PnL, and open positions. Mostly a read-only view, plus the
 * ONE manual write the app allows — a per-position close and an all-positions
 * kill switch. Those place REAL, reduce-only market orders (user-initiated,
 * confirmed by typing CLOSE, distinct from the simulation bots), so the
 * position you actually hold is always closeable from here.
 */
export function AccountPanel({ exchangeId }: AccountPanelProps) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.language);
  const { exchanges } = useExchangeCatalog(t("common.exchangeListFailed"));
  const { account, error, updatedAt, loading, closing, closeError, closePosition, closeAll } =
    useAccount(exchangeId);
  const hasPositions = (account?.positions.length ?? 0) > 0;
  // Figures from an earlier success stay on screen after a failed refresh,
  // but marked: they must never pass for current balances.
  const stale = account !== null && error !== null;
  const now = useClock();
  // Real closes act on what the snapshot shows: an old snapshot disables them.
  const tooOld = updatedAt === null || now - updatedAt > STALE_MS;

  return (
    <Panel
      title={t("account.title")}
      className="ae-acctpanel"
      aside={exchangeId ? <Chip tone="accent">{exchangeName(exchangeId, exchanges)}</Chip> : null}
    >
      {!exchangeId ? (
        <p className="ae-acctpanel__empty">{t("account.connectPrompt")}</p>
      ) : error && !account ? (
        <p className="ae-acctpanel__error" role="alert">{error}</p>
      ) : !account ? (
        <p className="ae-acctpanel__empty">{loading ? t("account.loading") : "—"}</p>
      ) : (
        <>
          {stale ? (
            <p className="ae-acctpanel__error" role="alert">
              {t("account.stale")} {error}
            </p>
          ) : null}
          {updatedAt !== null ? (
            <p className="ae-acctpanel__updated" data-stale={stale}>
              {t("account.lastUpdated", {
                time: new Date(updatedAt).toLocaleTimeString(locale, {
                  hour: "2-digit",
                  minute: "2-digit",
                  second: "2-digit",
                }),
              })}
            </p>
          ) : null}
          <div className="ae-acctpanel__stats" data-stale={stale}>
            <Stat label={t("account.walletBalance")} value={`${usd(account.totalWalletBalance)} USDT`} />
            <Stat label={t("account.available")} value={`${usd(account.availableBalance)} USDT`} />
            <Stat
              label={t("account.unrealizedPnl")}
              value={`${account.totalUnrealizedPnl >= 0 ? "+" : ""}${usd(account.totalUnrealizedPnl)} USDT`}
              tone={account.totalUnrealizedPnl >= 0 ? "up" : "down"}
            />
          </div>

          <Section
            level={3}
            title={t("account.positions")}
            count={hasPositions ? account.positions.length : undefined}
            aside={
              hasPositions ? (
                <CloseButton
                  idle={t("account.closeAll")}
                  title={t("account.closeAll")}
                  body={t("positions.closeAllBody", { count: account.positions.length })}
                  busy={closing === "*"}
                  busyLabel={t("account.closing")}
                  disabled={!!closing || tooOld}
                  onConfirm={() => void closeAll()}
                />
              ) : null
            }
          >
            {closeError ? <p className="ae-acctpanel__error" role="alert">{closeError}</p> : null}
            {!hasPositions ? (
              <p className="ae-acctpanel__empty">{t("account.noPositions")}</p>
            ) : (
              <>
                <ul className="ae-acctpanel__positions">
                  {account.positions.map((p) => {
                    const long = p.positionAmt > 0;
                    return (
                      <li key={`${p.symbol}:${long ? "long" : "short"}`} className="ae-acctpanel__pos">
                        <span className="ae-acctpanel__pos-dir" data-direction={long ? "long" : "short"}>
                          {long ? "▲" : "▼"}
                        </span>
                        <span className="ae-acctpanel__pos-sym">{p.symbol}</span>
                        <span className="ae-acctpanel__pos-size mono">{Math.abs(p.positionAmt)}</span>
                        <span className="ae-acctpanel__pos-entry mono">@ {formatPrice(p.entryPrice, locale)}</span>
                        <span className={`ae-acctpanel__pos-pnl mono ${p.unrealizedPnl >= 0 ? "ae-up" : "ae-down"}`}>
                          {p.unrealizedPnl >= 0 ? "+" : ""}
                          {usd(p.unrealizedPnl)}
                        </span>
                        <CloseButton
                          idle={t("account.close")}
                          title={`${t("account.close")} ${p.symbol}`}
                          body={t("positions.closeOneBody", { symbol: p.symbol, size: Math.abs(p.positionAmt) })}
                          busy={closing === p.symbol}
                          busyLabel={t("account.closing")}
                          disabled={!!closing || tooOld}
                          onConfirm={() => void closePosition(p.symbol)}
                        />
                      </li>
                    );
                  })}
                </ul>
                {tooOld ? <p className="ae-acctpanel__error">{t("positions.snapshotOld")}</p> : null}
                <p className="ae-acctpanel__note">{t("account.realOrderNote")}</p>
              </>
            )}
          </Section>
        </>
      )}
    </Panel>
  );
}

function Stat({ label, value, tone }: { label: string; value: string; tone?: "up" | "down" }) {
  return (
    <div className="ae-acctpanel__stat">
      <span className="ae-acctpanel__stat-label">{label}</span>
      <span className={`ae-acctpanel__stat-value${tone ? ` ae-${tone}` : ""}`}>{value}</span>
    </div>
  );
}

/** Real close orders wait for the typed word, never a stray click. Shared
 *  with the bot positions table, whose LIVE close is also a real order. */
export const CLOSE_WORD = CLOSE_CONFIRMATION;

/** Snapshots older than this disable the close buttons (prices may have moved). */
const STALE_MS = 60_000;

/**
 * Opens a typed confirmation ("CLOSE") before a REAL reduce-only close. The
 * dialog names the action; the order goes out only after the exact word.
 */
function CloseButton({
  idle,
  title,
  body,
  busy,
  busyLabel,
  disabled,
  onConfirm,
}: {
  idle: string;
  title: string;
  body: string;
  busy: boolean;
  busyLabel: string;
  disabled?: boolean;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="secondary" size="xs" disabled={disabled || busy} onClick={() => setOpen(true)}>
        {busy ? busyLabel : idle}
      </Button>
      <ConfirmDialog
        open={open}
        live
        title={title}
        body={body}
        word={CLOSE_WORD}
        confirmLabel={t("account.close")}
        danger
        onCancel={() => setOpen(false)}
        onConfirm={() => {
          setOpen(false);
          onConfirm();
        }}
      />
    </>
  );
}

/** A clock that ticks every 5 s, for the snapshot-age check. */
function useClock(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 5000);
    return () => window.clearInterval(id);
  }, []);
  return now;
}
