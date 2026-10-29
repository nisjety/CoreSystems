"use client";

import Link from "next/link";
import { motion } from "framer-motion";
import { ArrowRight } from "@/components/icons";
import { easing } from "@/lib/motion";
import { usePrefersReducedMotion } from "@/lib/use-prefers-reduced-motion";
import { serviceHighlights } from "./portfolio-content";

export function ClientWorkSection() {
  const prefersReducedMotion = usePrefersReducedMotion();

  return (
    <section
      aria-labelledby="client-work-heading"
      className="bg-[#F5F7FB] px-[clamp(24px,4.4vw,72px)] py-[clamp(88px,11vw,160px)] text-[#161616]"
      id="kundearbeid"
    >
      <div className="mx-auto grid max-w-[1600px] gap-12 lg:grid-cols-12 lg:items-end">
        <motion.div
          className="lg:col-span-8"
          initial={prefersReducedMotion ? false : { opacity: 0, y: 26 }}
          transition={{
            duration: prefersReducedMotion ? 0 : 0.75,
            ease: easing.perfect,
          }}
          viewport={{ once: true, margin: "-10%" }}
          whileInView={{ opacity: 1, y: 0 }}
        >
          <p className="mb-7 font-label text-[0.72rem] font-light uppercase tracking-[0.14em] text-[#626260]">
            04 / Hva vi bygger
          </p>
          <h2
            className="hero-title max-w-[13ch] font-manrope"
            id="client-work-heading"
          >
            Bygget for å
            <br />
            bli brukt.
          </h2>
        </motion.div>

        <motion.p
          className="m-0 max-w-[23rem] font-manrope text-[clamp(1.05rem,1.28vw,1.28rem)] font-light leading-[1.4] text-[#626260] lg:col-span-4 lg:justify-self-end"
          initial={prefersReducedMotion ? false : { opacity: 0, y: 22 }}
          transition={{
            duration: prefersReducedMotion ? 0 : 0.7,
            delay: prefersReducedMotion ? 0 : 0.1,
            ease: easing.perfect,
          }}
          viewport={{ once: true, margin: "-10%" }}
          whileInView={{ opacity: 1, y: 0 }}
        >
          Fra innhold og handel til digitale produkter: vi lager løsninger som
          fungerer godt nå, og som er enkle å utvikle videre.
        </motion.p>
      </div>

      <motion.div
        className="mx-auto mt-[clamp(64px,8vw,116px)] grid max-w-[1600px] border-l border-t border-[#E6E6E2] sm:grid-cols-2 lg:grid-cols-3"
        initial={prefersReducedMotion ? false : { opacity: 0 }}
        transition={{
          duration: prefersReducedMotion ? 0 : 0.6,
          delay: prefersReducedMotion ? 0 : 0.16,
        }}
        viewport={{ once: true, margin: "-10%" }}
        whileInView={{ opacity: 1 }}
      >
        {serviceHighlights.map((highlight) => {
          const content = (
            <>
              <div className="flex items-start justify-between gap-5 font-label text-[0.7rem] font-light uppercase tracking-[0.1em] text-[#969692]">
                <span>{highlight.number}</span>
                <span>{highlight.category}</span>
              </div>
              <div>
                <h3 className="m-0 font-manrope text-[clamp(1.55rem,2.45vw,2.8rem)] font-light leading-[0.9] tracking-[-0.055em]">
                  {highlight.title}
                </h3>
                <p className="mb-0 mt-4 max-w-[22rem] font-manrope text-[0.98rem] font-light leading-[1.4] tracking-[-0.025em] text-[#626260]">
                  {highlight.description}
                </p>
                {highlight.action ? (
                  <span className="mt-5 inline-flex items-center gap-1.5 font-body text-[0.9rem] font-light text-[#161616] transition-transform duration-300 group-hover:translate-x-1">
                    {highlight.action}
                    <ArrowRight aria-hidden="true" className="h-3.5 w-3.5" />
                  </span>
                ) : null}
              </div>
            </>
          );

          const className =
            "group grid min-h-[18rem] content-between border-b border-r border-[#E6E6E2] p-[clamp(22px,2.2vw,34px)] transition-colors hover:bg-[#F7F7F4] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#161616]";

          return highlight.href ? (
            <Link
              aria-label={highlight.action}
              className={className}
              href={highlight.href}
              key={highlight.title}
            >
              {content}
            </Link>
          ) : (
            <article className={className} key={highlight.title}>
              {content}
            </article>
          );
        })}
      </motion.div>
    </section>
  );
}
