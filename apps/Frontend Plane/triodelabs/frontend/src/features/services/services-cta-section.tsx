import { ArrowRight } from "@/components/icons";
import type { Locale } from "@/lib/i18n/locales";
import { localizeHref } from "@/lib/i18n/routes";
import { servicesPageCopy } from "./services-page-data";

export function ServicesCtaSection({ locale }: { locale: Locale }) {
  const copy = servicesPageCopy[locale].cta;

  return (
    <section
      id="kontakt"
      className="bg-[#171817] px-[clamp(24px,5vw,80px)] py-[clamp(90px,12vw,170px)] text-white"
      aria-labelledby="services-cta-heading"
    >
      <div className="grid gap-10 lg:grid-cols-12 lg:items-end">
        <div className="lg:col-span-8">
          <p className="mb-7 font-label text-[0.68rem] uppercase tracking-[0.16em] text-white/52">
            {copy.label}
          </p>
          <h2
            id="services-cta-heading"
            className="max-w-[10ch] font-manrope text-[clamp(3.4rem,7vw,7.6rem)] font-light leading-[0.88] tracking-[-0.078em]"
          >
            {copy.heading}
          </h2>
        </div>
        <div className="lg:col-span-3 lg:col-start-10">
          <p className="font-body text-[1.05rem] font-light leading-[1.45] text-white/70">
            {copy.body}
          </p>
          <a
            href={localizeHref("/kontakt", locale)}
            className="mt-8 inline-flex items-center gap-3 font-label text-[0.72rem] uppercase tracking-[0.12em] transition-colors hover:text-white/70 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-white"
          >
            {copy.button} <ArrowRight aria-hidden="true" className="h-3.5 w-3.5" />
          </a>
        </div>
      </div>
    </section>
  );
}
