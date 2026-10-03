import type { StatusKind } from "@/components/ui/StatusChip/StatusChip";
import type { LinkStateView } from "@/lib/ipc/link/link";

/** Phone-link state -> the shared status tone (Settings > Devices, Risk readout). */
export const LINK_STATUS: Record<LinkStateView["state"], StatusKind> = {
  unpaired: "idle",
  connecting: "connecting",
  connected: "connected",
  retrying: "retrying",
  stopped: "disconnected",
};
