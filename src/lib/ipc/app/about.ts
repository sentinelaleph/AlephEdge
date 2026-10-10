/**
 * Settings, About: this installation (app/about.rs) and the fixed places the
 * app may open outside itself. The webview cannot open a URL or a path on its
 * own; Rust opens one of a fixed list.
 */

import { inTauri, invoke } from "../bridge";

export interface AppInfo {
  version: string;
  os: string;
  arch: string;
  tauri: string;
  webview: string | null;
  liveBuild: boolean;
  identifier: string;
  dataDir: string | null;
}

export type HelpLink = "userGuide" | "webAccount" | "webBilling" | "webApiKeys" | "source" | "releases" | "licence" | "support" | "security";

/** Shown in `npm run dev` only, marked as a browser preview so it cannot pass for an installation. */
const BROWSER: AppInfo = {
  version: "dev",
  os: "browser",
  arch: "",
  tauri: "",
  webview: null,
  liveBuild: false,
  identifier: "com.sentinelaleph.edge",
  dataDir: null,
};

let browserInfo: AppInfo = BROWSER;

/** Dev screenshot harness only: the sample installation its About screen shows. */
export function setBrowserAppInfo(info: AppInfo): void {
  browserInfo = info;
}

export async function appInfo(): Promise<AppInfo> {
  if (!inTauri()) return browserInfo;
  return invoke<AppInfo>("app_info");
}

export async function openHelpLink(link: HelpLink): Promise<void> {
  if (!inTauri()) return;
  return invoke<void>("open_help_link", { link });
}

export async function openDataDir(): Promise<void> {
  if (!inTauri()) return;
  return invoke<void>("open_data_dir");
}

/** The TESTNET build has its own bundle identifier (tauri.testnet.conf.json). */
export function isTestnetBuild(info: AppInfo): boolean {
  return info.identifier.endsWith(".testnet");
}
