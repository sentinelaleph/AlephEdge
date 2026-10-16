/**
 * Foreign-trade watch IPC (mirrors src-tauri/src/app/foreign_fill.rs).
 *
 * In a live build Rust reads the Binance account's fills every minute and
 * flags any this app did not make (a leaked trade key being used). A flag
 * stops new entries on every bot; open positions are left to the user.
 * Paper builds never poll: `liveBuild` is false and the list stays empty.
 *
 * Outside Tauri, `npm run dev` gets an in-memory preview through `devMock`.
 */

import { devMock, inTauri, invoke } from "../bridge";

type Mock = typeof import("./foreignFill.mock");
const mock = <T>(run: (m: Mock) => Promise<T>) => devMock(() => import("./foreignFill.mock"), run);

export interface ForeignFill {
  symbol: string;
  tradeId: number;
  /** 0 when Binance did not return the trade itself. */
  orderId: number;
  /** "BUY" / "SELL"; empty when unknown. */
  side: string;
  qty: number;
  price: number;
  /** When the fill happened (UNIX ms, Binance time). */
  timeMs: number;
  /** When the app noticed it. */
  detectedMs: number;
}

export interface ForeignFillStatus {
  liveBuild: boolean;
  enabled: boolean;
  /** A check ran on the last minute (switched on, Binance trade key unlocked). */
  watching: boolean;
  lastCheckMs: number | null;
  /** Last failed check as an `errors.*` / `preflight.codes.*` code. */
  lastError: string | null;
  /** Unacknowledged fills, oldest first. */
  alarms: ForeignFill[];
}

export function foreignFillStatus(): Promise<ForeignFillStatus> {
  return inTauri() ? invoke<ForeignFillStatus>("foreign_fill_status") : mock((m) => m.mock.status());
}

/** "That was me": clears the alarm. Bots stay stopped until started again. */
export function foreignFillAck(): Promise<ForeignFillStatus> {
  return inTauri() ? invoke<ForeignFillStatus>("foreign_fill_ack") : mock((m) => m.mock.ack());
}

export function foreignFillSetEnabled(on: boolean): Promise<ForeignFillStatus> {
  return inTauri() ? invoke<ForeignFillStatus>("foreign_fill_set_enabled", { on }) : mock((m) => m.mock.setEnabled(on));
}
