import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { navigate } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { Chip } from "@/components/ui/Chip/Chip";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { TextField } from "@/components/ui/TextField/TextField";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatTableTime, NO_VALUE } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/bridge";
import {
  assistantClearConversation,
  assistantModels,
  assistantOpenKeysPage,
  assistantRemoveKey,
  assistantResetPrompt,
  assistantRestorePrompt,
  assistantSavePrompt,
  assistantSend,
  assistantSetKey,
  assistantSetModel,
  assistantStatus,
  type AssistantStatus,
  type BotDraft,
  type ChatEntry,
  type ModelInfo,
} from "@/lib/ipc/assistant/assistant";
import { costText, draftLabelKey, filterModels, toolSummary } from "./assistantModel";
import "./Assistant.css";

/** The error codes this page words itself (`assistant.errors.*`). */
function codeText(t: (k: string, o?: Record<string, unknown>) => string, code: string): string {
  const [head, detail] = code.split("|");
  return t(`assistant.errors.${head}`, { defaultValue: code, detail });
}

function DraftCard({ d }: { d: BotDraft }) {
  const { t } = useTranslation();
  return (
    <div className="ae-assist__draft" data-testid="assistant-draft">
      <div className="ae-assist__drafthead">
        <Chip tone="accent">{t("assistant.draft.badge")}</Chip>
        <span>{t(draftLabelKey(d.botType))}</span>
      </div>
      <FactList
        rows={[
          { label: t("assistant.draft.template"), value: d.presetId ? t(`strategy.preset.names.${d.presetId}`, { defaultValue: d.presetId }) : NO_VALUE },
          { label: t("assistant.draft.symbol"), value: d.symbol ?? NO_VALUE },
          ...(d.note ? [{ label: t("assistant.draft.note"), value: d.note }] : []),
        ]}
      />
      <p className="ae-subtle">{t("assistant.draft.confirmThere")}</p>
      <Button size="sm" variant="secondary" onClick={() => navigate(d.path)}>
        {t("assistant.draft.open")}
      </Button>
    </div>
  );
}

function Message({ e }: { e: ChatEntry }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const tools = toolSummary(e.tools);
  return (
    <li className="ae-assist__msg" data-role={e.role} data-testid={`assistant-msg-${e.role}`}>
      <div className="ae-assist__msghead">
        <span>{t(e.role === "user" ? "assistant.you" : "assistant.assistant")}</span>
        <span className="ae-subtle tabular">{formatTableTime(e.atMs, locale)}</span>
      </div>
      {e.error ? (
        <p className="ae-assist__error" role="alert">{codeText(t, e.error)}</p>
      ) : (
        <div className="ae-assist__text">{e.text || (e.role === "assistant" ? t("assistant.emptyAnswer") : "")}</div>
      )}
      {e.drafts.map((d, i) => (
        <DraftCard key={`${d.path}:${i}`} d={d} />
      ))}
      {e.role === "assistant" && !e.error ? (
        <p className="ae-assist__meta" data-testid="assistant-cost">
          {[
            e.model ?? NO_VALUE,
            e.usage ? t("assistant.tokens", { input: formatNumber(e.usage.promptTokens, locale), output: formatNumber(e.usage.completionTokens, locale) }) : null,
            costText(t, e.usage, locale),
            tools.read.length ? t("assistant.toolsRead", { list: tools.read.join(", ") }) : null,
            tools.refused.length ? t("assistant.toolsRefused", { list: tools.refused.join(", ") }) : null,
            e.truncated ? t("assistant.truncated") : null,
          ]
            .filter(Boolean)
            .join(" · ")}
        </p>
      ) : null}
    </li>
  );
}

