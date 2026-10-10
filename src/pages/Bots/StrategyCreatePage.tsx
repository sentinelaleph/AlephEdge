import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Link, navigate, useRoute } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { localeForLanguage } from "@/i18n";
import { formatNumber } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/bridge";
import {
  strategyCreate,
  strategyDefault,
  strategyDetail,
  strategyPresets,
  strategyStart,
  strategyValidate,
  type Preset,
  type StrategyConfig,
  type StrategyKind,
  type StrategyRiskView,
} from "@/lib/ipc/strategy/strategy";
import { StrategyForm, type FormActions, type FormPreview } from "./strategy/StrategyForm";
import { StrategyPreviewPanel } from "./strategy/StrategyPreviewPanel";

const DEFAULT_SYMBOL = "BTCUSDT";

/** Strategy budget free under the DCA/Grid cap now (what Start needs). */
export function freeUnderCap(risk: StrategyRiskView): number {
  return (risk.balance * risk.budgetCapPct) / 100 - risk.reservedBudget;
}

/**
 * The opening budget: a preset or the blank form starts at 1000 USDT, five
 * times the fresh-install cap (200). Lower it to what is free under the cap
 * (whole USDT when that still clears the minimum) when that budget still
 * clears the 5 USDT minimum order; otherwise keep it and report the
 * shortfall (`short`), since no budget the cap allows would pass.
 */
export function openingBudget(budget: number, free: number, minBudget: number | null): { budget: number; short: boolean } {
  if (budget <= free + 1e-9) return { budget, short: false };
  if (minBudget !== null && free + 1e-9 < minBudget) return { budget, short: true };
  const whole = Math.floor(free);
  const fit = minBudget === null || whole >= minBudget ? whole : Math.floor(free * 100) / 100;
  return fit > 0 ? { budget: fit, short: false } : { budget, short: true };
}

/** The ?symbol= pair as the pair field would hold it ("ethusdt" -> "ETHUSDT"); null when empty. */
export function querySymbol(raw: string | null): string | null {
  const s = (raw ?? "").toUpperCase().replace(/[^A-Z0-9]/g, "");
  return s || null;
}

/**
 * The opening config of a template, on its own pair or on `symbol`. A
 * BTC-only template was simulated on BTCUSDT alone and opens on no other
 * pair. Throws an i18n key + params, as the page's load does.
 */
export function presetStartConfig(p: Preset, kind: StrategyKind, symbol: string | null): StrategyConfig {
  if (p.config.params.kind !== kind) throw { key: "strategy.form.presetKind", params: { id: p.id } };
  if (p.universe.rule === "btcOnly") {
    if (symbol && symbol !== "BTCUSDT") throw { key: "strategy.form.presetPairOnly", params: { id: p.id, symbol } };
    return { ...p.config, symbol: "BTCUSDT", presetId: p.id };
  }
  return { ...p.config, symbol: symbol ?? p.config.symbol, presetId: p.id };
}

/**
 * #/bots/dca/new and #/bots/grid/new (?preset=<id> | ?from=<botId>, with
 * ?symbol=<pair> on a preset or a blank form).
 * Defaults come from Rust (strategy_default), a preset from strategy_presets,
 * a clone from strategy_detail. The bot is created Stopped unless the user
 * picks "Create and start". PAPER ONLY.
 */
