import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Button } from "@/components/ui/Button/Button";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { StatusChip, type StatusKind } from "@/components/ui/StatusChip/StatusChip";
import { localeForLanguage } from "@/i18n";
import type { MembershipView } from "@/lib/ipc/membership/membership";

/** Membership state -> the shared status tone (expiring warns, inactive is negative). */
const MEMBERSHIP_STATUS: Record<MembershipView["state"], StatusKind> = {
  active: "ok",
  expiring: "retrying",
  inactive: "error",
  unknown: "idle",
};

/**
 * #/account: the ribqa.com membership. Sign-out ends the session (App then
 * disconnects the Sentinel stream); running bots keep their state as before.
 * Free phase: no prices, no billing.
 */
export function AccountPage() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { membership, endpoints } = useDeskContext();
  const v = membership.view;
  const [refreshedAt, setRefreshedAt] = useState<number | null>(null);

  const refresh = async () => {
    await membership.refresh();
    setRefreshedAt(Date.now());
  };

  return (
    <PageShell title={t("nav.account")} single>
      <Panel
        title={v.displayName ?? v.email ?? t("nav.account")}
        aside={<StatusChip status={MEMBERSHIP_STATUS[v.state]} label={t(`membership.state.${v.state}`)} />}
      >
        <FactList
          rows={[
            { label: t("membership.email"), value: v.email ?? "—" },
            { label: t("accountPage.plan"), value: v.tier ?? "—" },
            {
              label: t("accountPage.validUntil"),
              value: v.currentPeriodEnd ? new Date(v.currentPeriodEnd).toLocaleDateString(locale) : "—",
            },
            ...(v.cancelAtPeriodEnd ? [{ label: t("accountPage.renewal"), value: t("accountPage.endsAtPeriodEnd"), tone: "warn" as const }] : []),
            {
              label: t("accountPage.lastRefresh"),
              value: refreshedAt ? new Date(refreshedAt).toLocaleTimeString(locale) : "—",
            },
            ...(endpoints ? [{ label: t("accountPage.service"), value: endpoints.apiBase }] : []),
          ]}
        />
        {membership.error ? <p className="ae-error">{membership.error}</p> : null}
        <div className="ae-toolbar">
          <Button variant="secondary" size="sm" disabled={membership.busy} onClick={() => void refresh()}>
            {t("accountPage.refresh")}
          </Button>
          <Button variant="ghost" size="sm" disabled={membership.busy} onClick={() => void membership.signOut()}>
            {t("membership.signOut")}
          </Button>
        </div>
      </Panel>
    </PageShell>
  );
}