function ModelPicker({ st, onChange, disabled }: { st: AssistantStatus; onChange: (s: AssistantStatus) => void; disabled: boolean }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const [models, setModels] = useState<ModelInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  useEffect(() => {
    assistantModels()
      .then(setModels)
      .catch((e: unknown) => setError(codeText(t, errorMessage(e, "orUnavailable"))));
  }, [t]);
  const shown = useMemo(() => filterModels(models ?? [], query, st.model), [models, query, st.model]);
  const current = models?.find((m) => m.id === st.model) ?? null;
  return (
    <div className="ae-assist__models">
      <TextField label={t("assistant.model.search")} value={query} onChange={(e) => setQuery(e.target.value)} disabled={disabled || !models} />
      <label className="ae-field">
        <span className="ae-field__label">{t("assistant.model.label")}</span>
        <select
          className="ae-field__input"
          value={st.model ?? ""}
          disabled={disabled || !models}
          onChange={(e) => {
            if (e.target.value) assistantSetModel(e.target.value).then(onChange).catch((err: unknown) => setError(codeText(t, errorMessage(err, "assistantModelInvalid"))));
          }}
        >
          <option value="">{models ? t("assistant.model.choose", { count: models.length }) : t("assistant.model.loading")}</option>
          {shown.map((m) => (
            <option key={m.id} value={m.id}>
              {m.name || m.id}
            </option>
          ))}
        </select>
      </label>
      {current ? (
        <p className="ae-subtle tabular">
          {t("assistant.model.price", {
            input: current.promptPerMtok === null ? NO_VALUE : formatNumber(current.promptPerMtok, locale, { maximumFractionDigits: 3 }),
            output: current.completionPerMtok === null ? NO_VALUE : formatNumber(current.completionPerMtok, locale, { maximumFractionDigits: 3 }),
          })}
        </p>
      ) : null}
      {error ? <p className="ae-error">{error}</p> : null}
    </div>
  );
}

