import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "@/app/AppShell/icons";
import { useDeskContext } from "@/app/DeskProvider";
import { Link } from "@/app/router/router";
import { useUpdateState } from "@/app/update";
import { EdgeMark } from "@/components/Shell/EdgeMark";
import { Button } from "@/components/ui/Button/Button";
import { Chip } from "@/components/ui/Chip/Chip";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { isTestnetBuild, openDataDir, openHelpLink, type AppInfo, type HelpLink } from "@/lib/ipc/app/about";
import { NO_VALUE } from "@/lib/format";
import { buildText, updateStatusText } from "./SettingsSummary";
import { UpdatePanel } from "./UpdatePanel";

const OS_NAME: Record<string, string> = { windows: "Windows", macos: "macOS", linux: "Linux" };
const ARCH_NAME: Record<string, string> = { x86_64: "x64", aarch64: "ARM64", x86: "x86" };

export function systemText(info: AppInfo): string {
  const os = OS_NAME[info.os] ?? info.os;
  const arch = ARCH_NAME[info.arch] ?? info.arch;
  return arch ? `${os} ${arch}` : os;
}

function ExternalButton({ link, children }: { link: HelpLink; children: string }) {
  return (
    <Button variant="secondary" size="sm" onClick={() => void openHelpLink(link).catch(() => undefined)}>
      {children}
      <Icon name="external" size={12} />
    </Button>
  );
}

/**
 * Settings, About: what this is, its version and updates, this installation,
 * where data goes, the licence and how to reach us. Every value is read from
 * the running app (app_info, endpoints, the update state); nothing is typed in.
 */
