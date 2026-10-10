// @vitest-environment jsdom
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import en from "./content/en.md?raw";
import tr from "./content/tr.md?raw";
import faqEn from "@/faq/en.md?raw";
import faqTr from "@/faq/tr.md?raw";
import { inline, parse, renderBlock, slugify } from "./markdown";

vi.mock("@/app/router/router", () => ({
  Link: ({ to, children }: { to: string; children: React.ReactNode }) => <a href={`#${to}`}>{children}</a>,
}));

describe("guide markdown", () => {
  it("reads headings with fixed ids, tables, lists and notes", () => {
    const b = parse("# Title\n\n## Start {#start}\n\nText **bold**.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n- one\n- two\n\n1. first\n\n> note line");
    expect(b.map((x) => x.kind)).toEqual(["h1", "h2", "p", "table", "ul", "ol", "note"]);
    expect(b[1]).toMatchObject({ text: "Start", slug: "start" });
    expect(b[3]).toMatchObject({ head: ["A", "B"], rows: [["1", "2"]] });
  });

  it("reads screenshot lines as images and renders only known names", () => {
    const b = parse(["Text before", "![Caption](shot:dca-detail)", "Text after"].join("\n"));
    expect(b.map((x) => x.kind)).toEqual(["p", "img", "p"]);
    expect(b[1]).toMatchObject({ name: "dca-detail", alt: "Caption" });
    expect(renderToStaticMarkup(<>{renderBlock(b[1], 0, { "dca-detail": "/x.webp" })}</>)).toContain('src="/x.webp"');
    expect(renderToStaticMarkup(<>{renderBlock(b[1], 0, {})}</>)).toBe("");
  });

  it("FAQ sections match in both languages and every screenshot exists", () => {
    const ids = (md: string) => parse(md).flatMap((x) => (x.kind === "h2" ? [x.slug] : []));
    expect(ids(faqTr)).toEqual(ids(faqEn));
    const files = Object.keys(import.meta.glob("@/faq/shots/*/*.webp"));
    for (const [lang, md] of [["tr", faqTr], ["en", faqEn]] as const) {
      for (const x of parse(md)) {
        if (x.kind === "img") expect(files.some((f) => f.endsWith(`/shots/${lang}/${x.name}.webp`)), `${lang}/${x.name}`).toBe(true);
      }
    }
  });

  it("folds Turkish letters in generated slugs", () => {
    expect(slugify("Grid nasıl çalışır")).toBe("grid-nasil-calisir");
  });

  it("links only inside the app and never renders HTML from the text", () => {
    const html = renderToStaticMarkup(<p>{inline("[Presets](#/presets) [site](https://example.com) <script>x</script>")}</p>);
    expect(html).toContain('href="#/presets"');
    expect(html).not.toContain("example.com");
    expect(html).not.toContain("<script>");
  });

  it("gives both languages the same section ids", () => {
    const ids = (md: string) => parse(md).flatMap((x) => (x.kind === "h2" ? [x.slug] : []));
    expect(ids(tr)).toEqual(ids(en));
    expect(ids(tr)).toContain("dca-settings");
    expect(ids(tr)).toContain("grid-settings");
    expect(ids(tr)).toContain("which-bot");
  });
});
