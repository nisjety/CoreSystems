"use client";

import { motion } from "framer-motion";
import Image from "next/image";
import type { Locale } from "@/lib/i18n/locales";
import { easing } from "@/lib/motion";
import { usePrefersReducedMotion } from "@/lib/use-prefers-reduced-motion";
import { servicesPageCopy } from "./services-page-data";

export function ServicesHeroSection({ locale }: { locale: Locale }) {
  const copy = servicesPageCopy[locale].hero;
  const reducedMotion = usePrefersReducedMotion();
  const entrance = (delay: number) => ({
    initial: reducedMotion ? false : { opacity: 0, y: 22 },
    animate: { opacity: 1, y: 0 },
    transition: {
      duration: reducedMotion ? 0 : 0.85,
      delay: reducedMotion ? 0 : delay,
      ease: easing.perfect,
    },
  });

  return (
    <section
      id="hero"
      className="grid border-b border-[#E1E1DD] bg-[#FCFCFA] pt-[76px] text-[#161616] lg:min-h-[100svh] lg:grid-cols-[minmax(0,1.05fr)_minmax(0,0.95fr)]"
      aria-labelledby="services-page-heading"
    >
      <motion.div
        {...entrance(0.04)}
        className="flex flex-col justify-between gap-16 px-[clamp(24px,4vw,64px)] pb-[clamp(40px,5vw,72px)] pt-[clamp(48px,8vh,104px)]"
      >
        <p className="m-0 font-label text-[0.7rem] font-light uppercase tracking-[0.14em] text-[#626260]">
          {copy.eyebrow}
        </p>
        <div>
          <h1
            id="services-page-heading"
            className="m-0 max-w-[11ch] font-manrope text-[clamp(3.2rem,6.4vw,7.4rem)] font-light leading-[0.9] tracking-[-0.075em]"
          >
            {copy.heading}
          </h1>
          <p className="mb-0 mt-[clamp(28px,4vh,48px)] max-w-[30rem] border-t border-[#161616] pt-6 font-body text-[clamp(1.05rem,1.3vw,1.25rem)] font-light leading-[1.45] text-[#4D4D4A]">
            {copy.body}
          </p>
        </div>
      </motion.div>

      <motion.div
        {...entrance(0.12)}
        className="relative min-h-[26rem] overflow-hidden bg-[#EDE7DF] lg:min-h-0"
      >
        <Image
          src="/images/Triodelab-weblosning.jpg"
          alt={copy.imageAlt}
          fill
          priority
          sizes="(min-width: 1024px) 48vw, 100vw"
          className="object-cover"
          style={{ objectPosition: "50% 60%" }}
        />
      </motion.div>
    </section>
  );
}
