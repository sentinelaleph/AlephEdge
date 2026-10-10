import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { Icon } from "@/app/AppShell/icons";
import { useDeskContext } from "@/app/DeskProvider";
import { Button } from "@/components/ui/Button/Button";
import { Chip } from "@/components/ui/Chip/Chip";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { StatusChip, type StatusKind } from "@/components/ui/StatusChip/StatusChip";
import { localeForLanguage } from "@/i18n";
import { openHelpLink, type HelpLink } from "@/lib/ipc/app/about";
import type { MembershipView } from "@/lib/ipc/membership/membership";
import { NO_VALUE } from "@/lib/format";
import { HelpPanel } from "@/pages/Settings/SettingsSummary";
import "./AccountPage.css";

/** Membership state -> the shared status tone (expiring warns, inactive is negative). */
const MEMBERSHIP_STATUS: Record<MembershipView["state"], StatusKind> = {
  active: "ok",
  expiring: "retrying",
  inactive: "error",
  unknown: "idle",
};

const KNOWN_TIERS = ["aleph", "free", "pro", "admin"];
const KNOWN_BILLING = ["active", "trialing", "past_due", "canceled", "none", "incomplete"];

/** A tier slug as a name ("aleph" -> "Aleph"); an unknown slug is shown as sent. */
export function tierName(t: TFunction, tier: string | undefined): string {
  if (!tier) return NO_VALUE;
  return KNOWN_TIERS.includes(tier) ? t(`accountPage.tier.${tier}`) : tier;
}

export function billingName(t: TFunction, status: string | undefined): string {
  if (!status) return NO_VALUE;
  return KNOWN_BILLING.includes(status) ? t(`accountPage.billing.${status}`) : status;
}

const COVERS = ["stream", "dcaGrid", "backtest", "market"] as const;
const WEB_LINKS: { link: HelpLink; key: string }[] = [
  { link: "webAccount", key: "accountPage.webAccount" },
  { link: "webBilling", key: "accountPage.webBilling" },
  { link: "webApiKeys", key: "accountPage.webApiKeys" },
];

/**
 * #/account: the ribqa.com membership this app runs on. Every value comes
 * from the membership the Rust core read from the server (when it read it is
 * shown); sign-out asks first and says what stops.
 */
