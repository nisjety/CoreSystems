import Image from "next/image";
import Link from "next/link";
import { ArrowRight } from "@/components/icons";
import { Footer } from "@/components/layout/footer";
import { Navigation } from "@/components/layout/navigation";
import { JsonLd } from "@/components/seo/json-ld";
import { siteUrl, toAbsoluteUrl } from "@/lib/seo";
import type { Insight } from "./insights-content";

function Marker({ children }: { children: React.ReactNode }) {
  return (
    <p className="m-0 font-label text-[0.66rem] uppercase tracking-[0.16em] text-[#626260]">
      {children}
    </p>
  );
}

export function InsightArticlePage({ insight }: { insight: Insight }) {
  const structuredData = {
    "@context": "https://schema.org",
    "@graph": [
      {
        "@type": "Article",
        headline: insight.title,
        description: insight.description,
        image: [toAbsoluteUrl(insight.hero.image)],
        datePublished: insight.publishedAt,
        dateModified: insight.lastUpdated,
        inLanguage: "nb-NO",
        mainEntityOfPage: {
          "@type": "WebPage",
          "@id": toAbsoluteUrl(insight.canonical),
        },
        author: { "@id": `${siteUrl}/#organization` },
        publisher: { "@id": `${siteUrl}/#organization` },
      },
      {
        "@type": "BreadcrumbList",
        itemListElement: [
          { "@type": "ListItem", position: 1, name: "Hjem", item: siteUrl },
          {
            "@type": "ListItem",
            position: 2,
            name: "Innsikt",
            item: toAbsoluteUrl("/innsikt"),
          },
          {
            "@type": "ListItem",
            position: 3,
            name: insight.title,
            item: toAbsoluteUrl(insight.canonical),
          },
        ],
      },
    ],
  };

  return (
    <>
      <Navigation locale="nb" variant="editorial" />
      <JsonLd data={structuredData} />
      <main
        className="bg-white pb-[clamp(72px,10vw,160px)] pt-28 text-[#161616] sm:pt-[7.5rem]"
        id="main-content"
      >
        <article className="mx-auto max-w-[1680px]">
          <header className="grid border-y border-[#E1E1DD] lg:grid-cols-[minmax(0,0.92fr)_minmax(0,1.08fr)]">
            <div className="relative min-h-[29rem] overflow-hidden border-b border-[#E1E1DD] lg:min-h-[calc(100svh-7.5rem)] lg:border-b-0 lg:border-r">
              <Image
                alt={insight.hero.alt}
                className="object-cover"
                fill
                priority
                sizes="(max-width: 1023px) 100vw, 46vw"
                src={insight.hero.image}
              />
              <div className="absolute inset-x-0 bottom-0 flex items-end justify-between gap-6 bg-gradient-to-t from-black/65 via-black/10 to-transparent px-[clamp(24px,4vw,56px)] pb-[clamp(22px,3vw,44px)] pt-28 text-white">
                <span className="font-label text-[0.66rem] uppercase tracking-[0.16em]">
                  Innsikt / {insight.label}
                </span>
                <span className="font-label text-[0.66rem] uppercase tracking-[0.16em]">
                  Oslo / Norge
                </span>
              </div>
            </div>
            <div className="grid content-between gap-16 p-[clamp(28px,5.4vw,96px)] lg:min-h-[calc(100svh-7.5rem)]">
              <div className="flex items-start justify-between gap-6">
                <Marker>{insight.readingTime}</Marker>
                <Marker>Oppdatert {insight.lastUpdatedLabel}</Marker>
              </div>
              <h1 className="m-0 max-w-[12ch] font-manrope text-[clamp(3.2rem,5.8vw,6.5rem)] font-light leading-[0.84] tracking-[-0.085em]">
                {insight.heading}
              </h1>
              <p className="m-0 max-w-[34rem] border-t border-[#161616] pt-6 font-body text-[clamp(1.08rem,1.45vw,1.38rem)] font-light leading-[1.42] text-[#4D4D4A]">
                {insight.summary}
              </p>
            </div>
          </header>

          <section
            aria-labelledby="intro-heading"
            className="grid border-b border-[#E1E1DD] lg:grid-cols-12"
          >
            <aside className="p-[clamp(28px,4vw,64px)] lg:col-span-3">
              <Marker>01 / Utgangspunktet</Marker>
            </aside>
            <div className="p-[clamp(32px,5.4vw,96px)] lg:col-span-7">
              <h2
                className="m-0 max-w-[12ch] font-manrope text-[clamp(2.6rem,4.4vw,5.25rem)] font-light leading-[0.9] tracking-[-0.07em]"
                id="intro-heading"
              >
                Start med det som må fungere bedre.
              </h2>
              <div className="mt-[clamp(40px,7vw,104px)] grid max-w-[40rem] gap-7 lg:ml-[14%]">
                {insight.intro.map((paragraph) => (
                  <p
                    className="m-0 font-body text-[clamp(1.06rem,1.28vw,1.25rem)] font-light leading-[1.58] text-[#4D4D4A]"
                    key={paragraph}
                  >
                    {paragraph}
                  </p>
                ))}
              </div>
            </div>
          </section>

          <section
            aria-labelledby="trend-heading"
            className="grid border-b border-[#E1E1DD] bg-[#F7F7F4] lg:grid-cols-12"
          >
            <aside className="p-[clamp(28px,4vw,64px)] lg:col-span-3">
              <Marker>02 / Aktuelt nå</Marker>
            </aside>
            <div className="p-[clamp(32px,5.4vw,96px)] lg:col-span-7">
              <h2
                className="m-0 max-w-[13ch] font-manrope text-[clamp(2.15rem,3.8vw,4.4rem)] font-light leading-[0.91] tracking-[-0.065em]"
                id="trend-heading"
              >
                {insight.trend.title}
              </h2>
              <p className="mb-0 mt-10 max-w-[43rem] font-body text-[clamp(1.03rem,1.2vw,1.18rem)] font-light leading-[1.6] text-[#4D4D4A] lg:ml-[14%]">
                {insight.trend.body}
              </p>
              <a
                className="ml-0 mt-8 inline-flex font-label text-[0.66rem] uppercase tracking-[0.13em] text-[#161616] underline decoration-[#161616]/35 underline-offset-4 transition-opacity hover:opacity-60 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#161616] lg:ml-[14%]"
                href={insight.trend.sourceHref}
              >
                {insight.trend.sourceKind}: {insight.trend.sourceLabel}
              </a>
              {insight.trend.additionalSources ? (
                <div className="ml-0 mt-3 grid gap-3 lg:ml-[14%]">
                  {insight.trend.additionalSources.map((source) => (
                    <a
                      className="inline-flex w-fit font-label text-[0.66rem] uppercase tracking-[0.13em] text-[#161616] underline decoration-[#161616]/35 underline-offset-4 transition-opacity hover:opacity-60 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#161616]"
                      href={source.href}
                      key={source.href}
                    >
                      Kilde: {source.label}
                    </a>
                  ))}
                </div>
              ) : null}
            </div>
          </section>

          {insight.sections.map((section, index) => (
            <section
              aria-labelledby={`section-${index + 3}`}
              className="grid border-b border-[#E1E1DD] lg:grid-cols-12"
              key={section.title}
            >
              <aside className="p-[clamp(28px,4vw,64px)] lg:col-span-3">
                <Marker>
                  {String(index + 3).padStart(2, "0")} / I praksis
                </Marker>
              </aside>
              <div className="p-[clamp(32px,5.4vw,96px)] lg:col-span-7">
                <h2
                  className="m-0 max-w-[13ch] font-manrope text-[clamp(2.15rem,3.8vw,4.4rem)] font-light leading-[0.91] tracking-[-0.065em]"
                  id={`section-${index + 3}`}
                >
                  {section.title}
                </h2>
                <div className="mt-10 grid max-w-[43rem] gap-7 lg:ml-[14%]">
                  {section.body.map((paragraph) => (
                    <p
                      className="m-0 font-body text-[clamp(1.03rem,1.2vw,1.18rem)] font-light leading-[1.6] text-[#4D4D4A]"
                      key={paragraph}
                    >
                      {paragraph}
                    </p>
                  ))}
                </div>
              </div>
            </section>
          ))}

          <section
            aria-labelledby="checklist-heading"
            className="grid bg-[#F7F7F4] lg:grid-cols-12"
          >
            <div className="p-[clamp(28px,4vw,64px)] lg:col-span-3">
              <Marker>Kontrollpunkter</Marker>
            </div>
            <div className="p-[clamp(32px,5.4vw,96px)] lg:col-span-7">
              <h2
                className="m-0 max-w-[12ch] font-manrope text-[clamp(2.65rem,4.45vw,5.4rem)] font-light leading-[0.89] tracking-[-0.07em]"
                id="checklist-heading"
              >
                En sjekkliste før dere går videre.
              </h2>
              <ol className="mt-12 grid list-none gap-0 p-0">
                {insight.checklist.map((item, index) => (
                  <li
                    className="grid grid-cols-[3rem_1fr] gap-4 border-t border-[#161616]/20 py-5 font-body text-[clamp(1rem,1.22vw,1.18rem)] font-light leading-[1.48] text-[#343432]"
                    key={item}
                  >
                    <span className="font-label text-[0.68rem] tracking-[0.1em] text-[#626260]">
                      {String(index + 1).padStart(2, "0")}
                    </span>
                    <span>{item}</span>
                  </li>
                ))}
              </ol>
            </div>
          </section>

          <section
            aria-labelledby="faq-heading"
            className="grid border-b border-[#E1E1DD] lg:grid-cols-12"
          >
            <aside className="p-[clamp(28px,4vw,64px)] lg:col-span-3">
              <Marker>Spørsmål og svar</Marker>
            </aside>
            <div className="p-[clamp(32px,5.4vw,96px)] lg:col-span-7">
              <h2
                className="m-0 max-w-[12ch] font-manrope text-[clamp(2.65rem,4.45vw,5.4rem)] font-light leading-[0.89] tracking-[-0.07em]"
                id="faq-heading"
              >
                Vanlige spørsmål før dere går videre.
              </h2>
              <div className="mt-12 max-w-[48rem]">
                {insight.faqs.map((faq) => (
                  <details
                    className="group border-t border-[#161616]/20 py-5 last:border-b"
                    key={faq.question}
                  >
                    <summary className="flex cursor-pointer list-none items-start justify-between gap-8 font-manrope text-[clamp(1.25rem,1.7vw,1.75rem)] font-light leading-[1.08] tracking-[-0.045em] marker:hidden">
                      {faq.question}
                      <span
                        aria-hidden="true"
                        className="mt-0.5 text-xl leading-none transition-transform duration-200 group-open:rotate-45"
                      >
                        +
                      </span>
                    </summary>
                    <p className="mb-0 mt-5 max-w-[42rem] font-body text-[1.03rem] font-light leading-[1.58] text-[#626260]">
                      {faq.answer}
                    </p>
                  </details>
                ))}
              </div>
            </div>
          </section>

          <section
            aria-labelledby="related-heading"
            className="px-[clamp(24px,5vw,80px)] py-[clamp(72px,10vw,160px)]"
          >
            <div className="grid gap-10 lg:grid-cols-12">
              <div className="lg:col-span-3">
                <Marker>Videre</Marker>
                <h2
                  className="mb-0 mt-5 max-w-[8ch] font-manrope text-[clamp(2.25rem,4vw,4.8rem)] font-light leading-[0.9] tracking-[-0.07em]"
                  id="related-heading"
                >
                  Gå videre med riktig spørsmål.
                </h2>
              </div>
              <div className="grid gap-3 lg:col-span-8 lg:col-start-5">
                {insight.relatedLinks.map((link) => (
                  <Link
                    className="group grid gap-5 border-t border-[#161616]/25 py-5 sm:grid-cols-[minmax(11rem,0.65fr)_minmax(0,1fr)_auto] sm:items-center"
                    href={link.href}
                    key={link.href}
                  >
                    <span className="font-manrope text-[clamp(1.35rem,1.7vw,1.8rem)] font-light tracking-[-0.05em]">
                      {link.label}
                    </span>
                    <span className="font-body text-[0.98rem] font-light leading-[1.45] text-[#626260]">
                      {link.description}
                    </span>
                    <ArrowRight
                      aria-hidden="true"
                      className="h-5 w-5 transition-transform group-hover:translate-x-1"
                    />
                  </Link>
                ))}
              </div>
            </div>
          </section>

          <section className="bg-[#161616] px-[clamp(32px,5.4vw,96px)] py-[clamp(56px,9vw,156px)] text-white">
            <div className="grid gap-12 lg:grid-cols-[minmax(0,1.15fr)_minmax(16rem,0.7fr)]">
              <div>
                <p className="m-0 font-label text-[0.66rem] uppercase tracking-[0.16em] text-white/65">
                  Neste steg
                </p>
                <h2 className="mb-0 mt-8 max-w-[10ch] font-manrope text-[clamp(3.1rem,6vw,6.8rem)] font-light leading-[0.86] tracking-[-0.08em]">
                  Usikker på hvor dere skal begynne?
                </h2>
              </div>
              <div className="self-end">
                <p className="m-0 max-w-[27rem] font-body text-[1.05rem] font-light leading-[1.52] text-white/70">
                  En kort kartlegging av dagens arbeidsflyt gir ofte bedre svar
                  enn å starte med en ferdig teknologiløsning.
                </p>
                <Link
                  className="mt-8 inline-flex items-center gap-3 font-label text-[0.72rem] uppercase tracking-[0.12em] transition-opacity hover:opacity-65 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-white"
                  href="/kontakt"
                >
                  Start en samtale
                  <ArrowRight aria-hidden="true" className="h-4 w-4" />
                </Link>
              </div>
            </div>
          </section>
        </article>
      </main>
      <Footer locale="nb" />
    </>
  );
}
