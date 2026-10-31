import Image from "next/image";
import Link from "next/link";
import { ArrowRight } from "@/components/icons";
import { Footer } from "@/components/layout/footer";
import { Navigation } from "@/components/layout/navigation";
import { JsonLd } from "@/components/seo/json-ld";
import { siteUrl, toAbsoluteUrl } from "@/lib/seo";
import { insights } from "./insights-content";

const insightsCanonical = "/innsikt";

export function InsightsPage() {
  const structuredData = {
    "@context": "https://schema.org",
    "@type": "CollectionPage",
    name: "Innsikt fra TriodeLab",
    description:
      "Praktiske guider om digitale arbeidsflyter, nettbutikk og teknologi.",
    url: toAbsoluteUrl(insightsCanonical),
    inLanguage: "nb-NO",
    isPartOf: { "@id": `${siteUrl}/#website` },
    publisher: { "@id": `${siteUrl}/#organization` },
    mainEntity: {
      "@type": "ItemList",
      itemListElement: insights.map((insight, index) => ({
        "@type": "ListItem",
        position: index + 1,
        url: toAbsoluteUrl(insight.canonical),
        name: insight.title,
      })),
    },
  };

  return (
    <>
      <Navigation locale="nb" variant="editorial" />
      <JsonLd data={structuredData} />
      <main
        className="bg-[#FCFCFA] pt-28 text-[#161616] sm:pt-[7.5rem]"
        id="main-content"
      >
        <section className="border-y border-[#E1E1DD] px-[clamp(24px,5vw,80px)] py-[clamp(72px,11vw,164px)]">
          <p className="m-0 font-label text-[0.68rem] uppercase tracking-[0.16em] text-[#626260]">
            Innsikt / Fagblogg
          </p>
          <div className="mt-10 grid gap-10 lg:grid-cols-12 lg:items-end">
            <h1 className="max-w-[10ch] font-manrope text-[clamp(3.8rem,8vw,9.2rem)] font-light leading-[0.82] tracking-[-0.09em] lg:col-span-8">
              Tydelige valg før dere bygger videre.
            </h1>
            <p className="m-0 max-w-[28rem] font-body text-[clamp(1.05rem,1.35vw,1.28rem)] font-light leading-[1.5] text-[#626260] lg:col-span-3 lg:col-start-10">
              Praktiske guider og fagartikler for virksomheter som vil gjøre
              teknologi, arbeidsflyter og videreutvikling enklere å styre.
            </p>
          </div>
        </section>

        <section
          aria-label="Guider og fagartikler"
          className="px-[clamp(24px,5vw,80px)] py-[clamp(72px,10vw,160px)]"
        >
          <div className="grid gap-5 lg:grid-cols-12">
            <p className="m-0 font-label text-[0.68rem] uppercase tracking-[0.16em] text-[#626260] lg:col-span-2">
              Guider og fagartikler
            </p>
            <div className="grid gap-6 lg:col-span-9 lg:col-start-4">
              {insights.map((insight, index) => (
                <article
                  className="group grid overflow-hidden border border-[#E1E1DD] bg-white transition-colors hover:bg-[#F7F7F4] lg:grid-cols-[minmax(15rem,0.88fr)_minmax(0,1.12fr)]"
                  key={insight.slug}
                >
                  <div className="relative aspect-[4/3] overflow-hidden lg:aspect-auto lg:min-h-[24rem]">
                    <Image
                      alt={insight.hero.alt}
                      className="object-cover transition-transform duration-500 group-hover:scale-[1.025]"
                      fill
                      sizes="(max-width: 1023px) 100vw, 42vw"
                      src={insight.hero.image}
                    />
                  </div>
                  <div className="flex min-h-full flex-col p-[clamp(24px,4vw,56px)]">
                    <div className="flex items-center justify-between gap-4 font-label text-[0.66rem] uppercase tracking-[0.14em] text-[#626260]">
                      <span>{String(index + 1).padStart(2, "0")}</span>
                      <span>{insight.readingTime}</span>
                    </div>
                    <p className="mb-0 mt-12 font-label text-[0.66rem] uppercase tracking-[0.14em] text-[#626260]">
                      {insight.contentType} · {insight.label}
                    </p>
                    <h2 className="mb-0 mt-4 max-w-[15ch] font-manrope text-[clamp(2rem,3.4vw,4rem)] font-light leading-[0.9] tracking-[-0.07em]">
                      {insight.title}
                    </h2>
                    <p className="mb-0 mt-6 max-w-[34rem] font-body text-[1rem] font-light leading-[1.55] text-[#626260]">
                      {insight.description}
                    </p>
                    <Link
                      className="mt-10 inline-flex w-fit items-center gap-3 font-label text-[0.7rem] uppercase tracking-[0.12em] transition-opacity hover:opacity-55 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#161616]"
                      href={insight.canonical}
                    >
                      Les {insight.contentType.toLowerCase()}
                      <ArrowRight aria-hidden="true" className="h-4 w-4" />
                    </Link>
                  </div>
                </article>
              ))}
            </div>
          </div>
        </section>
      </main>
      <Footer locale="nb" />
    </>
  );
}
