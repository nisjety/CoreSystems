import Image from "next/image";
import Link from "next/link";
import { ArrowRight } from "@/components/icons";
import type { Locale } from "@/lib/i18n/locales";
import { localizeHref } from "@/lib/i18n/routes";
import type { ServiceStory } from "./services-page-data";
import {
  serviceStoriesByLocale,
  servicesPageCopy,
} from "./services-page-data";
import { ServicesSectionLabel } from "./services-section-label";

function ServiceCard({
  locale,
  service,
}: {
  locale: Locale;
  service: ServiceStory;
}) {
  const content = (
    <>
      <div className="relative aspect-[16/10] overflow-hidden bg-[#E4E4DF]">
        <Image
          alt={service.imageAlt}
          className="object-cover transition-transform duration-700 ease-out group-hover:scale-[1.03] motion-reduce:transition-none"
          fill
          sizes="(min-width: 1024px) 40vw, (min-width: 640px) 50vw, 100vw"
          src={service.image}
          style={
            service.imagePosition
              ? { objectPosition: service.imagePosition }
              : undefined
          }
        />
      </div>
      <div className="flex flex-1 flex-col p-[clamp(24px,2.6vw,38px)]">
        <div className="flex items-start justify-between gap-6">
          <span className="font-label text-[0.68rem] tracking-[0.15em] text-[#969690]">
            {service.number}
          </span>
          <ArrowRight
            aria-hidden="true"
            className="h-4 w-4 transition-transform duration-300 group-hover:translate-x-1 motion-reduce:transition-none"
          />
        </div>
        <h3 className="mt-4 font-manrope text-[clamp(1.8rem,2.4vw,2.6rem)] font-light leading-none tracking-[-0.055em]">
          {service.title}
        </h3>
        <p className="mb-0 mt-4 max-w-[31rem] font-body text-[0.98rem] font-light leading-[1.48] text-[#656560]">
          {service.description}
        </p>
        {service.href ? (
          <span className="mt-auto inline-flex w-fit items-center gap-2 border-b border-[#161616]/30 pb-1 pt-7 font-label text-[0.7rem] font-light uppercase tracking-[0.1em] text-[#161616] transition-colors group-hover:border-[#161616]">
            {servicesPageCopy[locale].overview.readAbout(service.title)}
          </span>
        ) : null}
      </div>
    </>
  );
  const className =
    "group flex h-full flex-col overflow-hidden bg-[#F1F2EF] transition-colors hover:bg-[#ECEDE9]";

  return service.href ? (
    <Link
      className={`${className} focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#161616]`}
      href={localizeHref(service.href, locale)}
    >
      {content}
    </Link>
  ) : (
    <article className={className}>{content}</article>
  );
}

export function ServicesOverviewSection({ locale }: { locale: Locale }) {
  const copy = servicesPageCopy[locale].overview;
  const serviceStories = serviceStoriesByLocale[locale];

  return (
    <section
      className="px-[clamp(24px,5vw,80px)] py-[clamp(92px,12vw,176px)]"
      aria-labelledby="services-intro-heading"
    >
      <div className="grid gap-10 lg:grid-cols-12">
        <div className="lg:col-span-2">
          <ServicesSectionLabel>{copy.label}</ServicesSectionLabel>
        </div>
        <h2
          id="services-intro-heading"
          className="max-w-[12ch] font-manrope text-[clamp(2.8rem,5.1vw,5.6rem)] font-light leading-[0.94] tracking-[-0.07em] lg:col-span-5"
        >
          {copy.heading}
        </h2>
        <div className="lg:col-span-4 lg:col-start-9">
          <p className="font-body text-[clamp(1.05rem,1.25vw,1.2rem)] font-light leading-[1.5] text-[#626260]">
            {copy.body}
          </p>
          <Link
            className="mt-6 inline-flex items-center gap-2 border-b border-[#161616]/30 pb-1 font-body text-sm text-[#161616] transition-colors hover:border-[#161616] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#161616]"
            href={localizeHref(
              "/tjenester/radgivning-og-konsulentbistand",
              locale,
            )}
          >
            {copy.advisoryLink}
            <ArrowRight aria-hidden="true" className="h-3.5 w-3.5" />
          </Link>
        </div>
      </div>

      <div className="mt-[clamp(76px,10vw,144px)] grid gap-3 lg:grid-cols-12">
        <div className="pb-5 lg:col-span-2">
          <ServicesSectionLabel>{copy.servicesLabel}</ServicesSectionLabel>
        </div>
        <div className="grid gap-3 sm:grid-cols-2 lg:col-span-10">
          {serviceStories.map((service, index) => (
            <div
              key={service.number}
              className={
                serviceStories.length % 2 !== 0 &&
                index === serviceStories.length - 1
                  ? "h-full sm:col-span-2"
                  : "h-full"
              }
            >
              <ServiceCard locale={locale} service={service} />
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
