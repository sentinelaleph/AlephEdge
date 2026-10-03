// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Button } from "@/components/ui/Button/Button";
import { DataTable, type DataColumn } from "./DataTable";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

interface Row {
  id: string;
  sym: string;
  note?: string;
  bad?: boolean;
}

const ROWS: Row[] = [
  { id: "a", sym: "BTCUSDT", note: "hint" },
  { id: "b", sym: "ETHUSDT", note: "refused", bad: true },
  { id: "c", sym: "SOLUSDT" },
];

const COLUMNS: DataColumn<Row>[] = [
  { id: "sym", header: "Symbol", cell: (r) => r.sym },
  { id: "ev", header: "Evidence", headerTip: "What the score means", numeric: true, cell: () => 70 },
  { id: "details", header: "Details", detailLabel: "All details", detailOnly: true, wrap: true, cell: (r) => `${r.sym} details` },
];

let root: Root | null = null;
let host: HTMLDivElement | null = null;

async function mount(node: React.ReactNode) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root!.render(node));
  return host;
}

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
});

const key = (el: Element, k: string) =>
  act(async () => {
    el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
  });

describe("DataTable", () => {
  it("rows are one tab stop; Enter / Space activate; arrows move focus", async () => {
    const onActivate = vi.fn();
    const el = await mount(
      <DataTable label="t" columns={COLUMNS} rows={ROWS} rowKey={(r) => r.id} onRowActivate={onActivate} isSelected={(r) => r.id === "b"} />,
    );
    const trs = Array.from(el.querySelectorAll<HTMLTableRowElement>("tbody tr[data-rowkey]"));
    expect(trs.map((tr) => tr.tabIndex)).toEqual([-1, 0, -1]); // the selected row holds the tab stop
    expect(trs[1].getAttribute("aria-current")).toBe("true");

    await key(trs[1], "Enter");
    expect(onActivate).toHaveBeenLastCalledWith(ROWS[1]);
    await key(trs[1], " ");
    expect(onActivate).toHaveBeenCalledTimes(2);

    trs[1].focus();
    await key(trs[1], "ArrowDown");
    expect(document.activeElement).toBe(trs[2]);
    const after = Array.from(el.querySelectorAll<HTMLTableRowElement>("tbody tr[data-rowkey]"));
    expect(after.map((tr) => tr.tabIndex)).toEqual([-1, -1, 0]);
    await key(after[2], "Home");
    expect(document.activeElement).toBe(after[0]);
  });

  it("a click on the row activates it; a click on its own control does not", async () => {
    const onActivate = vi.fn();
    const onButton = vi.fn();
    const el = await mount(
      <DataTable
        label="t"
        columns={COLUMNS}
        rows={ROWS}
        rowKey={(r) => r.id}
        onRowActivate={onActivate}
        actions={() => (
          <Button size="xs" variant="secondary" onClick={onButton}>
            Stop
          </Button>
        )}
      />,
    );
    const tr = el.querySelector<HTMLTableRowElement>('tbody tr[data-rowkey="a"]')!;
    await act(async () => tr.querySelector("td")!.click());
    expect(onActivate).toHaveBeenCalledWith(ROWS[0]);
    await act(async () => tr.querySelector<HTMLButtonElement>(".ae-dt__actions button")!.click());
    expect(onButton).toHaveBeenCalledTimes(1);
    expect(onActivate).toHaveBeenCalledTimes(1);
  });

  it("a row without onRowActivate is not focusable", async () => {
    const el = await mount(<DataTable label="t" columns={COLUMNS} rows={ROWS} rowKey={(r) => r.id} />);
    const tr = el.querySelector<HTMLTableRowElement>('tbody tr[data-rowkey="a"]')!;
    expect(tr.hasAttribute("tabindex")).toBe(false);
  });

  it("headerTip makes a focusable header; detailOnly never becomes a column", async () => {
    const el = await mount(<DataTable label="t" columns={COLUMNS} rows={ROWS} rowKey={(r) => r.id} />);
    const heads = Array.from(el.querySelectorAll("thead th")).map((th) => th.textContent);
    expect(heads).not.toContain("Details");
    expect(heads).toContain("Evidence");
    const tip = el.querySelector(".ae-dt__htip");
    expect(tip?.textContent).toBe("Evidence");
    expect(tip?.getAttribute("tabindex")).toBe("0");

    // The expander shows the detail-only value under its detail label.
    await act(async () => el.querySelector<HTMLButtonElement>(".ae-dt__expbtn")!.click());
    expect(el.querySelector(".ae-dt__detaillist dt")?.textContent).toBe("All details");
    expect(el.querySelector(".ae-dt__detaillist dd")?.textContent).toBe("BTCUSDT details");
  });

  it("row notes are muted unless rowTone marks the row", async () => {
    const el = await mount(
      <DataTable
        label="t"
        columns={COLUMNS}
        rows={ROWS}
        rowKey={(r) => r.id}
        rowNote={(r) => r.note ?? null}
        rowTone={(r) => (r.bad ? "danger" : undefined)}
      />,
    );
    const notes = Array.from(el.querySelectorAll(".ae-dt__note"));
    expect(notes).toHaveLength(2);
    expect(notes[0].hasAttribute("data-tone")).toBe(false);
    expect(notes[1].getAttribute("data-tone")).toBe("danger");
  });

  it("a wrap column marks its cells", async () => {
    const wrapCols: DataColumn<Row>[] = [COLUMNS[0], { id: "reason", header: "Reason", wrap: true, cell: (r) => r.sym }];
    const el = await mount(<DataTable label="t" columns={wrapCols} rows={ROWS} rowKey={(r) => r.id} />);
    expect(el.querySelectorAll("td[data-wrap]")).toHaveLength(3);
  });
});

describe("Button disabledReason", () => {
  it("a disabled button with a reason sits in a focusable tooltip wrapper", async () => {
    const el = await mount(
      <Button size="xs" disabled disabledReason="No key">
        Start
      </Button>,
    );
    const anchor = el.querySelector(".ae-tip-anchor");
    expect(anchor?.getAttribute("tabindex")).toBe("0");
    expect(anchor?.hasAttribute("data-proxy")).toBe(true);
    expect(anchor?.querySelector("button")?.disabled).toBe(true);
  });

  it("an enabled button ignores the reason (no wrapper)", async () => {
    const el = await mount(
      <Button size="xs" disabledReason="No key">
        Start
      </Button>,
    );
    expect(el.querySelector(".ae-tip-anchor")).toBeNull();
    expect(el.querySelector("button")).not.toBeNull();
  });
});
