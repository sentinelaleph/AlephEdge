import { useTranslation } from "react-i18next";
import type { SkipNote } from "@/lib/ipc/bot/bot";
import { minimumOrderNotice } from "@/lib/skipNotes";
import { EXCHANGE_MIN_ORDER_USDT } from "@/lib/botLimits";

/**
 * One plain line when signals were refused because the sized order was under
 * the exchange minimum: the bot is running but cannot open anything.
 */
export function MinimumOrderBanner({ notes }: { notes: SkipNote[] }) {
  const { t } = useTranslation();
  const n = minimumOrderNotice(notes);
  if (!n) return null;
  return (
    <div className="ae-banner" data-tone="warn" role="status" data-testid="minimum-order-banner">
      <span>
        {t("bots.minimumOrder.line", { count: n.count, order: n.orderUsdt, symbol: n.symbol, min: EXCHANGE_MIN_ORDER_USDT })}{" "}
        {t("bots.minimumOrder.fix")}
      </span>
    </div>
  );
}
