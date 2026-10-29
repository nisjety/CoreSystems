import Link from "next/link";
import type { Locale, Localized } from "@/lib/i18n/locales";
import { localizeHref } from "@/lib/i18n/routes";
import type { ServiceSeo } from "./service-seo";
import { serviceArticleLabels } from "./service-article-content";
import {
  serviceDeliveries,
  type ServiceDelivery,
} from "./service-delivery-data";
import { serviceDeliveriesEn } from "./service-delivery-data-en";

const deliveriesByLocale: Localized<
  Record<ServiceSeo["canonical"], ServiceDelivery>
> = { nb: serviceDeliveries, en: serviceDeliveriesEn };

export function ServiceDeliverySection({
  canonical,
  locale,
}: {
  canonical: ServiceSeo["canonical"];
  locale: Locale;
}) {
  const delivery = deliveriesByLocale[locale][canonical];
  const labels = serviceArticleLabels[locale].delivery;

  return (
    <section
      aria-labelledby="delivery-heading"
      className="border-b border-[#E1E1DD] bg-[#F7F7F4] px-[clamp(28px,5.4vw,96px)] py-[clamp(56px,8vw,112px)]"
    >
      <p className="m-0 font-label text-[0.66rem] font-light uppercase tracking-[0.16em] text-[#626260]">
        {labels.marker}
      </p>
      <div className="mt-10 grid gap-8 lg:grid-cols-2">
        <h2
          id="delivery-heading"
          className="m-0 max-w-[16ch] font-manrope text-[clamp(2.4rem,4vw,4.6rem)] font-light leading-[0.98] tracking-[-0.065em]"
        >
          {labels.heading}
        </h2>
        <p className="m-0 max-w-[34rem] self-end font-body text-[1.08rem] font-light leading-[1.58] text-[#4D4D4A]">
          {labels.intro}
        </p>
      </div>
      <div className="mt-12 grid gap-10 border-t border-[#D8D8D3] pt-8 lg:grid-cols-3">
        {delivery.deliverables.map((item) => (
          <div key={item.title}>
            <h3 className="m-0 font-manrope text-[1.55rem] font-light leading-[1.1] tracking-[-0.045em]">
              {item.title}
            </h3>
            <p className="mb-0 mt-5 font-body text-base font-light leading-[1.65] text-[#4D4D4A]">
              {item.description}
            </p>
          </div>
        ))}
      </div>
      <dl className="mb-0 mt-12 divide-y divide-[#D8D8D3] border-y border-[#D8D8D3]">
        {[
          [labels.collaboration, delivery.collaboration],
          [labels.quality, delivery.quality],
          [labels.continuation, delivery.continuation],
        ].map(([title, description]) => (
          <div
            key={title}
            className="grid gap-4 py-7 lg:grid-cols-[minmax(12rem,0.55fr)_minmax(0,1fr)] lg:gap-12"
          >
            <dt className="font-manrope text-[1.35rem] font-light tracking-[-0.04em]">
              {title}
            </dt>
            <dd className="m-0 max-w-[47rem] font-body text-base font-light leading-[1.65] text-[#4D4D4A]">
              {description}
            </dd>
          </div>
        ))}
      </dl>
      <div className="mt-10 grid max-w-[56rem] gap-4">
        {delivery.related.map((relation) => (
          <p
            key={relation.href}
            className="m-0 font-body text-base font-light leading-[1.65] text-[#4D4D4A]"
          >
            {relation.before}
            <Link
              href={localizeHref(relation.href, locale)}
              className="text-[#161616] underline decoration-[#161616]/40 underline-offset-4 hover:decoration-[#161616] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4"
            >
              {relation.label}
            </Link>
            {relation.after}
          </p>
        ))}
      </div>
    </section>
  );
}
