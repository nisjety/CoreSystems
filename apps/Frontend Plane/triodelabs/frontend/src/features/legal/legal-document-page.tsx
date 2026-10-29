import { Footer } from "@/components/layout/footer";
import { Navigation } from "@/components/layout/navigation";
import { CookiePreferencesButton } from "./cookie-preferences-button";
import { LegalDisclosureLinks } from "./legal-disclosure-links";
import type { Locale, Localized } from "@/lib/i18n/locales";
import type { LegalDocument } from "./legal-content";

const labels: Localized<{ legal: string; updated: string }> = {
  nb: { legal: "Juridisk", updated: "Oppdatert" },
  en: { legal: "Legal", updated: "Updated" },
};

export function LegalDocumentPage({
  document,
  locale,
  showCookiePreferences = false,
}: {
  document: LegalDocument;
  locale: Locale;
  showCookiePreferences?: boolean;
}) {
  return (
    <>
      <Navigation locale={locale} variant="editorial" />
      <main
        id="main-content"
        className="bg-[#FCFCFA] px-6 pb-24 pt-36 text-[#161616] sm:px-12 sm:pt-44 lg:px-16"
      >
        <article className="mx-auto max-w-[1280px]">
          <header className="border-b border-[#D9D9D4] pb-12 sm:pb-16">
            <div className="mb-10 flex flex-wrap justify-between gap-4 font-label text-[0.7rem] uppercase tracking-[0.14em] text-[#626260]">
              <span>{labels[locale].legal}</span>
              <span>
                {labels[locale].updated}{" "}
                <time dateTime={document.updatedAt}>
                  {document.updatedLabel}
                </time>
              </span>
            </div>
            <h1 className="m-0 break-words font-manrope text-[clamp(2.5rem,6vw,6.5rem)] font-light leading-[1.02] tracking-[-0.07em]">
              {document.title}
            </h1>
            <p className="mb-0 mt-8 max-w-[760px] font-body text-[clamp(1.1rem,1.5vw,1.35rem)] font-light leading-relaxed text-[#4D4D4A]">
              {document.introduction}
            </p>
            {document.notice && (
              <p className="mb-0 mt-6 max-w-[760px] border-l border-[#161616] pl-4 font-body text-sm font-light leading-relaxed text-[#626260]">
                {document.notice}
              </p>
            )}
          </header>
          <div className="grid gap-12 pt-12 lg:grid-cols-[minmax(0,1fr)_minmax(0,3fr)] lg:gap-20">
            {/* Links to the other legal documents live in the footer. */}
            <div aria-hidden="true" className="max-lg:hidden" />
            <div>
              {document.sections.map((section, index) => (
                <section
                  aria-labelledby={`legal-section-${index + 1}`}
                  className="mb-10 border-b border-[#D9D9D4] pb-10 last:mb-0"
                  key={section.title}
                >
                  <p
                    className="mb-4 font-label text-xs tracking-[0.14em] text-[#626260]"
                    aria-hidden="true"
                  >
                    {String(index + 1).padStart(2, "0")}
                  </p>
                  <h2
                    id={`legal-section-${index + 1}`}
                    className="mb-5 font-manrope text-[clamp(1.5rem,2.2vw,2rem)] font-light leading-tight tracking-[-0.04em]"
                  >
                    {section.title}
                  </h2>
                  {section.paragraphs.map((paragraph) => (
                    <p
                      className="mb-4 font-body text-base font-light leading-[1.75] text-[#4D4D4A] last:mb-0 sm:text-lg"
                      key={paragraph}
                    >
                      {paragraph}
                    </p>
                  ))}
                  {section.items && (
                    <ul className="mt-4 list-disc space-y-2 pl-5 font-body text-base font-light leading-[1.75] text-[#4D4D4A] sm:text-lg">
                      {section.items.map((item) => (
                        <li key={item}>{item}</li>
                      ))}
                    </ul>
                  )}
                  {section.links && <LegalDisclosureLinks links={section.links} />}
                </section>
              ))}
            </div>
          </div>
          {showCookiePreferences && <CookiePreferencesButton locale={locale} />}
        </article>
      </main>
      <Footer locale={locale} />
    </>
  );
}
