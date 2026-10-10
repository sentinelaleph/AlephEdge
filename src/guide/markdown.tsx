import type { ReactNode } from "react";
import { Link } from "@/app/router/router";

/**
 * A deliberately small Markdown reader for the in-app guide: headings,
 * paragraphs, bullet and numbered lists, tables, "> " notes, **bold**,
 * `code` and in-app links ([text](#/path)). It builds React elements, never
 * HTML strings, so guide text cannot inject markup. External links are
 * rendered as plain text: the guide sends nobody off the app.
 */

export type Block =
  | { kind: "h1" | "h2" | "h3"; text: string; slug: string }
  | { kind: "p"; text: string }
  | { kind: "note"; text: string }
  | { kind: "ul" | "ol"; items: string[] }
  | { kind: "table"; head: string[]; rows: string[][] }
  | { kind: "img"; name: string; alt: string };

const FOLD: Record<string, string> = { ç: "c", ğ: "g", ı: "i", ö: "o", ş: "s", ü: "u", â: "a", î: "i", û: "u" };

/** URL-safe id for a heading ("Grid nasıl çalışır" → "grid-nasil-calisir"). */
export function slugify(text: string): string {
  return text
    .toLocaleLowerCase("tr")
    .replace(/[çğıöşüâîû]/g, (c) => FOLD[c] ?? c)
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

const cells = (line: string) =>
  line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((c) => c.trim());

export function parse(md: string): Block[] {
  const lines = md.replace(/\r\n?/g, "\n").split("\n");
  const out: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const t = line.trim();
    if (!t) {
      i++;
      continue;
    }
    const h = /^(#{1,3})\s+(.*?)(?:\s+\{#([a-z0-9-]+)\})?$/.exec(t);
    if (h) {
      const kind = (["h1", "h2", "h3"] as const)[h[1].length - 1];
      // "## Title {#id}" gives a section the same id in every language.
      out.push({ kind, text: h[2], slug: h[3] ?? slugify(h[2]) });
      i++;
      continue;
    }
    // "![caption](shot:name)": a bundled screenshot, resolved by the page.
    const img = /^!\[([^\]]*)\]\(shot:([a-z0-9-]+)\)$/.exec(t);
    if (img) {
      out.push({ kind: "img", alt: img[1], name: img[2] });
      i++;
      continue;
    }
    if (t.startsWith("|")) {
      const head = cells(t);
      i++;
      if (i < lines.length && /^\|?\s*:?-{2,}/.test(lines[i].trim())) i++;
      const rows: string[][] = [];
      while (i < lines.length && lines[i].trim().startsWith("|")) {
        rows.push(cells(lines[i]));
        i++;
      }
      out.push({ kind: "table", head, rows });
      continue;
    }
    if (t.startsWith(">")) {
      const parts: string[] = [];
      while (i < lines.length && lines[i].trim().startsWith(">")) {
        parts.push(lines[i].trim().replace(/^>\s?/, ""));
        i++;
      }
      out.push({ kind: "note", text: parts.join(" ") });
      continue;
    }
    if (/^[-*]\s+/.test(t) || /^\d+\.\s+/.test(t)) {
      const ordered = /^\d+\.\s+/.test(t);
      const re = ordered ? /^\d+\.\s+/ : /^[-*]\s+/;
      const items: string[] = [];
      while (i < lines.length && re.test(lines[i].trim())) {
        items.push(lines[i].trim().replace(re, ""));
        i++;
      }
      out.push({ kind: ordered ? "ol" : "ul", items });
      continue;
    }
    const parts: string[] = [];
    while (i < lines.length && lines[i].trim() && !/^(#{1,3}\s|\||>|[-*]\s|\d+\.\s|!\[)/.test(lines[i].trim())) {
      parts.push(lines[i].trim());
      i++;
    }
    out.push({ kind: "p", text: parts.join(" ") });
  }
  return out;
}

/** **bold**, `code` and [text](#/in-app) links. */
export function inline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /\*\*(.+?)\*\*|`([^`]+)`|\[([^\]]+)\]\(([^)]+)\)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let k = 0;
  while ((m = re.exec(text))) {
    if (m.index > last) out.push(text.slice(last, m.index));
    if (m[1] !== undefined) out.push(<strong key={k++}>{m[1]}</strong>);
    else if (m[2] !== undefined) out.push(<code key={k++}>{m[2]}</code>);
    else if (m[4].startsWith("#/")) {
      out.push(
        <Link key={k++} to={m[4].slice(1)} className="ae-link">
          {m[3]}
        </Link>,
      );
    } else out.push(m[3]);
    last = re.lastIndex;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

/**
 * One block as React. `images` maps a screenshot name to its bundled URL;
 * a name without an image renders nothing rather than a broken picture.
 */
export function renderBlock(
  b: Block,
  key: number,
  images?: Record<string, string>,
  onZoom?: (src: string, alt: string) => void,
): ReactNode {
  switch (b.kind) {
    case "img": {
      const src = images?.[b.name];
      return src ? (
        <figure key={key} className="ae-guide__shot">
          {onZoom ? (
            <button type="button" className="ae-guide__zoom" onClick={() => onZoom(src, b.alt)} aria-label={b.alt}>
              <img src={src} alt={b.alt} loading="lazy" />
            </button>
          ) : (
            <img src={src} alt={b.alt} loading="lazy" />
          )}
          {b.alt ? <figcaption>{b.alt}</figcaption> : null}
        </figure>
      ) : null;
    }
    case "h1":
      return null;
    case "h2":
      return (
        <h2 key={key} id={b.slug} className="ae-guide__h2">
          {inline(b.text)}
        </h2>
      );
    case "h3":
      return (
        <h3 key={key} id={b.slug} className="ae-guide__h3">
          {inline(b.text)}
        </h3>
      );
    case "p":
      return <p key={key}>{inline(b.text)}</p>;
    case "note":
      return (
        <p key={key} className="ae-guide__note">
          {inline(b.text)}
        </p>
      );
    case "ul":
    case "ol": {
      const Tag = b.kind;
      return (
        <Tag key={key} className="ae-guide__list">
          {b.items.map((it, i) => (
            <li key={i}>{inline(it)}</li>
          ))}
        </Tag>
      );
    }
    case "table":
      return (
        <div key={key} className="ae-guide__tablewrap">
          <table className="ae-guide__table">
            <thead>
              <tr>
                {b.head.map((h, i) => (
                  <th key={i}>{inline(h)}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {b.rows.map((r, i) => (
                <tr key={i}>
                  {r.map((c, j) => (
                    <td key={j}>{inline(c)}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
  }
}
