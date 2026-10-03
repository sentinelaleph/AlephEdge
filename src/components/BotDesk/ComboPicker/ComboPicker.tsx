import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Chip, type ChipTone } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { localeForLanguage } from "@/i18n";
import { formatPercent, NO_VALUE } from "@/lib/format";
import { fetchComboCatalog, type ComboEntry } from "@/lib/ipc/signal/combos";
import { errorMessage } from "@/lib/ipc/bridge";
import "./ComboPicker.css";

interface ComboPickerProps {
  /** Selected combo ids. Empty = no combo filter (trade every signal). */
  value: string[];
  onChange: (ids: string[]) => void;
  disabled?: boolean;
}

const STATUS_TONE: Record<string, ChipTone> = { best: "success", candidate: "neutral", disabled: "danger" };

/**
 * Pick which named setups this bot is allowed to trade.
 *
 * Every row prints BOTH records, published and backtest, each with its
 * sample size. On this book they disagree by more than 20 percentage points
 * on the best-looking combo; a user who sees only one figure is handed the
 * same false confidence a retrospective 76.2% short once handed this project
 * before it forward-tested at 41.8%. The caution sits on both headers.
 *
 * Selecting nothing is a valid, visible state ("All"), not an empty form.
 */
export function ComboPicker({ value, onChange, disabled }: ComboPickerProps) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const [entries, setEntries] = useState<ComboEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    fetchComboCatalog()
      .then((rows) => {
        if (alive) {
          setEntries(rows);
          setError(null);
        }
      })
      .catch((e) => {
        // "Could not load" is not "no combos exist": an empty list on a
        // network failure would quietly say this desk has nothing to offer.
        if (alive) setError(errorMessage(e, t("bots.comboLoadFailed")));
      });
    return () => {
      alive = false;
    };
  }, [t]);

  const toggle = (id: string) => {
    if (disabled) return;
    onChange(value.includes(id) ? value.filter((v) => v !== id) : [...value, id]);
  };

  // A rate is printed only over a sample: "0%" from n=0 is an empty cell wearing a number.
  const record = (rec: ComboEntry["live"]) =>
    rec.n === 0 ? (
      <span className="ae-combos__none" title={t("bots.comboNoSample")}>
        {NO_VALUE}
      </span>
    ) : (
      <>
        {formatPercent(rec.winRate * 100, locale, 1)} <span className="ae-combos__n">{t("bots.comboOfN", { n: rec.n })}</span>
      </>
    );

  const columns: DataColumn<ComboEntry>[] = [
    {
      id: "on",
      header: "",
      width: "1%",
      cell: (c) => (
        <input
          type="checkbox"
          className="ae-combos__check"
          aria-label={c.name}
          checked={value.includes(c.id)}
          disabled={disabled}
          onChange={() => toggle(c.id)}
        />
      ),
    },
    {
      id: "name",
      header: t("table.name"),
      keep: true,
      cell: (c) => (
        <span className="ae-combos__name" title={c.sequenceHuman}>
          {c.name}
        </span>
      ),
    },
    {
      id: "status",
      header: t("table.state"),
      priority: 2,
      cell: (c) => <Chip tone={STATUS_TONE[c.status] ?? "neutral"}>{t(`bots.comboStatus.${c.status}`, c.status)}</Chip>,
    },
    { id: "live", header: t("bots.comboLive"), headerTip: t("bots.comboRecordCaution"), numeric: true, cell: (c) => record(c.live) },
    {
      id: "backtest",
      header: t("bots.comboBacktest"),
      headerTip: t("bots.comboRecordCaution"),
      numeric: true,
      priority: 2,
      cell: (c) => record(c.backtest),
    },
  ];

  return (
    <div className="ae-combos">
      <div className="ae-combos__head">
        <span className="ae-field__label">{t("bots.comboWhitelist")}</span>
        <span className="ae-combos__count tabular">
          {value.length === 0 ? t("bots.dir.all") : entries ? `${value.length} / ${entries.length}` : value.length}
        </span>
      </div>
      {error ? (
        <p className="ae-sfield__error" role="alert">
          {error}
        </p>
      ) : (
        <DataTable
          label={t("bots.comboWhitelist")}
          columns={columns}
          rows={entries ?? []}
          rowKey={(c) => c.id}
          loading={entries === null}
          compact
          isSelected={(c) => value.includes(c.id)}
          onRowActivate={disabled ? undefined : (c) => toggle(c.id)}
        />
      )}
    </div>
  );
}