export function AboutTab({ info }: { info: AppInfo | null }) {
  const { t, i18n } = useTranslation();
  const { endpoints } = useDeskContext();
  const update = useUpdateState();
  const [copy, setCopy] = useState<"idle" | "done" | "failed">("idle");
  const browser = info?.os === "browser";

  const system = info ? (browser ? t("settings.about.browserPreview") : systemText(info)) : NO_VALUE;
  const orders = endpoints
    ? `${endpoints.binanceFuturesBase} (${t(endpoints.binanceIsProduction ? "accounts.production" : "app.testnetBadge")})`
    : NO_VALUE;

  // Support text: no paths (they carry the Windows user name) and no desk id.
  const diagnostics = [
    `Aleph Edge ${info?.version ?? NO_VALUE}`,
    `${t("settings.summary.build")}: ${buildText(t, info)}`,
    `${t("settings.about.system")}: ${system}`,
    `${t("settings.about.webview")}: ${info?.webview ?? NO_VALUE}`,
    `${t("settings.about.tauri")}: ${info?.tauri || NO_VALUE}`,
    `${t("settings.keys.sentinelApi")}: ${endpoints?.apiBase ?? NO_VALUE}`,
    `${t("settings.relay")}: ${endpoints?.relayUrl ?? NO_VALUE}`,
    `${t("accounts.binanceOrders")}: ${orders}`,
    `${t("settings.summary.updates")}: ${updateStatusText(t, update)}`,
    `${t("settings.language")}: ${i18n.resolvedLanguage ?? NO_VALUE}`,
  ].join("\n");

  const copyDiagnostics = async () => {
    try {
      await navigator.clipboard.writeText(diagnostics);
      setCopy("done");
    } catch {
      setCopy("failed");
    }
    window.setTimeout(() => setCopy("idle"), 2500);
  };

  return (
    <>
      <section className="ae-about" aria-labelledby="ae-about-name">
        <span className="ae-about__mark" aria-hidden="true">
          <EdgeMark />
        </span>
        <div className="ae-about__body">
          <div className="ae-about__titlerow">
            <h2 id="ae-about-name" className="ae-about__name">
              {t("app.name")}
            </h2>
            {info && !browser ? <span className="ae-about__version tabular">v{info.version}</span> : null}
            <Chip tone={info?.liveBuild ? "live" : "paper"}>
              {t(info?.liveBuild ? "settings.about.liveBuild" : "settings.about.paperBuild")}
            </Chip>
            {info && isTestnetBuild(info) ? <Chip tone="warn">{t("settings.about.testnetBuild")}</Chip> : null}
            <Chip tone="neutral">{t("app.betaBadge")}</Chip>
          </div>
          <p className="ae-about__tagline">{t("settings.about.tagline")}</p>
          <p className="ae-subtle">
            {t(info?.liveBuild ? "settings.about.buildNote.live" : "settings.about.buildNote.paper")} {t("settings.about.notAdvice")}
          </p>
        </div>
      </section>

      <div className="ae-setgrid">
        <UpdatePanel />

        <Panel
          title={t("settings.about.installTitle")}
          className="ae-installfacts"
          aside={
            <Button variant="secondary" size="sm" onClick={() => void copyDiagnostics()}>
              <Icon name="copy" size={12} />
              {copy === "done" ? t("settings.about.copied") : copy === "failed" ? t("settings.about.copyFailed") : t("settings.about.copyDiagnostics")}
            </Button>
          }
        >
          <FactList
            rows={[
              { label: t("settings.about.system"), value: system },
              { label: t("settings.about.webview"), value: info?.webview ?? NO_VALUE },
              { label: t("settings.about.tauri"), value: info?.tauri || NO_VALUE },
              { label: t("settings.deskId"), value: endpoints?.deskId ?? NO_VALUE },
              { label: t("settings.keys.sentinelApi"), value: endpoints?.apiBase ?? NO_VALUE },
              { label: t("settings.relay"), value: endpoints?.relayUrl ?? NO_VALUE },
              { label: t("accounts.binanceOrders"), value: endpoints?.binanceFuturesBase ?? NO_VALUE },
              { label: t("settings.about.dataDir"), value: info?.dataDir ?? NO_VALUE },
            ]}
          />
          {info?.dataDir ? (
            <div className="ae-toolbar">
              <Button variant="secondary" size="sm" onClick={() => void openDataDir().catch(() => undefined)}>
                <Icon name="folder" size={12} />
                {t("settings.about.openFolder")}
              </Button>
            </div>
          ) : null}
        </Panel>
      </div>

      <Panel title={t("settings.about.dataTitle")}>
        <div className="ae-dataflow">
          <div className="ae-dataflow__col">
            <h3 className="ae-dataflow__title">{t("settings.about.local.title")}</h3>
            <ul className="ae-setlist">
              {(["keys", "bots", "prefs"] as const).map((k) => (
                <li key={k}>{t(`settings.about.local.${k}`)}</li>
              ))}
            </ul>
          </div>
          <div className="ae-dataflow__col">
            <h3 className="ae-dataflow__title">{t("settings.about.sentinel.title")}</h3>
            <ul className="ae-setlist">
              {(["signIn", "stream", "market"] as const).map((k) => (
                <li key={k}>{t(`settings.about.sentinel.${k}`)}</li>
              ))}
            </ul>
            <ul className="ae-setlist" data-mark="no">
              <li>{t("settings.about.sentinel.none")}</li>
            </ul>
          </div>
          <div className="ae-dataflow__col">
            <h3 className="ae-dataflow__title">{t("settings.about.exchanges.title")}</h3>
            <ul className="ae-setlist">
              {(["public", "keyed"] as const).map((k) => (
                <li key={k}>{t(`settings.about.exchanges.${k}`)}</li>
              ))}
            </ul>
          </div>
        </div>
      </Panel>

      <div className="ae-setgrid">
        <Panel title={t("settings.about.licenceTitle")}>
          <p className="ae-muted">{t("settings.about.licence")}</p>
          <p className="ae-subtle">{t("settings.about.builtWith")}</p>
          <div className="ae-toolbar">
            <ExternalButton link="source">{t("settings.about.source")}</ExternalButton>
            <ExternalButton link="releases">{t("settings.about.releases")}</ExternalButton>
            <ExternalButton link="licence">{t("settings.about.licenceText")}</ExternalButton>
          </div>
        </Panel>

        <Panel title={t("settings.about.contactTitle")}>
          <ul className="ae-sethelp">
            <li>
              <Link to="/guide" className="ae-link">
                {t("settings.help.guide")}
              </Link>
            </li>
            <li>
              <Link to="/faq" className="ae-link">
                {t("settings.help.faq")}
              </Link>
            </li>
            <li>
              <button type="button" className="ae-link ae-sethelp__ext" onClick={() => void openHelpLink("userGuide").catch(() => undefined)}>
                {t("settings.help.userGuide")}
                <Icon name="external" size={12} />
              </button>
            </li>
            <li>
              <button type="button" className="ae-link ae-sethelp__ext" onClick={() => void openHelpLink("support").catch(() => undefined)}>
                {t("settings.about.supportEmail")}
                <Icon name="external" size={12} />
              </button>
            </li>
          </ul>
          <h3 className="ae-dataflow__title">{t("settings.about.securityTitle")}</h3>
          <p className="ae-subtle">{t("settings.about.security")}</p>
          <div className="ae-toolbar">
            <ExternalButton link="security">{t("settings.about.reportSecurity")}</ExternalButton>
          </div>
        </Panel>
      </div>
    </>
  );
}
