import Image from "next/image";
import Link from "next/link";
import { ArrowRight } from "@/components/icons";
import { Footer } from "@/components/layout/footer";
import { Navigation } from "@/components/layout/navigation";
import { JsonLd } from "@/components/seo/json-ld";
import { htmlLang, type Locale } from "@/lib/i18n/locales";
import { localizePath } from "@/lib/i18n/routes";
import { siteUrl, toAbsoluteUrl } from "@/lib/seo";
import {
  contactSourceParam,
  getContactSlugForSource,
  getContactSource,
} from "@/features/contact/contact-sources";
import {
  serviceArticleLabels,
  type ServiceArticleContent,
} from "./service-article-content";
import { ServiceDeliverySection } from "./service-delivery-section";
import { RelatedInsightSection } from "../insights/related-insight-section";

const relatedInsights = {
  "/tjenester/integrasjoner": {
    description:
      "En praktisk guide til å planlegge dataflyter mellom nettbutikk, lager, økonomi og andre systemer – med ansvar og drift i sentrum.",
    href: "/innsikt/systemintegrasjon-for-nettbutikk",
    title: "Gjør dataflyten enklere å stole på.",
  },
  "/tjenester/ki-og-automatisering": {
    description:
      "Se hvordan dere kan velge en avgrenset KI-pilot, teste kvalitet og beholde menneskelig kontroll gjennom hele arbeidsflyten.",
    href: "/innsikt/ki-pilot-for-bedrifter",
    title: "Start med riktig oppgave – ikke bare ny teknologi.",
  },
  "/tjenester/google-og-meta-ads": {
    description:
      "En fagartikkel om hvordan mennesker, KI og måling kan spille sammen i annonsering på tvers av kanaler.",
    href: "/innsikt/google-meta-tiktok-snapchat-annonsering",
    title: "Annonsering med tydeligere valg og måling.",
  },
} as const;

type ServiceArticlePageProps = ServiceArticleContent & {
  children?: React.ReactNode;
  locale: Locale;
};

function Marker({
  children,
  inverse = false,
}: {
  children: React.ReactNode;
  inverse?: boolean;
}) {
  return (
    <p
      className={`m-0 font-label text-[0.66rem] font-light uppercase tracking-[0.16em] ${
        inverse ? "text-white/65" : "text-[#626260]"
      }`}
    >
      {children}
    </p>
  );
}

function ArticleTitle({ lines }: { lines: readonly string[] }) {
  return (
    <h1 className="m-0 max-w-[12ch] font-manrope text-[clamp(3.4rem,6.5vw,7.25rem)] font-light leading-[0.82] tracking-[-0.085em] text-[#161616]">
      {lines.map((line) => (
        <span className="block" key={line}>
          {line}
        </span>
      ))}
    </h1>
  );
}

