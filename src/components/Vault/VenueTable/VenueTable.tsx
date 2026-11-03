import { useTranslation } from "react-i18next";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { Panel } from "@/components/ui/Panel/Panel";
import type { ExchangeInfo, VenueLive } from "@/lib/ipc/exchange/exchange";
import { useExchangeCatalog } from "@/lib/ipc/exchange/useExchangeCatalog";
import "./VenueTable.css";

type Row = ExchangeInfo & { venue: NonNullable<ExchangeInfo["venue"]> };

export const LIVE_LABEL: Record<VenueLive, string> = {
  verified: "venues.live.verified",
  notVerified: "venues.live.notVerified",
  paperOnly: "venues.live.paperOnly",
};

/**
 * The 15 venues and how far each is: prices, account read (a key the vault
 * verifies as trade-only), order path, test network, live. The 8 order
 * venues and the 7 price-data / paper venues come from Rust's capability
 * table (`exchange/venue/caps.rs`) through the catalog, so nothing here can
 * claim more than the code does.
 */
export function VenueTable() {
  const { t } = useTranslation();
  const catalog = useExchangeCatalog(t("common.exchangeListFailed"));
  const rows = (catalog.exchanges ?? []).filter((x): x is Row => Boolean(x.venue));
  const yes = t("venues.yes");
  const no = t("venues.no");
  const columns: DataColumn<Row>[] = [
    { id: "venue", header: t("venues.col.venue"), cell: (r) => r.name, keep: true },
    { id: "margin", header: t("venues.col.margin"), cell: (r) => r.venue.margin, priority: 2 },
    { id: "prices", header: t("venues.col.prices"), cell: (r) => (r.supportsFutures ? yes : no), priority: 3 },
    {
      id: "account",
      header: t("venues.col.account"),
      headerTip: t("venues.tip.account"),
      cell: (r) => (r.venue.orders ? yes : no),
      tone: (r) => (r.venue.orders ? undefined : "muted"),
    },
    {
      id: "orders",
      header: t("venues.col.orders"),
      headerTip: t("venues.tip.orders"),
      cell: (r) => (r.venue.orders ? t("venues.built") : no),
      tone: (r) => (r.venue.orders ? undefined : "muted"),
      priority: 2,
    },
    { id: "testnet", header: t("venues.col.testnet"), cell: (r) => (r.venue.orders?.testnet ? yes : no), priority: 2 },
    {
      id: "live",
      header: t("venues.col.live"),
      cell: (r) => t(LIVE_LABEL[r.venue.live]),
      tone: (r) => (r.venue.live === "verified" ? "up" : r.venue.live === "paperOnly" ? "muted" : undefined),
      keep: true,
      wrap: true,
    },
  ];
  return (
    <Panel title={t("venues.title")}>
      <DataTable label={t("venues.title")} columns={columns} rows={rows} rowKey={(r) => r.id} loading={catalog.exchanges === null && !catalog.error} bare compact />
      <p className="ae-subtle ae-venues__note">{t("venues.note")}</p>
    </Panel>
  );
}
