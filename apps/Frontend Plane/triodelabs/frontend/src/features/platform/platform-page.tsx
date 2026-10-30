import Image from "next/image";
import Link from "next/link";
import { ArrowRight } from "@/components/icons";
import { Footer } from "@/components/layout/footer";
import { Navigation } from "@/components/layout/navigation";
import type { Locale } from "@/lib/i18n/locales";
import { localizeHref } from "@/lib/i18n/routes";
import {
  platformPageContent,
  type PlatformProduct,
} from "./platform-content";

const labelClassName =
  "m-0 font-label text-[0.72rem] uppercase tracking-[0.14em] text-[#626260]";

function ProductCard({
  locale,
  product,
  reversed,
}: {
  locale: Locale;
  product: PlatformProduct;
  reversed: boolean;
}) {
  const content = platformPageContent[locale];

  return (
    <article className="grid overflow-hidden border border-[#E1E1DD] bg-white lg:grid-cols-2">
      <a
        className={`group relative block min-h-[20rem] overflow-hidden bg-[#E9E9E5] focus-visible:outline focus-visible:outline-2 focus-visible:-outline-offset-4 focus-visible:outline-[#161616] sm:min-h-[28rem] ${
          reversed ? "lg:order-2" : ""
        }`}
        href={product.href}
        rel="noreferrer"
        target="_blank"
        tabIndex={-1}
        aria-hidden="true"
      >
        <Image
          alt={product.image.alt}
          className="object-cover transition-transform duration-700 ease-out group-hover:scale-[1.03] motion-reduce:transition-none"
          fill
          sizes="(min-width: 1024px) 50vw, 100vw"
          src={product.image.src}
        />
      </a>
      <div className="flex flex-col p-[clamp(28px,4vw,64px)] lg:min-h-[32rem]">
        <div className="flex items-start justify-between gap-6 font-label text-[0.7rem] uppercase tracking-[0.13em] text-[#626260]">
          <span>{content.productLabel(product.number)}</span>
          <span className="inline-flex items-center gap-2 text-[#161616]">
            <span className="size-2 rounded-full bg-[#91BDCA]" />
            {product.status}
          </span>
        </div>
        <div className="mt-auto pt-14">
          <h3 className="m-0 font-manrope text-[clamp(3.2rem,5.4vw,6.2rem)] font-light leading-[0.82] tracking-[-0.085em] text-[#161616]">
            {product.name}
          </h3>
          <p className="mb-0 mt-6 max-w-[26rem] font-manrope text-[clamp(1.25rem,1.7vw,1.85rem)] font-light leading-[1.08] tracking-[-0.05em] text-[#161616]">
            {product.summary}
          </p>
          <p className="mb-0 mt-6 max-w-[31rem] font-body text-[clamp(1rem,1.15vw,1.15rem)] font-light leading-[1.55] text-[#626260]">
            {product.description}
          </p>
          <a
            className="site-cta site-cta--ocean mt-9"
            href={product.href}
            rel="noreferrer"
            target="_blank"
          >
            {content.visit(product.name)}
            <ArrowRight aria-hidden="true" className="h-3.5 w-3.5" />
          </a>
        </div>
      </div>
    </article>
  );
}

