"use client";

import Link from "next/link";
import { motion } from "framer-motion";
import { ArrowRight } from "@/components/icons";
import { easing } from "@/lib/motion";
import { usePrefersReducedMotion } from "@/lib/use-prefers-reduced-motion";
import type { Localized, Locale } from "@/lib/i18n/locales";
import { localizeHref } from "@/lib/i18n/routes";

const copy: Localized<{
  body: string;
  cta: string;
  headingLines: readonly [string, string];
}> = {
  nb: {
    headingLines: ["Har dere noe", "som bør virke bedre?"],
    body: "En første prat handler om å finne et tydelig sted å starte — ikke om å ha alle svarene klare.",
    cta: "Fortell oss om det",
  },
  en: {
    headingLines: ["Got something", "that should work better?"],
    body: "A first conversation is about finding a clear place to start — not about having every answer ready.",
    cta: "Tell us about it",
  },
};

export function ProjectStartSection({ locale }: { locale: Locale }) {
  const prefersReducedMotion = usePrefersReducedMotion();
  const content = copy[locale];

  return (
    <section
      aria-labelledby="project-start-heading"
      className="bg-[#FCFCFA] py-[var(--home-section-space)] text-[#161616]"
    >
      <div className="mx-auto grid w-[calc(100%-48px)] max-w-[1600px] gap-12 overflow-hidden rounded-lg bg-[#191B1A] px-[clamp(28px,5vw,80px)] py-[clamp(64px,9vw,136px)] text-white sm:w-[92vw] lg:grid-cols-12 lg:items-end">
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
          <h2
            className="home-display-heading max-w-[14ch] !text-white"
            id="project-start-heading"
          >
            {content.headingLines[0]}
            <br />
            {content.headingLines[1]}
          </h2>
        </motion.div>

        <motion.div
          className="grid gap-9 lg:col-span-4 lg:justify-self-end"
          initial={prefersReducedMotion ? false : { opacity: 0, y: 22 }}
          transition={{
            duration: prefersReducedMotion ? 0 : 0.7,
            delay: prefersReducedMotion ? 0 : 0.1,
            ease: easing.perfect,
          }}
          viewport={{ once: true, margin: "-10%" }}
          whileInView={{ opacity: 1, y: 0 }}
        >
          <p className="home-body m-0 max-w-[22rem] !text-white/75">
            {content.body}
          </p>
          <Link
            className="site-cta site-cta--light"
            href={localizeHref("/kontakt", locale)}
          >
            {content.cta}{" "}
            <ArrowRight aria-hidden="true" className="h-4 w-4" />
          </Link>
        </motion.div>
      </div>
    </section>
  );
}