export function AssistantPage() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const [st, setSt] = useState<AssistantStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [key, setKey] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState<string | null>(null);
  const [promptText, setPromptText] = useState("");
  const [promptError, setPromptError] = useState<string | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);
  const listEnd = useRef<HTMLDivElement>(null);

  const apply = useCallback((s: AssistantStatus) => {
    setSt(s);
    setPromptText(s.prompt);
  }, []);

  useEffect(() => {
    assistantStatus()
      .then(apply)
      .catch((e: unknown) => setLoadError(errorMessage(e, t("assistant.errors.loadFailed"))));
  }, [apply, t]);

  useEffect(() => {
    listEnd.current?.scrollIntoView({ block: "end", behavior: "auto" });
  }, [st?.conversation.length]);

  const saveKey = () => {
    setKeyError(null);
    assistantSetKey(key)
      .then((s) => {
        setKey("");
        apply(s);
      })
      .catch((e: unknown) => setKeyError(codeText(t, errorMessage(e, "assistantKeyFormat"))));
  };

  const send = () => {
    const text = draft.trim();
    if (!text || sending) return;
    setSending(true);
    setSendError(null);
    assistantSend(text)
      .then((s) => {
        setDraft("");
        setSt(s);
      })
      .catch((e: unknown) => setSendError(codeText(t, errorMessage(e, "orUnavailable"))))
      .finally(() => setSending(false));
  };

  if (!st) {
    return (
      <PageShell title={t("nav.assistant")} single>
        {loadError ? <EmptyState tone="error" title={t("assistant.errors.loadFailed")} detail={loadError} /> : <EmptyState tone="pending" title={t("assistant.loading")} />}
      </PageShell>
    );
  }

  const promptDirty = promptText.trim() !== st.prompt;
  const left = (
    <>
      <Panel title={t("assistant.key.title")} aside={<Chip tone={st.keyPresent ? "success" : "warn"}>{t(st.keyPresent ? "assistant.key.saved" : "assistant.key.none")}</Chip>}>
        <p className="ae-subtle">{t("assistant.key.where")}</p>
        <TextField
          label={t(st.keyPresent ? "assistant.key.replace" : "assistant.key.label")}
          type="password"
          autoComplete="off"
          spellCheck={false}
          value={key}
          onChange={(e) => setKey(e.target.value)}
          placeholder="sk-or-…"
        />
        <div className="ae-toolbar">
          <Button size="sm" onClick={saveKey} disabled={!key.trim()}>
            {t("assistant.key.save")}
          </Button>
          {st.keyPresent ? (
            <Button size="sm" variant="secondary" onClick={() => assistantRemoveKey().then(apply)}>
              {t("assistant.key.remove")}
            </Button>
          ) : null}
        </div>
        {keyError ? <p className="ae-error">{keyError}</p> : null}
        <p className="ae-subtle">{t("assistant.key.billing")}</p>
      </Panel>
      <Panel title={t("assistant.model.title")}>
        <ModelPicker st={st} onChange={apply} disabled={sending} />
      </Panel>
      <Panel title={t("assistant.tools.title")}>
        <p className="ae-subtle">{t("assistant.tools.line")}</p>
        <ul className="ae-assist__tools">
          {st.toolNames.map((n) => (
            <li key={n}>
              <code>{n}</code> <span className="ae-subtle">{t(`assistant.tools.names.${n}`, { defaultValue: "" })}</span>
            </li>
          ))}
        </ul>
      </Panel>
    </>
  );

  const right = (
    <Panel title={t("assistant.prompt.title")} aside={<Chip>{t(st.promptIsDefault ? "assistant.prompt.default" : "assistant.prompt.edited")}</Chip>}>
      <details className="ae-assist__guard">
        <summary>{t("assistant.prompt.guard")}</summary>
        <pre>{st.guard}</pre>
      </details>
      <label className="ae-field">
        <span className="ae-field__label">{t("assistant.prompt.label")}</span>
        <textarea className="ae-field__input ae-assist__prompt" rows={12} value={promptText} onChange={(e) => setPromptText(e.target.value)} spellCheck={false} />
      </label>
      <div className="ae-toolbar">
        <Button
          size="sm"
          disabled={!promptDirty}
          onClick={() => {
            setPromptError(null);
            assistantSavePrompt(promptText).then(apply).catch((e: unknown) => setPromptError(codeText(t, errorMessage(e, "assistantPromptEmpty"))));
          }}
        >
          {t("assistant.prompt.save")}
        </Button>
        <Button size="sm" variant="secondary" disabled={st.promptIsDefault && !promptDirty} onClick={() => assistantResetPrompt().then(apply)}>
          {t("assistant.prompt.reset")}
        </Button>
      </div>
      {promptError ? <p className="ae-error">{promptError}</p> : null}
      <p className="ae-subtle">{t("assistant.prompt.cannotUnlock")}</p>
      <h3 className="ae-assist__h3">{t("assistant.prompt.history", { count: st.history.length })}</h3>
      {st.history.length === 0 ? (
        <p className="ae-subtle">{t("assistant.prompt.noHistory")}</p>
      ) : (
        <ul className="ae-list" data-testid="assistant-history">
          {st.history.map((h) => (
            <li key={h.savedAtMs} title={h.text}>
              <span className="tabular">{formatTableTime(h.savedAtMs, locale)}</span>
              <Button size="xs" variant="ghost" disabled={h.text === st.prompt} onClick={() => assistantRestorePrompt(h.savedAtMs).then(apply)}>
                {t("assistant.prompt.restore")}
              </Button>
            </li>
          ))}
        </ul>
      )}
    </Panel>
  );

  const secondary = (
    <Button variant="secondary" size="sm" disabled={st.conversation.length === 0 || sending} onClick={() => setConfirmClear(true)}>
      {t("assistant.clear")}
    </Button>
  );

  return (
    <PageShell title={t("nav.assistant")} secondary={secondary} left={left} right={right}>
      {!st.keyPresent ? (
        <EmptyState
          title={t("assistant.empty.title")}
          detail={t("assistant.empty.detail")}
          actions={
            <Button size="sm" variant="secondary" onClick={() => void assistantOpenKeysPage()}>
              {t("assistant.empty.getKey")}
            </Button>
          }
        />
      ) : (
        <div className="ae-assist">
          <p className="ae-subtle">{t("assistant.scopeLine")}</p>
          {st.conversation.length === 0 ? (
            <EmptyState title={t("assistant.noMessages")} detail={t("assistant.noMessagesDetail")} />
          ) : (
            <ol className="ae-assist__list" aria-live="polite">
              {st.conversation.map((e, i) => (
                <Message key={`${e.atMs}:${i}`} e={e} />
              ))}
            </ol>
          )}
          <div ref={listEnd} />
          <div className="ae-assist__composer">
            <label className="ae-field">
              <span className="ae-field__label">{t("assistant.messageLabel")}</span>
              <textarea
                className="ae-field__input"
                rows={3}
                value={draft}
                disabled={sending}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                    e.preventDefault();
                    send();
                  }
                }}
                placeholder={t("assistant.placeholder")}
              />
            </label>
            <div className="ae-toolbar">
              <span className="ae-subtle">{st.model ? t("assistant.sendsTo", { model: st.model }) : t("assistant.errors.assistantNoModel")}</span>
              <span className="ae-toolbar__spacer" />
              <Button size="sm" onClick={send} disabled={!draft.trim() || sending || !st.model}>
                {sending ? t("assistant.sending") : t("assistant.send")}
              </Button>
            </div>
            {sendError ? <p className="ae-error" role="alert">{sendError}</p> : null}
          </div>
        </div>
      )}
      <ConfirmDialog
        open={confirmClear}
        title={t("assistant.clearTitle")}
        body={t("assistant.clearBody", { count: st.conversation.length })}
        confirmLabel={t("assistant.clear")}
        danger
        onConfirm={() => {
          setConfirmClear(false);
          void assistantClearConversation().then(apply);
        }}
        onCancel={() => setConfirmClear(false)}
      />
    </PageShell>
  );
}
