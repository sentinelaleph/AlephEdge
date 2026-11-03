import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import { Chip } from "@/components/ui/Chip/Chip";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { NO_VALUE } from "@/lib/format";
import { PINE_LIMITS } from "@/lib/ipc/pine/pine";
import { FieldShell } from "@/pages/Bots/strategy/formControls";
import { shortHash } from "./pineBotModel";
import { engineMessage, errorKindText } from "./pineText";
import type { PineCompileState } from "./usePine";

interface Props {
  name: string;
  script: string;
  compile: PineCompileState;
  /** The saved script's hash (what a Live arm confirmed), or null for a new bot. */
  savedHash: string | null;
  onName: (v: string) => void;
  onScript: (v: string) => void;
}

/**
 * Script: paste it or read a .pine file from disk (read here, nothing is
 * uploaded), then the compile of that text: the indicator title, or the
 * engine's named error with its line and column. The hash is the script's
 * sha256, the one the Live arm confirms.
 */
export function PineScriptPanel({ name, script, compile, savedHash, onName, onScript }: Props) {
  const { t, i18n } = useTranslation();
  const exists = (k: string) => i18n.exists(k);
  const file = useRef<HTMLInputElement>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const k = (x: string) => `botsPine.${x}`;

  const read = async (f: File | undefined) => {
    setFileError(null);
    if (!f) return;
    if (f.size > PINE_LIMITS.maxScriptKb * 1024) return setFileError(t(k("error.scriptSize"), { max: PINE_LIMITS.maxScriptKb }));
    try {
      onScript(await f.text());
    } catch {
      setFileError(t(k("fileUnreadable")));
    }
  };

  const info = compile.info;
  const hash = info?.hash ?? null;
  const changed = savedHash !== null && hash !== null && hash !== savedHash;
  const result = !script.trim() ? (
    <span className="ae-muted">{t(k("compileEmpty"))}</span>
  ) : compile.busy || (!info && !compile.refused) ? (
    <span className="ae-muted">{t(k("compiling"))}</span>
  ) : compile.refused ? (
    <span className="ae-error">{compile.refused}</span>
  ) : info?.ok ? (
    <span role="status">
      <Chip tone="success">{t(k("compileOk"))}</Chip> {info.title ? <strong>{info.title}</strong> : null}{" "}
      <span className="ae-muted">
        {t(info.overlay ? k("overlayOn") : k("overlayOff"))} · {t(k("counts"), { inputs: info.inputs.length, triggers: info.triggers.length })}
      </span>
    </span>
  ) : info?.error ? (
    <span className="ae-error" role="alert">
      {t(k("compileError"), { kind: errorKindText(t, exists, info.error.kind), line: info.error.line, col: info.error.col })}{" "}
      <code className="mono">{engineMessage(t, exists, info.error)}</code>
    </span>
  ) : null;

  return (
    <Panel
      title={t(k("script"))}
      aside={
        <Button size="sm" variant="secondary" onClick={() => file.current?.click()}>
          {t(k("import"))}
        </Button>
      }
    >
      <div className="ae-sform__grid">
        <FieldShell label={t(k("name"))} htmlFor="pine-name">
          <input id="pine-name" className="ae-field__input" value={name} maxLength={40} onChange={(e) => onName(e.target.value)} />
        </FieldShell>
      </div>
      <input
        ref={file}
        type="file"
        accept=".pine,.txt,text/plain"
        hidden
        aria-label={t(k("import"))}
        data-testid="pine-file"
        onChange={(e) => {
          void read(e.target.files?.[0]);
          e.target.value = "";
        }}
      />
      <FieldShell label={t(k("scriptText"))} htmlFor="pine-script" hint={t(k("scriptHint"), { max: PINE_LIMITS.maxScriptKb })} error={fileError}>
        <textarea
          id="pine-script"
          className="ae-field__input mono ae-pine__script"
          spellCheck={false}
          autoComplete="off"
          rows={14}
          value={script}
          onChange={(e) => onScript(e.target.value)}
        />
      </FieldShell>
      <p className="ae-pine__compile">{result}</p>
      <FactList
        rows={[
          {
            label: t(k("hash")),
            value: (
              <span className="mono" title={hash ?? undefined}>
                {hash ? shortHash(hash) : NO_VALUE}
                {changed ? <span className="ae-muted"> · {t(k("hashChanged"), { saved: shortHash(savedHash) })}</span> : null}
              </span>
            ),
          },
        ]}
      />
    </Panel>
  );
}
