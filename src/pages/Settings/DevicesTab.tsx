import { useTranslation } from "react-i18next";
import { PairingPanel } from "@/components/Link/PairingPanel/PairingPanel";
import { Panel } from "@/components/ui/Panel/Panel";

/** What the desk carries out for a paired phone (aleph-link Command). */
const CAN = ["status", "stop", "close", "start"] as const;
/** Commands that do not exist in the protocol at all. */
const CANNOT = ["settings", "keys", "live", "funds"] as const;
const SECURITY = ["qr", "signed", "relay", "local"] as const;

/** Settings, Devices: phone pairing, what a phone may do, and how it is protected. */
export function DevicesTab() {
  const { t } = useTranslation();
  return (
    <>
      <PairingPanel />
      <p className="ae-subtle">{t("settings.devices.appNote")}</p>
      <div className="ae-setgrid">
        <Panel title={t("settings.devices.canTitle")}>
          <ul className="ae-setlist" data-mark="yes">
            {CAN.map((k) => (
              <li key={k}>{t(`settings.devices.can.${k}`)}</li>
            ))}
          </ul>
        </Panel>
        <Panel title={t("settings.devices.cannotTitle")}>
          <ul className="ae-setlist" data-mark="no">
            {CANNOT.map((k) => (
              <li key={k}>{t(`settings.devices.cannot.${k}`)}</li>
            ))}
          </ul>
        </Panel>
      </div>
      <Panel title={t("settings.devices.securityTitle")}>
        <ul className="ae-setlist">
          {SECURITY.map((k) => (
            <li key={k}>{t(`settings.devices.security.${k}`)}</li>
          ))}
        </ul>
      </Panel>
    </>
  );
}
