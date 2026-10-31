import { ArrowRight } from "@/components/icons";
import type { Locale } from "@/lib/i18n/locales";
import { localizeHref } from "@/lib/i18n/routes";
import { aboutContent } from "./about-page-data";
import { AboutReveal } from "./about-reveal";

export function AboutCtaSection({ locale }: { locale: Locale }) {
  const content = aboutContent[locale].cta;

  return (
    <section
      id="kontakt"
      className="grid gap-10 bg-[#191918] px-[clamp(20px,4vw,64px)] py-[clamp(72px,10vw,136px)] text-[#FCFCFA] sm:grid-cols-12 sm:items-end"
      aria-labelledby="about-cta-heading"
    >
      <AboutReveal className="sm:col-span-8">
        <p className="mb-6 font-label text-[0.72rem] uppercase tracking-[0.16em] text-white/50">
          {content.eyebrow}
        </p>
        <h2
          id="about-cta-heading"
          className="max-w-[12ch] font-manrope text-[clamp(3rem,6.4vw,7rem)] font-light leading-[0.92] tracking-[-0.075em]"
        >
          {content.headingLines[0]}
          <br />
          {content.headingLines[1]}
        </h2>
      </AboutReveal>
      <AboutReveal className="sm:col-span-4 sm:justify-self-end" delay={0.1}>
        <p className="mb-7 max-w-[18rem] font-body text-[clamp(1.04rem,1.25vw,1.2rem)] font-light leading-[1.4] text-white/70">
          {content.body}
        </p>
        <a
          href={localizeHref("/kontakt", locale)}
          className="inline-flex items-center gap-3 border-b border-white/55 pb-2 font-label text-[0.78rem] uppercase tracking-[0.12em] text-white transition-colors hover:border-white hover:text-white/70 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-white"
        >
          {content.link} <ArrowRight aria-hidden="true" className="h-3.5 w-3.5" />
        </a>
      </AboutReveal>
    </section>
  );
}
