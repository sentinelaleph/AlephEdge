/**
 * DEV-ONLY browser preview of the Assistant. No model is called: an answer
 * says so. Starts without a key (the empty state); `?assistant=key` starts
 * with a key and a model so the chat can be reviewed.
 */

import type { AssistantStatus, ChatEntry, ModelInfo } from "./assistant";

const GUARD = "Fixed rules (preview copy): read-only tools plus a bot draft; no tool starts or stops a bot or places an order.";
const DEFAULT_PROMPT = "Answer briefly and precisely, in the user's language. Quote each figure with its n and its as_of time.";

const withKey = typeof window !== "undefined" && new URLSearchParams(window.location.search).get("assistant") === "key";

const state: AssistantStatus = {
  keyPresent: withKey,
  model: withKey ? "deepseek/deepseek-v4.1-flash" : null,
  prompt: DEFAULT_PROMPT,
  promptIsDefault: true,
  defaultPrompt: DEFAULT_PROMPT,
  guard: GUARD,
  promptVersion: "assistant-v1",
  history: [],
  conversation: [],
  busy: false,
  toolNames: ["list_bots", "open_positions", "pnl_summary", "risk_summary", "before_you_trade", "recent_signals", "draft_bot"],
};

const snap = (): Promise<AssistantStatus> => Promise.resolve(structuredClone(state));

export function status() {
  return snap();
}
export function setKey(key: string) {
  if (!key.trim().startsWith("sk-or-") || key.trim().length < 20) return Promise.reject("assistantKeyFormat");
  state.keyPresent = true;
  return snap();
}
export function removeKey() {
  state.keyPresent = false;
  return snap();
}
export function models(): Promise<ModelInfo[]> {
  return Promise.resolve([
    { id: "anthropic/claude-sonnet-4.5", name: "Anthropic: Claude Sonnet 4.5", contextLength: 200000, promptPerMtok: 3, completionPerMtok: 15 },
    { id: "deepseek/deepseek-v4.1-flash", name: "DeepSeek: V4.1 Flash", contextLength: 128000, promptPerMtok: 0.1, completionPerMtok: 0.4 },
    { id: "openai/gpt-5-mini", name: "OpenAI: GPT-5 mini", contextLength: 400000, promptPerMtok: 0.25, completionPerMtok: 2 },
  ]);
}
export function setModel(model: string) {
  state.model = model;
  return snap();
}
export function savePrompt(text: string) {
  if (!text.trim()) return Promise.reject("assistantPromptEmpty");
  state.prompt = text.trim();
  state.promptIsDefault = state.prompt === DEFAULT_PROMPT;
  state.history = [{ savedAtMs: Date.now(), text: state.prompt }, ...state.history].slice(0, 10);
  return snap();
}
export function resetPrompt() {
  return savePrompt(DEFAULT_PROMPT);
}
export function restorePrompt(savedAtMs: number) {
  const v = state.history.find((h) => h.savedAtMs === savedAtMs);
  return v ? savePrompt(v.text) : Promise.reject("assistantPromptUnknownVersion");
}
export function clearConversation() {
  state.conversation = [];
  return snap();
}
export function send(text: string) {
  if (!state.keyPresent) return Promise.reject("assistantNoKey");
  if (!state.model) return Promise.reject("assistantNoModel");
  const user: ChatEntry = { role: "user", text, atMs: Date.now(), model: null, usage: null, tools: [], drafts: [], error: null, truncated: false };
  const wantsBot = /bot|dca|grid/i.test(text);
  const answer: ChatEntry = {
    role: "assistant",
    text: "Browser preview: no model was called, so there is no answer. In the desktop app the selected OpenRouter model answers here.",
    atMs: Date.now(),
    model: state.model,
    usage: null,
    tools: wantsBot ? [{ name: "draft_bot", outcome: "draft" }] : [{ name: "list_bots", outcome: "ok" }],
    drafts: wantsBot ? [{ botType: "dca", presetId: "dca_long_classic", symbol: null, note: "Preview draft", path: "/bots/dca/new?preset=dca_long_classic" }] : [],
    error: null,
    truncated: false,
  };
  state.conversation = [...state.conversation, user, answer];
  return snap();
}

/** Tests: back to the first-run state (with or without a key and model). */
export function reset(key: boolean) {
  Object.assign(state, {
    keyPresent: key,
    model: key ? "deepseek/deepseek-v4.1-flash" : null,
    prompt: DEFAULT_PROMPT,
    promptIsDefault: true,
    history: [],
    conversation: [],
  });
}
