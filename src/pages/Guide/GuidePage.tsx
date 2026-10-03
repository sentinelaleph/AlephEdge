import { useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useQueryParam } from "@/app/router/router";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { parse, renderBlock } from "@/guide/markdown";
import en from "@/guide/content/en.md?raw";
import tr from "@/guide/content/tr.md?raw";
import "./GuidePage.css";

/** Guide text per language; languages without one read the English text. */
const CONTENT: Record<string, string> = { tr, en };

/**
 * #/guide: the DCA and Grid handbook, in the app. Sections are the "##"
 * headings; `?s=<slug>` opens one (links from forms and presets land there).
 */
export function GuidePage() {
  const { t, i18n } = useTranslation();
  const lang = (i18n.resolvedLanguage ?? "en").split("-")[0];
  const blocks = useMemo(() => parse(CONTENT[lang] ?? CONTENT.en), [lang]);
  const sections = blocks.flatMap((b) => (b.kind === "h2" ? [{ slug: b.slug, text: b.text }] : []));
  const [section, setSection] = useQueryParam("s");

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
              href={`#/guide?s=${s.slug}`}
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
    <PageShell title={title ?? t("nav.guide")} crumbs={[{ label: t("nav.groups.research") }]} left={toc}>
      <article className="ae-guide">{blocks.map(renderBlock)}</article>
    </PageShell>
  );
}