export function StrategyCreatePage({ kind }: { kind: StrategyKind }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const route = useRoute();
  const { strategy } = useDeskContext();
  const presetId = route.query.get("preset");
  const from = route.query.get("from");
  const symbol = querySymbol(route.query.get("symbol"));
  const listPath = `/bots/${kind}`;
  const [initial, setInitial] = useState<StrategyConfig | null>(null);
  const [preset, setPreset] = useState<Preset | null>(null);
  const [templates, setTemplates] = useState<Preset[] | null>(null);
  /** The loaded config before the opening budget is fitted to the cap. */
  const [loaded, setLoaded] = useState<{ cfg: StrategyConfig; minBudget: number | null } | null>(null);
  /** The config's minimum budget is above what is free under the cap. */
  const [shortfall, setShortfall] = useState<{ min: number; free: number } | null>(null);
  /** i18n key + params of a failed load (translated at render, so a language switch never resets the form). */
  const [loadError, setLoadError] = useState<{ key: string; params?: Record<string, string> } | null>(null);
  const [summary, setSummary] = useState<FormPreview | null>(null);
  const [formActions, setFormActions] = useState<FormActions | null>(null);
  const count = strategy.bots?.filter((b) => b.kind === kind).length ?? 0;

  useEffect(() => {
    let alive = true;
    setInitial(null);
    setLoaded(null);
    setShortfall(null);
    setLoadError(null);
    const load = async (): Promise<{ cfg: StrategyConfig; preset: Preset | null; templates: Preset[] | null }> => {
      // The templates name the passed ones on a blank form; there a failed
      // read only drops that note. A preset link needs them.
      const all = await strategyPresets().catch((e: unknown) => (presetId ? Promise.reject(e) : null));
      if (presetId) {
        const p = all?.find((x) => x.id === presetId);
        if (!p) throw { key: "strategy.form.presetUnknown", params: { id: presetId } };
        return { cfg: presetStartConfig(p, kind, symbol), preset: p, templates: all };
      }
      if (from) {
        const d = await strategyDetail(from);
        if (d.config.params.kind !== kind) throw { key: "botDetail.notFound" };
        // A clone of a preset bot keeps the parity check: a changed setting
        // drops the link, an unchanged one keeps it.
        const p = d.config.presetId ? (all?.find((x) => x.id === d.config.presetId) ?? null) : null;
        return { cfg: { ...d.config, name: t("strategy.form.copyName", { name: d.config.name }).slice(0, 40) }, preset: p, templates: all };
      }
      const pair = symbol ?? DEFAULT_SYMBOL;
      const cfg = await strategyDefault(kind, "binance", pair);
      return { cfg: { ...cfg, name: `${kind === "dca" ? "DCA" : "Grid"} ${pair} #${count + 1}` }, preset: null, templates: all };
    };
    load()
      .then(async (r) => {
        const minBudget = await strategyValidate(r.cfg)
          .then((v) => v.minBudget)
          .catch(() => null);
        if (!alive) return;
        setLoaded({ cfg: r.cfg, minBudget });
        setPreset(r.preset);
        setTemplates(r.templates);
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
  }, [kind, presetId, from, symbol]);

  // The opening budget is fitted once, when both the config and the risk
  // settings are known (a failed risk read keeps the config as loaded).
  const risk = strategy.risk;
  const riskFailed = risk === null && strategy.loadError !== null;
  useEffect(() => {
    if (!loaded || initial || (!risk && !riskFailed)) return;
    if (!risk) {
      setInitial(loaded.cfg);
      return;
    }
    const free = freeUnderCap(risk);
    const fit = openingBudget(loaded.cfg.budget, free, loaded.minBudget);
    setInitial({ ...loaded.cfg, budget: fit.budget });
    setShortfall(fit.short && loaded.minBudget !== null ? { min: loaded.minBudget, free: Math.max(0, free) } : null);
  }, [loaded, initial, risk, riskFailed]);

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
  const usdt = (v: number) => formatNumber(v, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
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
            <Button variant="secondary" size="sm" disabled={!formActions || !formActions.canStart} onClick={() => formActions?.submit(true)}>
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
      {initial && shortfall ? (
        <p className="ae-banner" data-tone="warn" role="note">
          {t("strategy.form.budgetShort", { min: usdt(shortfall.min), free: usdt(shortfall.free) })}{" "}
          <Link to="/risk" className="ae-link">
            {t("nav.risk")}
          </Link>
        </p>
      ) : null}
      {initial ? (
        <StrategyForm
          key={`${presetId ?? ""}|${from ?? ""}|${symbol ?? ""}`}
          kind={kind}
          initial={initial}
          preset={preset}
          templates={templates}
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
