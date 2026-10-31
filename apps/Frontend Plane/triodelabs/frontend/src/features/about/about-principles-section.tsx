import type { Locale } from "@/lib/i18n/locales";
import { AboutReveal } from "./about-reveal";
import { aboutContent } from "./about-page-data";

export function AboutPrinciplesSection({ locale }: { locale: Locale }) {
  const content = aboutContent[locale].principles;

  return (
    <section
      className="bg-[#FCFCFA] px-[clamp(20px,4vw,64px)] py-[clamp(72px,10vw,156px)]"
      aria-labelledby="principles-heading"
    >
      <div className="grid gap-12 lg:grid-cols-12 lg:gap-8">
        <AboutReveal className="lg:col-span-7">
          <p className="mb-6 font-label text-[0.72rem] uppercase tracking-[0.16em] text-[#969692]">
            {content.eyebrow}
          </p>
          <h2
            id="principles-heading"
            className="max-w-[14ch] font-manrope text-[clamp(2.7rem,5.2vw,5.6rem)] font-light leading-[0.96] tracking-[-0.065em]"
          >
            {content.headingLines[0]}
            <br />
            {content.headingLines[1]}
          </h2>
        </AboutReveal>
        <AboutReveal
          className="max-w-[23rem] self-end lg:col-span-4 lg:col-start-9"
          delay={0.1}
        >
          <p className="font-body text-[clamp(1.05rem,1.35vw,1.3rem)] font-light leading-[1.42] text-[#626260]">
            {content.body}
          </p>
        </AboutReveal>
      </div>

      <div className="mt-[clamp(52px,7vw,96px)] grid gap-3 md:grid-cols-3">
        {content.items.map((principle, index) => (
          <AboutReveal key={principle.number} delay={index * 0.08}>
            <article className="flex min-h-[15rem] flex-col justify-between border border-[#E6E6E2] bg-[#F7F7F4] p-[clamp(24px,2.6vw,40px)] transition-colors duration-500 hover:bg-[#F2F1ED]">
              <span className="font-label text-[0.72rem] tracking-[0.14em] text-[#969692]">
                {principle.number}
              </span>
              <div>
                <h3 className="mb-3 font-manrope text-[clamp(1.55rem,2vw,2.1rem)] font-light tracking-[-0.045em]">
                  {principle.title}
                </h3>
                <p className="m-0 max-w-[18rem] font-body text-[1rem] font-light leading-[1.4] text-[#626260]">
                  {principle.description}
                </p>
              </div>
            </article>
          </AboutReveal>
        ))}
      </div>
    </section>
  );
}
