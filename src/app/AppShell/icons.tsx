/** Line icons for the shell (16px, currentColor). Decorative: always aria-hidden. */

const paths: Record<string, string> = {
  dashboard: "M4 4h7v7H4zM13 4h7v4h-7zM13 10h7v10h-7zM4 13h7v7H4z",
  bots: "M7 8h10a3 3 0 0 1 3 3v5a3 3 0 0 1-3 3H7a3 3 0 0 1-3-3v-5a3 3 0 0 1 3-3zM12 4v4M9 13h.01M15 13h.01",
  signal: "M3 12h4l3-7 4 14 3-7h4",
  dca: "M4 6h16M6 10h12M8 14h8M10 18h4",
  grid: "M4 6h16M4 10h16M4 14h16M4 18h16M8 4v16M16 4v16",
  signals: "M5 12a7 7 0 0 1 14 0M8.5 12a3.5 3.5 0 0 1 7 0M12 12v8",
  presets: "M5 4h14v16l-7-4-7 4z",
  backtest: "M4 19h16M6 15l4-5 3 3 5-7",
  guide: "M5 4.5A1.5 1.5 0 0 1 6.5 3H19v15H6.5A1.5 1.5 0 0 0 5 19.5zM5 19.5A1.5 1.5 0 0 0 6.5 21H19M9 7h6M9 10h6",
  positions: "M4 7h16v12H4zM9 7V5h6v2",
  history: "M12 7v5l3 2M3.5 12a8.5 8.5 0 1 0 2.5-6M3 4v4h4",
  risk: "M12 3l8 4v5c0 4.5-3.4 8.3-8 9-4.6-.7-8-4.5-8-9V7z",
  settings: "M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM19 12l2-1-1-3-2 .3-1.4-1.4.3-2-3-1-1 2h-2l-1-2-3 1 .3 2L6 7.3 4 7 3 10l2 1v2l-2 1 1 3 2-.3 1.4 1.4-.3 2 3 1 1-2h2l1 2 3-1-.3-2 1.4-1.4 2 .3 1-3-2-1z",
  plus: "M12 5v14M5 12h14",
  bell: "M6 16V11a6 6 0 0 1 12 0v5l2 2H4zM10 20a2 2 0 0 0 4 0",
  menu: "M4 6h16M4 12h16M4 18h16",
  collapse: "M15 6l-6 6 6 6",
  expand: "M9 6l6 6-6 6",
  chevron: "M6 9l6 6 6-6",
  lock: "M7 11V8a5 5 0 0 1 10 0v3M5 11h14v9H5z",
  close: "M6 6l12 12M18 6L6 18",
  sun: "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4",
  moon: "M20 14.2A8 8 0 1 1 9.8 4a6.4 6.4 0 0 0 10.2 10.2z",
  contrast: "M12 3a9 9 0 1 0 0 18zM12 3a9 9 0 0 1 0 18",
  system: "M3 5h18v11H3zM9 20h6M12 16v4",
  panelRight: "M4 5h16v14H4zM15 5v14",
  more: "M5 11.2a.8.8 0 1 1 0 1.6a.8.8 0 1 1 0-1.6zM12 11.2a.8.8 0 1 1 0 1.6a.8.8 0 1 1 0-1.6zM19 11.2a.8.8 0 1 1 0 1.6a.8.8 0 1 1 0-1.6z",
};

export type IconName = keyof typeof paths;

export function Icon({ name, size = 16 }: { name: IconName; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d={paths[name]} />
    </svg>
  );
}
