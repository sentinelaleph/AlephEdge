import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { navigate, useRoute } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { errorMessage } from "@/lib/ipc/bridge";
import {
  strategyCreate,
  strategyDefault,
  strategyDetail,
  strategyPresets,
  strategyStart,
  type Preset,
  type StrategyConfig,
  type StrategyKind,
} from "@/lib/ipc/strategy/strategy";
import { StrategyForm, type FormActions, type FormPreview } from "./strategy/StrategyForm";
import { StrategyPreviewPanel } from "./strategy/StrategyPreviewPanel";

const DEFAULT_SYMBOL = "BTCUSDT";

/**
 * #/bots/dca/new and #/bots/grid/new (?preset=<id> | ?from=<botId>).
 * Defaults come from Rust (strategy_default), a preset from strategy_presets,
 * a clone from strategy_detail. The bot is created Stopped unless the user
 * picks "Create and start". PAPER ONLY.
 */
export function StrategyCreatePage({ kind }: { kind: StrategyKind }) {
  const { t } = useTranslation();
  const route = useRoute();
  const { strategy } = useDeskContext();
  const presetId = route.query.get("preset");
  const from = route.query.get("from");
  const listPath = `/bots/${kind}`;
  const [initial, setInitial] = useState<StrategyConfig | null>(null);
  const [preset, setPreset] = useState<Preset | null>(null);
  /** i18n key + params of a failed load (translated at render, so a language switch never resets the form). */
  const [loadError, setLoadError] = useState<{ key: string; params?: Record<string, string> } | null>(null);
  const [summary, setSummary] = useState<FormPreview | null>(null);
  const [formActions, setFormActions] = useState<FormActions | null>(null);
  const count = strategy.bots?.filter((b) => b.kind === kind).length ?? 0;

  useEffect(() => {
    let alive = true;
    setInitial(null);
    setLoadError(null);
    const load = async (): Promise<{ cfg: StrategyConfig; preset: Preset | null }> => {
      if (presetId) {
        const p = (await strategyPresets()).find((x) => x.id === presetId);
        if (!p) throw { key: "strategy.form.presetUnknown", params: { id: presetId } };
        if (p.config.params.kind !== kind) throw { key: "strategy.form.presetKind", params: { id: presetId } };
        return { cfg: { ...p.config, presetId: p.id }, preset: p };
      }
      if (from) {
        const d = await strategyDetail(from);
        if (d.config.params.kind !== kind) throw { key: "botDetail.notFound" };
        return { cfg: { ...d.config, name: t("strategy.form.copyName", { name: d.config.name }).slice(0, 40) }, preset: null };
      }
      const cfg = await strategyDefault(kind, "binance", DEFAULT_SYMBOL);
      return { cfg: { ...cfg, name: `${kind === "dca" ? "DCA" : "Grid"} ${DEFAULT_SYMBOL} #${count + 1}` }, preset: null };
    };
    load()
      .then((r) => {
        if (!alive) return;
        setInitial(r.cfg);
        setPreset(r.preset);
      })
      .catch((e: unknown) => {
        if (!alive) return;
        if (e && typeof e === "object" && "key" in e) setLoadError(e as { key: string; params?: Record<string, string> });
        else setLoadError({ key: `strategy.errors.${errorMessage(e, "botUnknown").split("|")[0]}` });
      });
    return () => {
      alive = false;
    };
    // `count` and `t` only name a fresh bot; a refreshed list or a language
    // switch must not reset the form.
  }, [kind, presetId, from]);

  const onSubmit = useCallback(
    async (cfg: StrategyConfig, startAfter: boolean): Promise<string | null> => {
      try {
        const view = await strategyCreate(cfg);
        let startErr: string | null = null;
        if (startAfter) {
          try {
            await strategyStart(view.id);
          } catch (e) {
            startErr = errorMessage(e, "botUnknown");
          }
        }
        await strategy.refresh();
        navigate(startErr ? `/bots/${view.id}?err=${encodeURIComponent(startErr)}` : `/bots/${view.id}`);
        return null;
      } catch (e) {
        return errorMessage(e, "botUnknown");
      }
    },
    [strategy],
  );

  const title = t(kind === "dca" ? "page.dcaNew" : "page.gridNew");
  const crumbs = [
    { label: t("nav.groups.bots"), to: "/bots" },
    { label: t(kind === "dca" ? "nav.dcaBots" : "nav.gridBots"), to: listPath },
  ];

  if (loadError) {
    return (
      <PageShell title={title} crumbs={crumbs} single>
        <EmptyState
          tone="error"
          title={t(loadError.key, { ...loadError.params, defaultValue: loadError.key.split(".").pop() })}
          actions={
            <Button variant="secondary" size="sm" onClick={() => navigate(listPath)}>
              {t(kind === "dca" ? "nav.dcaBots" : "nav.gridBots")}
            </Button>
          }
        />
      </PageShell>
    );
  }

  const busy = !formActions || !formActions.canSubmit;
  return (
    <PageShell
      title={title}
      crumbs={crumbs}
      secondary={
        initial ? (
          <>
            <Button variant="ghost" size="sm" onClick={() => navigate(listPath)}>
              {t("states.cancel")}
            </Button>
            <Button variant="secondary" size="sm" disabled={busy} onClick={() => formActions?.submit(true)}>
              {t("strategy.form.createAndStart")}
            </Button>
          </>
        ) : null
      }
      primary={
        initial ? (
          <Button size="sm" disabled={busy} aria-busy={formActions?.saving || undefined} onClick={() => formActions?.submit(false)}>
            {t("strategy.form.create")}
          </Button>
        ) : null
      }
      right={
        initial ? (
          <StrategyPreviewPanel
            preview={summary?.preview ?? null}
            error={summary?.error ?? null}
            cfg={summary?.cfg ?? initial}
            available={summary?.available ?? null}
          />
        ) : null
      }
    >
      {initial ? (
        <StrategyForm
          key={`${presetId ?? ""}|${from ?? ""}`}
          kind={kind}
          initial={initial}
          preset={preset}
          onSubmit={onSubmit}
          onCancel={() => navigate(listPath)}
          onPreview={setSummary}
          onActions={setFormActions}
        />
      ) : (
        <p className="ae-subtle">{t("workspace.loading")}</p>
      )}
    </PageShell>
  );
}