export function PlatformPage({ locale }: { locale: Locale }) {
  const content = platformPageContent[locale];

  return (
    <>
      <Navigation locale={locale} variant="editorial" />
      <main
        id="main-content"
        className="bg-[#F7F7F4] pt-28 text-[#161616] sm:pt-[7.5rem]"
      >
        <section
          aria-labelledby="platform-heading"
          className="border-b border-[#E1E1DD] px-[clamp(24px,5vw,80px)] pb-[clamp(56px,8vw,128px)] pt-[clamp(48px,7vw,112px)]"
        >
          <div className="mx-auto grid max-w-[1680px] gap-12 lg:grid-cols-[minmax(0,1.15fr)_minmax(18rem,0.55fr)] lg:items-end">
            <div>
              <p className={labelClassName}>{content.hero.eyebrow}</p>
              <h1
                id="platform-heading"
                className="mb-0 mt-8 max-w-[10ch] font-manrope text-[clamp(3.8rem,8.4vw,10rem)] font-light leading-[0.8] tracking-[-0.09em] text-[#161616]"
              >
                {content.hero.heading}
              </h1>
            </div>
            <p className="m-0 max-w-[27rem] font-body text-[clamp(1.1rem,1.4vw,1.4rem)] font-light leading-[1.48] text-[#4D4D4A]">
              {content.hero.body}
            </p>
          </div>
        </section>

        <section
          aria-labelledby="platform-products-heading"
          className="px-[clamp(24px,5vw,80px)] py-[clamp(40px,5vw,80px)]"
        >
          <div className="mx-auto max-w-[1680px]">
            <h2 className="sr-only" id="platform-products-heading">
              {content.productsHeading}
            </h2>
            <div className="grid gap-[clamp(24px,3vw,48px)]">
              {content.products.map((product, index) => (
                <ProductCard
                  key={product.name}
                  locale={locale}
                  product={product}
                  reversed={index % 2 === 1}
                />
              ))}
            </div>
          </div>
        </section>

        <section
          aria-labelledby="platform-models-heading"
          className="border-t border-[#E1E1DD] px-[clamp(24px,5vw,80px)] py-[clamp(64px,9vw,144px)]"
        >
          <div className="mx-auto max-w-[1680px]">
            <div className="grid gap-10 pb-[clamp(40px,5vw,72px)] lg:grid-cols-[minmax(0,1.1fr)_minmax(18rem,0.55fr)] lg:items-end">
              <h2
                id="platform-models-heading"
                className="m-0 max-w-[11ch] font-manrope text-[clamp(2.8rem,5vw,6.2rem)] font-light leading-[0.86] tracking-[-0.08em]"
              >
                {content.models.heading}
              </h2>
              <p className="m-0 max-w-[30rem] font-body text-[clamp(1.05rem,1.25vw,1.22rem)] font-light leading-[1.55] text-[#4D4D4A]">
                {content.models.body}
              </p>
            </div>
            <div className="grid border border-[#E1E1DD] lg:grid-cols-2">
              <div className="border-b border-[#E1E1DD] bg-white p-[clamp(28px,4vw,64px)] lg:border-b-0 lg:border-r">
                <p className={labelClassName}>
                  {content.models.consulting.label}
                </p>
                <h3 className="mb-0 mt-12 max-w-[12ch] font-manrope text-[clamp(2rem,3vw,3.6rem)] font-light leading-[0.92] tracking-[-0.065em]">
                  {content.models.consulting.heading}
                </h3>
                <p className="mb-0 mt-6 max-w-[31rem] font-body text-[clamp(1rem,1.15vw,1.15rem)] font-light leading-[1.55] text-[#4D4D4A]">
                  {content.models.consulting.body}
                </p>
              </div>
              <div className="bg-[#DCECF0] p-[clamp(28px,4vw,64px)]">
                <p className={`${labelClassName} !text-[#365866]`}>
                  {content.models.subscription.label}
                </p>
                <h3 className="mb-0 mt-12 max-w-[12ch] font-manrope text-[clamp(2rem,3vw,3.6rem)] font-light leading-[0.92] tracking-[-0.065em]">
                  {content.models.subscription.heading}
                </h3>
                <p className="mb-0 mt-6 max-w-[31rem] font-body text-[clamp(1rem,1.15vw,1.15rem)] font-light leading-[1.55] text-[#365866]">
                  {content.models.subscription.body}
                </p>
              </div>
            </div>
          </div>
        </section>

        <section
          aria-labelledby="platform-cta-heading"
          className="bg-[#161616] px-[clamp(24px,5vw,80px)] py-[clamp(64px,9vw,144px)] text-white"
        >
          <div className="mx-auto grid max-w-[1680px] gap-10 lg:grid-cols-[minmax(0,1fr)_minmax(20rem,0.65fr)] lg:items-end">
            <h2
              id="platform-cta-heading"
              className="m-0 max-w-[12ch] font-manrope text-[clamp(2.8rem,5vw,6.2rem)] font-light leading-[0.86] tracking-[-0.08em]"
            >
              {content.cta.heading}
            </h2>
            <div>
              <p className="m-0 max-w-[30rem] font-body text-[clamp(1.05rem,1.25vw,1.22rem)] font-light leading-[1.55] text-white/70">
                {content.cta.body}
              </p>
              <Link
                className="site-cta site-cta--light mt-8"
                href={localizeHref("/kontakt", locale)}
              >
                {content.cta.link}
                <ArrowRight aria-hidden="true" className="h-3.5 w-3.5" />
              </Link>
            </div>
          </div>
        </section>
      </main>
      <Footer locale={locale} />
    </>
  );
}
