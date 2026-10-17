/**
 * Assistant (mirrors src-tauri/src/assistant): a chat over the desk's data on
 * the user's own OpenRouter key. The key goes to Rust once (setKey) and is
 * kept in the OS keychain; no call here returns it. Tools are read-only plus
 * a bot DRAFT the user opens in New bot and confirms.
 */

import { devMock, inTauri, invoke } from "../bridge";

export interface PromptVersion {
  savedAtMs: number;
  text: string;
}

export interface ToolUse {
  name: string;
  /** "ok" | "draft" | a refusal code ("unknownTool", "draftUnknownPreset", ...). */
  outcome: string;
}

export interface Usage {
  promptTokens: number;
  completionTokens: number;
  /** As OpenRouter reports it (USD); null when it reported none. */
  costUsd: number | null;
}

export interface BotDraft {
  botType: "dca" | "grid" | "signal_futures" | "signal_spot";
  presetId: string | null;
  symbol: string | null;
  note: string | null;
  /** New bot form path with this draft. */
  path: string;
}

export interface ChatEntry {
  role: "user" | "assistant";
  text: string;
  atMs: number;
  model: string | null;
  usage: Usage | null;
  tools: ToolUse[];
  drafts: BotDraft[];
  error: string | null;
  truncated: boolean;
}

export interface AssistantStatus {
  keyPresent: boolean;
  model: string | null;
  prompt: string;
  promptIsDefault: boolean;
  defaultPrompt: string;
  guard: string;
  promptVersion: string;
  history: PromptVersion[];
  conversation: ChatEntry[];
  busy: boolean;
  toolNames: string[];
}

export interface ModelInfo {
  id: string;
  name: string;
  contextLength: number | null;
  promptPerMtok: number | null;
  completionPerMtok: number | null;
}

type Mock = typeof import("./assistant.mock");
const mock = <T>(run: (m: Mock) => Promise<T>): Promise<T> => devMock(() => import("./assistant.mock"), run);

export const assistantStatus = (): Promise<AssistantStatus> =>
  inTauri() ? invoke("assistant_status") : mock((m) => m.status());
export const assistantSetKey = (key: string): Promise<AssistantStatus> =>
  inTauri() ? invoke("assistant_set_key", { key }) : mock((m) => m.setKey(key));
export const assistantRemoveKey = (): Promise<AssistantStatus> =>
  inTauri() ? invoke("assistant_remove_key") : mock((m) => m.removeKey());
export const assistantModels = (refresh = false): Promise<ModelInfo[]> =>
  inTauri() ? invoke("assistant_models", { refresh }) : mock((m) => m.models());
export const assistantSetModel = (model: string): Promise<AssistantStatus> =>
  inTauri() ? invoke("assistant_set_model", { model }) : mock((m) => m.setModel(model));
export const assistantSavePrompt = (text: string): Promise<AssistantStatus> =>
  inTauri() ? invoke("assistant_save_prompt", { text }) : mock((m) => m.savePrompt(text));
export const assistantResetPrompt = (): Promise<AssistantStatus> =>
  inTauri() ? invoke("assistant_reset_prompt") : mock((m) => m.resetPrompt());
export const assistantRestorePrompt = (savedAtMs: number): Promise<AssistantStatus> =>
  inTauri() ? invoke("assistant_restore_prompt", { savedAtMs }) : mock((m) => m.restorePrompt(savedAtMs));
export const assistantClearConversation = (): Promise<AssistantStatus> =>
  inTauri() ? invoke("assistant_clear_conversation") : mock((m) => m.clearConversation());
export const assistantOpenKeysPage = (): Promise<void> =>
  inTauri() ? invoke("assistant_open_keys_page") : mock(async () => void window.open("https://openrouter.ai/keys", "_blank", "noopener,noreferrer"));
export const assistantSend = (text: string): Promise<AssistantStatus> =>
  inTauri() ? invoke("assistant_send", { text }) : mock((m) => m.send(text));
