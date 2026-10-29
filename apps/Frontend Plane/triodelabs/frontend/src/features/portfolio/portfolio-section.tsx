"use client";

import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { ArrowRight } from "@/components/icons";
import type { Locale } from "@/lib/i18n/locales";
import { ownedProductsByLocale, portfolioCopy } from "./portfolio-content";
import { ProductOrbitVisual } from "./product-orbit-visual";
import { getNextOwnedProductIndex } from "./product-selection";

export function PortfolioSection({ locale }: { locale: Locale }) {
  const ownedProducts = ownedProductsByLocale[locale];
  const content = portfolioCopy[locale];
  const [activeProductIndex, setActiveProductIndex] = useState(0);
  const [isHydrated, setIsHydrated] = useState(false);
  const pendingFocusIndexRef = useRef<number | null>(null);

  useEffect(() => setIsHydrated(true), []);

  useEffect(() => {
    const pendingFocusIndex = pendingFocusIndexRef.current;

    if (pendingFocusIndex === null) {
      return;
    }

    document.getElementById(`owned-product-tab-${pendingFocusIndex}`)?.focus();
    pendingFocusIndexRef.current = null;
  }, [activeProductIndex]);

  if (ownedProducts.length === 0) {
    return null;
  }

  const activeProduct = ownedProducts[activeProductIndex] ?? ownedProducts[0];
  const otherProduct =
    ownedProducts[(activeProductIndex + 1) % ownedProducts.length] ??
    activeProduct;

  function handleProductTabKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    const nextProductIndex = getNextOwnedProductIndex(
      event.key,
      activeProductIndex,
      ownedProducts.length,
    );

    if (nextProductIndex === null) {
      return;
    }

    event.preventDefault();
    pendingFocusIndexRef.current = nextProductIndex;
    setActiveProductIndex(nextProductIndex);
  }

  return (
    <section
      id="prosjekter"
      aria-labelledby="projects-heading"
      className="overflow-x-clip bg-[#FCFCFA] py-[var(--home-section-space)] text-[#161616]"
    >
      <div>
        <header className="mx-auto grid w-[calc(100%-48px)] max-w-[1600px] gap-8 border-t border-[#D7D6D0] pt-8 sm:w-[92vw] lg:grid-cols-12 lg:items-end">
          <div className="lg:col-span-7">
            <h2 className="home-section-heading" id="projects-heading">
              {content.heading}
            </h2>
          </div>
          <p className="home-body m-0 max-w-[25rem] text-[#626260] lg:col-span-4 lg:col-start-9 lg:justify-self-end">
            {content.body}
          </p>
        </header>

        <div className="mx-auto mt-[clamp(28px,4vw,56px)] grid w-[calc(100%-48px)] max-w-[1600px] items-center gap-6 sm:w-[92vw] lg:min-h-[46rem] lg:grid-cols-[3fr_2fr] lg:gap-0">
          <ProductOrbitVisual
            locale={locale}
            otherProduct={otherProduct}
            product={activeProduct}
          />

          <div className="w-full min-w-0 px-[clamp(24px,4.4vw,72px)] pb-8 lg:py-16 lg:pl-[clamp(24px,3vw,48px)]">
            <div
              aria-label={content.tablist}
              className="flex w-fit max-w-full overflow-x-auto rounded-full bg-[#E9EDF3] p-1.5 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
              role="tablist"
            >
              {ownedProducts.map((product, index) => {
                const isActive = activeProductIndex === index;

                return (
                  <button
                    aria-controls={`owned-product-panel-${index}`}
                    aria-selected={isActive}
                    className="home-ui shrink-0 rounded-full px-6 py-3 text-[#555B66] transition-colors hover:text-[#17191D] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#17191D] aria-selected:bg-[#FFFFFF] aria-selected:text-[#17191D] aria-selected:shadow-[0_2px_8px_rgba(22,27,36,0.08)] sm:px-8"
                    id={`owned-product-tab-${index}`}
                    key={product.title}
                    disabled={!isHydrated}
                    onClick={() => setActiveProductIndex(index)}
                    onKeyDown={handleProductTabKeyDown}
                    role="tab"
                    tabIndex={isActive ? 0 : -1}
                    type="button"
                  >
                    {product.title}
                  </button>
                );
              })}
            </div>

            {ownedProducts.map((product, index) => {
              const isActive = activeProductIndex === index;

              return (
                <article
                  aria-labelledby={`owned-product-heading-${index}`}
                  className={`mt-[clamp(56px,7vw,96px)] ${isActive ? "block" : "hidden"}`}
                  hidden={!isActive}
                  id={`owned-product-panel-${index}`}
                  key={product.title}
                  role="tabpanel"
                >
                  <h3
                    className="home-product-heading"
                    id={`owned-product-heading-${index}`}
                  >
                    {product.title}
                  </h3>
                  <p className="home-body mb-0 mt-8 max-w-[26rem] text-[#626260]">
                    {product.description}
                  </p>
                  <a
                    className="site-cta site-cta--quiet mt-12"
                    href={product.href}
                    rel="noreferrer"
                    target="_blank"
                  >
                    {content.explore(product.title)}
                    <ArrowRight aria-hidden="true" className="h-3.5 w-3.5" />
                  </a>
                </article>
              );
            })}
          </div>
        </div>
      </div>
    </section>
  );
}
