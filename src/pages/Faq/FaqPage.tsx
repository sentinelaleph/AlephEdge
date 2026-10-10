import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryParam } from "@/app/router/router";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import en from "@/faq/en.md?raw";
import tr from "@/faq/tr.md?raw";
import { parse, renderBlock } from "@/guide/markdown";
import "@/pages/Guide/GuidePage.css";

/** FAQ text per language; languages without one read the English text. */
const CONTENT: Record<string, string> = { tr, en };

/** Screenshots per language: src/faq/shots/<lang>/<name>.webp. */
const SHOT_FILES = import.meta.glob("@/faq/shots/*/*.webp", { eager: true, query: "?url", import: "default" }) as Record<
  string,
  string
>;
const SHOTS: Record<string, Record<string, string>> = {};
for (const [path, url] of Object.entries(SHOT_FILES)) {
  const m = /shots\/([^/]+)\/([^/]+)\.webp$/.exec(path);
  if (m) (SHOTS[m[1]] ??= {})[m[2]] = url;
}

/**
 * #/faq: questions and answers per sidebar menu, with screenshots of each
 * screen (sample data, labelled as such). `?s=<slug>` opens one section.
 */
export function FaqPage() {
  const { t, i18n } = useTranslation();
  const lang = (i18n.resolvedLanguage ?? "en").split("-")[0];
  const textLang = lang in CONTENT ? lang : "en";
  const blocks = useMemo(() => parse(CONTENT[textLang]), [textLang]);
  const images = SHOTS[textLang] ?? SHOTS.en ?? {};
  const sections = blocks.flatMap((b) => (b.kind === "h2" ? [{ slug: b.slug, text: b.text }] : []));
  const [section, setSection] = useQueryParam("s");
  const [zoom, setZoom] = useState<{ src: string; alt: string } | null>(null);

  useEffect(() => {
    if (!zoom) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setZoom(null);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [zoom]);

  useEffect(() => {
    if (!section) return;
    document.getElementById(section)?.scrollIntoView({ block: "start" });
  }, [section, blocks]);

  const toc = (
    <nav aria-label={t("guide.contents")} className="ae-guide__toc">
      <p className="ae-guide__toctitle">{t("guide.contents")}</p>
      <ol>
        {sections.map((s) => (
          <li key={s.slug}>
            <a
              href={`#/faq?s=${s.slug}`}
              aria-current={section === s.slug ? "true" : undefined}
              onClick={(e) => {
                e.preventDefault();
                setSection(s.slug);
                document.getElementById(s.slug)?.scrollIntoView({ block: "start" });
              }}
            >
              {s.text}
            </a>
          </li>
        ))}
      </ol>
      {!(lang in CONTENT) ? <p className="ae-subtle">{t("guide.englishOnly")}</p> : null}
    </nav>
  );

  const title = blocks.flatMap((b) => (b.kind === "h1" ? [b.text] : []))[0];
  return (
    <PageShell title={title ?? t("nav.faq")} crumbs={[{ label: t("nav.groups.research") }]} left={toc}>
      <article className="ae-guide ae-guide--wide">
        {blocks.map((b, i) => renderBlock(b, i, images, (src, alt) => setZoom({ src, alt })))}
      </article>
      {zoom ? (
        <div className="ae-guide__lightbox" role="dialog" aria-modal="true" aria-label={zoom.alt} onClick={() => setZoom(null)}>
          <img src={zoom.src} alt={zoom.alt} />
          <p>{zoom.alt}</p>
        </div>
      ) : null}
    </PageShell>
  );
}
