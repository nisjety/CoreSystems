import type { Locale } from "@/lib/i18n/locales";
import { AboutReveal } from "./about-reveal";
import { aboutContent } from "./about-page-data";

export function AboutMethodSection({ locale }: { locale: Locale }) {
  const content = aboutContent[locale].method;

  return (
    <section
      className="grid gap-12 bg-[#FCFCFA] px-[clamp(20px,4vw,64px)] py-[clamp(72px,10vw,148px)] lg:grid-cols-12 lg:gap-8"
      aria-labelledby="method-heading"
    >
      <AboutReveal className="lg:col-span-5">
        <p className="mb-6 font-label text-[0.72rem] uppercase tracking-[0.16em] text-[#969692]">
          {content.eyebrow}
        </p>
        <h2
          id="method-heading"
          className="max-w-[9ch] font-manrope text-[clamp(2.9rem,5.2vw,5.5rem)] font-light leading-[0.95] tracking-[-0.07em]"
        >
          {content.heading}
        </h2>
      </AboutReveal>
      <div className="grid gap-0 lg:col-span-6 lg:col-start-7">
        {content.steps.map(([number, title, description], index) => (
          <AboutReveal key={number} delay={index * 0.08}>
            <div className="grid grid-cols-[48px_1fr] gap-5 border-t border-[#D5D5D0] py-7 sm:grid-cols-[64px_1fr]">
              <span className="font-label text-[0.72rem] tracking-[0.14em] text-[#969692]">
                {number}
              </span>
              <div className="grid gap-3 sm:grid-cols-[minmax(120px,0.6fr)_1fr] sm:gap-8">
                <h3 className="font-manrope text-[1.45rem] font-light tracking-[-0.045em]">
                  {title}
                </h3>
                <p className="m-0 max-w-[24rem] font-body text-[1rem] font-light leading-[1.42] text-[#626260]">
                  {description}
                </p>
              </div>
            </div>
          </AboutReveal>
        ))}
      </div>
    </section>
  );
}
