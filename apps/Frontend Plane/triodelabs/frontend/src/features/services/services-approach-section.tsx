import Image from "next/image";
import type { Locale } from "@/lib/i18n/locales";
import { servicesPageCopy } from "./services-page-data";
import { ServicesSectionLabel } from "./services-section-label";

export function ServicesApproachSection({ locale }: { locale: Locale }) {
  const copy = servicesPageCopy[locale].approach;

  return (
    <section
      className="grid bg-[#F0F1EE] lg:grid-cols-2"
      aria-labelledby="services-approach-heading"
    >
      <div className="relative min-h-[36rem] overflow-hidden lg:min-h-[52rem]">
        <Image
            src="/images/Triodelab-bilder/JackogAbdi.jpg"
            alt={copy.imageAlt}
          fill
          sizes="(min-width: 1024px) 50vw, 100vw"
          className="object-cover"
        />
        <p className="absolute bottom-6 left-6 max-w-[17rem] bg-[#FCFCFA]/92 px-4 py-3 font-label text-[0.7rem] leading-[1.4] text-[#444440] sm:bottom-9 sm:left-9">
          {copy.caption}
        </p>
      </div>

      <div className="flex flex-col justify-between p-[clamp(30px,6vw,88px)]">
        <div>
          <ServicesSectionLabel>{copy.label}</ServicesSectionLabel>
          <h2
            id="services-approach-heading"
            className="mt-8 max-w-[10ch] font-manrope text-[clamp(3rem,5.3vw,5.7rem)] font-light leading-[0.92] tracking-[-0.07em]"
          >
            {copy.heading}
          </h2>
          <p className="mt-7 max-w-[29rem] font-body text-[1.05rem] font-light leading-[1.5] text-[#626260]">
            {copy.body}
          </p>
        </div>

        <div className="mt-16 space-y-6">
          {copy.steps.map((step, index) => (
            <div key={step.title} className="grid grid-cols-[38px_1fr] gap-4">
              <span className="pt-1 font-label text-[0.67rem] tracking-[0.14em] text-[#8C8C87]">
                0{index + 1}
              </span>
              <div>
                <h3 className="font-manrope text-[1.35rem] font-light tracking-[-0.04em]">
                  {step.title}
                </h3>
                <p className="mt-2 max-w-[27rem] font-body text-[0.96rem] font-light leading-[1.45] text-[#656560]">
                  {step.copy}
                </p>
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