export function ServiceArticlePage({
  children,
  cta,
  editorial,
  faqs,
  hero,
  intro,
  label,
  locale,
  reflection,
  seo,
}: ServiceArticlePageProps) {
  const labels = serviceArticleLabels[locale];
  const pageUrl = toAbsoluteUrl(localizePath(seo.canonical, locale));
  const contactSource = getContactSource(seo.canonical.split("/").at(-1));
  const contactHref = `${localizePath("/kontakt", locale)}?${
    contactSourceParam[locale]
  }=${getContactSlugForSource(contactSource, locale)}`;
  const structuredData = {
    "@context": "https://schema.org",
    "@graph": [
      {
        "@type": "WebPage",
        name: seo.name,
        description: seo.description,
        url: pageUrl,
        dateModified: seo.lastUpdated,
        inLanguage: htmlLang[locale],
        isPartOf: {
          "@id": `${siteUrl}/#website`,
        },
      },
      {
        "@type": "Service",
        name: seo.name,
        description: seo.description,
        serviceType: seo.serviceType,
        url: pageUrl,
        provider: {
          "@id": `${siteUrl}/#organization`,
        },
        areaServed: "NO",
      },
      {
        "@type": "BreadcrumbList",
        itemListElement: [
          {
            "@type": "ListItem",
            position: 1,
            name: labels.breadcrumbHome,
            item: toAbsoluteUrl(localizePath("/", locale)),
          },
          {
            "@type": "ListItem",
            position: 2,
            name: labels.breadcrumbServices,
            item: toAbsoluteUrl(localizePath("/tjenester", locale)),
          },
          {
            "@type": "ListItem",
            position: 3,
            name: seo.name,
            item: pageUrl,
          },
        ],
      },
      {
        "@type": "FAQPage",
        mainEntity: faqs.map((faq) => ({
          "@type": "Question",
          name: faq.question,
          acceptedAnswer: { "@type": "Answer", text: faq.answer },
        })),
      },
    ],
  };

  return (
    <>
      <Navigation locale={locale} variant="editorial" />
      <JsonLd data={structuredData} />
      <main
        id="main-content"
        className="bg-white pb-[clamp(72px,10vw,160px)] pt-28 text-[#161616] sm:pt-[7.5rem]"
      >
        <article className="mx-auto max-w-[1680px]">
          {/* Above the 1680px article width, pull the header out to the
              viewport's left edge so the hero image bleeds to the edge. */}
          <header className="grid border-y border-[#E1E1DD] lg:grid-cols-[minmax(0,0.92fr)_minmax(0,1.08fr)] min-[1680px]:ml-[calc(50%-50vw)]">
            <div className="relative min-h-[29rem] overflow-hidden border-b border-[#E1E1DD] lg:min-h-[calc(100svh-7.5rem)] lg:border-b-0 lg:border-r">
              <Image
                alt={hero.imageAlt}
                className="object-cover"
                fill
                priority
                sizes="(max-width: 1023px) 100vw, 46vw"
                src={hero.imageSrc}
                style={
                  hero.imagePosition
                    ? { objectPosition: hero.imagePosition }
                    : undefined
                }
              />
              <div className="absolute inset-x-0 bottom-0 flex items-end justify-between gap-6 bg-gradient-to-t from-black/65 via-black/10 to-transparent px-[clamp(24px,4vw,56px)] pb-[clamp(22px,3vw,44px)] pt-28 text-white">
                <span className="font-label text-[0.66rem] font-light uppercase tracking-[0.16em]">
                  {label}
                </span>
                <span className="font-label text-[0.66rem] font-light uppercase tracking-[0.16em]">
                  {labels.location}
                </span>
              </div>
            </div>

            <div className="grid content-between gap-16 p-[clamp(28px,5.4vw,96px)] lg:min-h-[calc(100svh-7.5rem)]">
              <div className="flex items-start justify-between gap-6">
                <Marker>{labels.servicePage}</Marker>
                <Marker>{labels.updated(seo.lastUpdatedLabel)}</Marker>
              </div>
              <ArticleTitle lines={hero.title} />
              <div className="grid gap-8 border-t border-[#161616] pt-6 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end">
                <p className="m-0 max-w-[31rem] font-body text-[clamp(1.08rem,1.45vw,1.38rem)] font-light leading-[1.42] text-[#4D4D4A]">
                  {hero.summary}
                </p>
                <Link
                  className="inline-flex w-fit items-center gap-3 font-label text-[0.72rem] font-light uppercase tracking-[0.12em] transition-opacity hover:opacity-55 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#161616]"
                  href="#kontakt"
                >
                  {labels.startProject} <span aria-hidden="true">↓</span>
                </Link>
              </div>
            </div>
          </header>

          <section
            aria-labelledby="intro-heading"
            className="grid border-b border-[#E1E1DD] lg:grid-cols-12"
          >
            <aside className="p-[clamp(28px,4vw,64px)] lg:col-span-3">
              <Marker>01 / {intro.kicker}</Marker>
            </aside>
            <div className="p-[clamp(32px,5.4vw,96px)] lg:col-span-7">
              <h2
                className="m-0 max-w-[13ch] font-manrope text-[clamp(2.65rem,4.45vw,5.4rem)] font-light leading-[0.89] tracking-[-0.07em]"
                id="intro-heading"
              >
                {intro.title}
              </h2>
              <div className="mt-[clamp(40px,7vw,104px)] grid max-w-[39rem] gap-7 lg:ml-[14%]">
                {intro.body.map((paragraph) => (
                  <p
                    className="m-0 font-body text-[clamp(1.06rem,1.28vw,1.25rem)] font-light leading-[1.58] text-[#4D4D4A]"
                    key={paragraph}
                  >
                    {paragraph}
                  </p>
                ))}
              </div>
            </div>
            <div className="flex items-end bg-[#F7F7F4] p-[clamp(32px,4vw,64px)] lg:col-span-2">
              <p className="m-0 max-w-[14rem] border-l border-[#161616] pl-5 font-manrope text-[clamp(1.15rem,1.55vw,1.65rem)] font-light leading-[1.1] tracking-[-0.05em]">
                {editorial.introAside}
              </p>
            </div>
          </section>

          <section
            aria-labelledby="narrative-heading"
            className="grid border-b border-[#E1E1DD] lg:grid-cols-12"
          >
            <div className="p-[clamp(28px,4vw,64px)] lg:col-span-3">
              <Marker>{labels.narrativeMarker}</Marker>
            </div>
            <div className="grid gap-[clamp(48px,8vw,144px)] p-[clamp(32px,5.4vw,96px)] lg:col-span-9 lg:grid-cols-[minmax(0,0.95fr)_minmax(16rem,0.7fr)]">
              <h2
                className="m-0 max-w-[11ch] font-manrope text-[clamp(2.65rem,4.45vw,5.4rem)] font-light leading-[0.89] tracking-[-0.07em]"
                id="narrative-heading"
              >
                {editorial.narrativeTitle}
              </h2>
              <div className="grid content-end gap-7">
                {editorial.narrative.map((paragraph) => (
                  <p
                    className="m-0 max-w-[32rem] font-body text-[clamp(1.06rem,1.28vw,1.25rem)] font-light leading-[1.58] text-[#4D4D4A]"
                    key={paragraph}
                  >
                    {paragraph}
                  </p>
                ))}
              </div>
            </div>
          </section>

          <ServiceDeliverySection canonical={seo.canonical} locale={locale} />
          {children}
          {locale === "nb" &&
          relatedInsights[seo.canonical as keyof typeof relatedInsights] ? (
            <RelatedInsightSection
              {...relatedInsights[seo.canonical as keyof typeof relatedInsights]}
            />
          ) : null}

          <section
            aria-labelledby="reflection-heading"
            data-nav-theme="light"
            className="bg-[#161616] px-[clamp(32px,5.4vw,96px)] py-[clamp(56px,9vw,156px)] text-white"
          >
            <div className="grid gap-12 lg:grid-cols-[minmax(0,1.15fr)_minmax(16rem,0.7fr)]">
              <div>
                <Marker inverse>04 / {reflection.label}</Marker>
                <h2
                  className="mb-0 mt-[clamp(64px,10vw,156px)] max-w-[10ch] font-manrope text-[clamp(2.75rem,4.6vw,5.8rem)] font-light leading-[0.88] tracking-[-0.075em]"
                  id="reflection-heading"
                >
                  {editorial.reflectionTitle}
                </h2>
              </div>
              <div className="grid content-end gap-7">
                {reflection.body.map((paragraph) => (
                  <p
                    className="m-0 max-w-[31rem] font-body text-[clamp(1.06rem,1.28vw,1.25rem)] font-light leading-[1.58] text-white/70"
                    key={paragraph}
                  >
                    {paragraph}
                  </p>
                ))}
              </div>
            </div>
          </section>

          <section
            aria-labelledby="faq-heading"
            className="grid border-b border-[#E1E1DD] lg:grid-cols-12"
          >
            <div className="p-[clamp(28px,4vw,64px)] lg:col-span-3">
              <Marker>{labels.faqMarker}</Marker>
            </div>
            <div className="p-[clamp(32px,5.4vw,96px)] lg:col-span-9">
              <h2
                className="m-0 max-w-[12ch] font-manrope text-[clamp(2.65rem,4.45vw,5.4rem)] font-light leading-[0.89] tracking-[-0.07em]"
                id="faq-heading"
              >
                {editorial.faqTitle}
              </h2>
              <div className="mt-[clamp(44px,7vw,104px)] divide-y divide-[#E1E1DD] border-y border-[#161616]">
                {faqs.map((faq) => (
                  <details className="group py-7" key={faq.question}>
                    <summary className="flex cursor-pointer list-none items-start justify-between gap-8 font-manrope text-[clamp(1.35rem,2vw,2.25rem)] font-light leading-[0.98] tracking-[-0.055em] marker:content-none">
                      <span>{faq.question}</span>
                      <span
                        aria-hidden="true"
                        className="shrink-0 text-[1.35rem] leading-none group-open:rotate-45"
                      >
                        +
                      </span>
                    </summary>
                    <p className="mb-1 mt-6 max-w-[38rem] font-body text-[clamp(1.04rem,1.22vw,1.18rem)] font-light leading-[1.58] text-[#4D4D4A]">
                      {faq.answer}
                    </p>
                  </details>
                ))}
              </div>
            </div>
          </section>

          <footer
            className="grid gap-10 px-[clamp(32px,5.4vw,96px)] py-[clamp(56px,8vw,144px)] lg:grid-cols-[minmax(0,1.2fr)_minmax(16rem,0.8fr)]"
            id="kontakt"
          >
            <h2 className="m-0 max-w-[11ch] font-manrope text-[clamp(2.8rem,4.8vw,6rem)] font-light leading-[0.88] tracking-[-0.075em]">
              {cta.title}
            </h2>
            <div className="grid content-end gap-8">
              <p className="m-0 max-w-[29rem] font-body text-[clamp(1.05rem,1.25vw,1.24rem)] font-light leading-[1.54] text-[#4D4D4A]">
                {cta.body}
              </p>
              <Link
                className="inline-flex w-fit items-center gap-3 border-b border-[#161616] pb-2 font-label text-[0.74rem] font-light uppercase tracking-[0.12em] transition-opacity hover:opacity-55 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#161616]"
                href={contactHref}
              >
                {labels.contact}{" "}
                <ArrowRight aria-hidden="true" className="h-3.5 w-3.5" />
              </Link>
            </div>
          </footer>
        </article>
      </main>
      <Footer locale={locale} />
    </>
  );
}