export function AccountPage() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { membership, endpoints, feed, vault } = useDeskContext();
  const v = membership.view;
  const [confirmOut, setConfirmOut] = useState(false);
  const name = v.displayName ?? v.email ?? t("nav.account");
  const initial = (name.trim()[0] ?? "?").toUpperCase();

  const validUntil = v.currentPeriodEnd
    ? new Date(v.currentPeriodEnd).toLocaleDateString(locale)
    : v.admin
      ? t("accountPage.notApplicable")
      : v.active
        ? t("accountPage.noEndDate")
        : NO_VALUE;
  const checked = v.checkedAtMs ? new Date(v.checkedAtMs).toLocaleString(locale) : t("accountPage.notReadYet");

  const right = (
    <>
      <Panel title={t("accountPage.glanceTitle")}>
        <FactList
          rows={[
            { label: t("accountPage.membership"), value: t(`membership.state.${v.state}`), tone: v.active ? undefined : "warn" },
            { label: t("accountPage.plan"), value: tierName(t, v.tier) },
            { label: t("accountPage.canTrade"), value: t(v.active ? "accountPage.yes" : "accountPage.no"), tone: v.active ? undefined : "danger" },
            {
              label: t("accountPage.stream"),
              value: t(feed.health.connected ? "accountPage.streamUp" : "accountPage.streamDown"),
              tone: feed.health.connected ? undefined : "warn",
            },
            { label: t("accountPage.keys"), value: vault.credentials.length },
          ]}
        />
      </Panel>
      <HelpPanel />
    </>
  );

  return (
    <PageShell title={t("nav.account")} right={right}>
      <div className="ae-acctpage">
        <section className="ae-acct" aria-labelledby="ae-acct-name">
          <span className="ae-acct__avatar" aria-hidden="true">
            {initial}
          </span>
          <div className="ae-acct__body">
            <div className="ae-acct__titlerow">
              <h2 id="ae-acct-name" className="ae-acct__name">
                {name}
              </h2>
              <StatusChip status={MEMBERSHIP_STATUS[v.state]} label={t(`membership.state.${v.state}`)} />
              {v.tier ? <Chip tone="accent">{tierName(t, v.tier)}</Chip> : null}
              {v.admin ? <Chip tone="neutral">{t("accountPage.adminChip")}</Chip> : null}
            </div>
            {v.email && v.email !== name ? <p className="ae-acct__email tabular">{v.email}</p> : null}
            <p className="ae-subtle">{t("accountPage.signedInHere")}</p>
          </div>
          <div className="ae-acct__actions">
            <Button variant="secondary" size="sm" disabled={membership.busy} onClick={() => void membership.refresh()}>
              {t("accountPage.refresh")}
            </Button>
            <Button variant="ghost" size="sm" disabled={membership.busy} onClick={() => setConfirmOut(true)}>
              {t("membership.signOut")}
            </Button>
          </div>
        </section>
        {membership.error ? (
          <p className="ae-error" role="alert">
            {membership.error}
          </p>
        ) : null}

        <div className="ae-setgrid">
          <Panel title={t("accountPage.membershipTitle")}>
            <FactList
              rows={[
                { label: t("accountPage.plan"), value: tierName(t, v.tier) },
                { label: t("accountPage.billingStatus"), value: v.admin ? t("accountPage.notApplicable") : billingName(t, v.billingStatus) },
                { label: t("accountPage.validUntil"), value: validUntil },
                ...(v.cancelAtPeriodEnd ? [{ label: t("accountPage.renewal"), value: t("accountPage.endsAtPeriodEnd"), tone: "warn" as const }] : []),
                { label: t("accountPage.lastChecked"), value: checked },
              ]}
            />
            <p className="ae-subtle">
              {v.admin ? t("accountPage.adminNote") : t("accountPage.betaNote")} {t("accountPage.checkedNote")}
            </p>
          </Panel>

          <Panel title={t("accountPage.coversTitle")}>
            <ul className="ae-setlist" data-mark="yes">
              {COVERS.map((k) => (
                <li key={k}>{t(`accountPage.covers.${k}`)}</li>
              ))}
            </ul>
            <p className="ae-subtle">{t("accountPage.coversGate")}</p>
          </Panel>
        </div>

        <div className="ae-setgrid">
          <Panel title={t("accountPage.sessionTitle")}>
            <FactList
              rows={[
                { label: t("accountPage.service"), value: endpoints?.apiBase ?? NO_VALUE },
                { label: t("accountPage.keptIn"), value: t("accountPage.keptInValue") },
                { label: t("accountPage.afterRestart"), value: t("accountPage.afterRestartValue") },
                { label: t("accountPage.testnet"), value: t("accountPage.testnetValue") },
              ]}
            />
            <p className="ae-subtle">{t("accountPage.signOutNote")}</p>
          </Panel>

          <Panel title={t("accountPage.manageTitle")}>
            <p className="ae-muted">{t("accountPage.manageNote")}</p>
            <div className="ae-toolbar">
              {WEB_LINKS.map(({ link, key }) => (
                <Button key={link} variant="secondary" size="sm" onClick={() => void openHelpLink(link).catch(() => undefined)}>
                  {t(key)}
                  <Icon name="external" size={12} />
                </Button>
              ))}
            </div>
          </Panel>
        </div>
      </div>

      <ConfirmDialog
        open={confirmOut}
        title={t("accountPage.signOutConfirmTitle")}
        body={t("accountPage.signOutNote")}
        confirmLabel={t("membership.signOut")}
        busy={membership.busy}
        onCancel={() => setConfirmOut(false)}
        onConfirm={() => {
          setConfirmOut(false);
          void membership.signOut();
        }}
      />
    </PageShell>
  );
}
