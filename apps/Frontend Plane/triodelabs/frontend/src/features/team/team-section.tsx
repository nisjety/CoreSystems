"use client";

import { motion } from "framer-motion";
import { SectionShell } from "@/components/layout";
import { easing } from "@/lib/motion";
import { usePrefersReducedMotion } from "@/lib/use-prefers-reduced-motion";
import type { Locale } from "@/lib/i18n/locales";
import { teamContent } from "./team-data";
import { TeamMember } from "./team-member";

export function TeamSection({ locale }: { locale: Locale }) {
  const prefersReducedMotion = usePrefersReducedMotion();
  const content = teamContent[locale];

  return (
    <section
      id="team"
      aria-labelledby="team-heading"
      className="overflow-hidden bg-[#FFFFFF] py-[clamp(104px,13vw,204px)] text-[#161616]"
    >
      <SectionShell>
        <motion.header
          initial={prefersReducedMotion ? false : { opacity: 0, y: 24 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true, margin: "-10%" }}
          transition={{
            duration: prefersReducedMotion ? 0 : 0.8,
            ease: easing.perfect,
          }}
          className="grid gap-9 border-b border-[#E6E6E2] pb-[clamp(52px,7vw,96px)] sm:grid-cols-12 sm:items-end sm:gap-x-8"
        >
          <h2
            id="team-heading"
            className="hero-title max-w-[13ch] sm:col-span-8 sm:max-w-none"
          >
            {content.headingLines[0]}
            <br />
            {content.headingLines[1]}
          </h2>
          <p className="max-w-[18rem] font-manrope text-[clamp(1.06rem,1.25vw,1.28rem)] font-light leading-[1.38] text-[#626260] sm:col-span-3 sm:col-start-10 sm:pb-1">
            {content.body}
          </p>
        </motion.header>

        <div className="mt-[clamp(54px,7vw,104px)] grid grid-cols-1 gap-x-[clamp(20px,2vw,32px)] gap-y-[clamp(68px,8vw,112px)] min-[640px]:grid-cols-2 lg:grid-cols-3">
          {content.members.map((member, index) => (
            <TeamMember
              key={member.name}
              linkedinLabel={content.linkedin(member.name)}
              member={member}
              index={index}
              reducedMotion={prefersReducedMotion}
            />
          ))}
        </div>
      </SectionShell>
    </section>
  );
}
