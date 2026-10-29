"use client";

import Image from "next/image";
import { motion } from "framer-motion";
import { Linkedin } from "@/components/icons";
import type { TeamMemberData } from "./team-data";
import { easing } from "@/lib/motion";

interface TeamMemberProps {
  linkedinLabel: string;
  member: TeamMemberData;
  index: number;
  reducedMotion: boolean | null;
}

export function TeamMember({
  linkedinLabel,
  member,
  index,
  reducedMotion,
}: TeamMemberProps) {
  return (
    <motion.article
      initial={reducedMotion ? false : { opacity: 0, y: 28 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: "-8%" }}
      transition={{
        duration: reducedMotion ? 0 : 0.7,
        delay: reducedMotion ? 0 : index * 0.1,
        ease: easing.perfect,
      }}
    >
      <div className="group relative aspect-[4/5] overflow-hidden bg-[#F2F1ED]">
        <Image
          src={member.image}
          alt={member.imageAlt}
          fill
          sizes="(min-width: 1024px) 30vw, (min-width: 640px) 44vw, 100vw"
          className="object-cover transition-transform duration-700 ease-[cubic-bezier(0.16,1,0.3,1)] group-hover:scale-[1.018] motion-reduce:transition-none"
          style={{ objectPosition: member.imagePosition }}
        />
      </div>

      <div className="mt-5 border-t border-[#E6E6E2] pt-4">
        <div className="flex items-start justify-between gap-4">
          <div>
            <p className="mb-2 font-label text-[0.63rem] font-light uppercase tracking-[0.16em] text-[#969692]">
              {String(index + 1).padStart(2, "0")}
            </p>
            <h3 className="font-manrope text-[clamp(1.45rem,2vw,2rem)] font-light leading-[1.05] tracking-[-0.04em] text-[#161616]">
              {member.name}
            </h3>
          </div>

          {member.linkedin ? (
            <a
              href={member.linkedin}
              target="_blank"
              rel="noreferrer"
              className="mt-1 inline-grid h-8 w-8 shrink-0 place-items-center text-[#626260] transition-colors hover:text-[#161616] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#161616]"
            >
              <Linkedin aria-hidden="true" className="h-4 w-4 stroke-[1.5]" />
              <span className="sr-only">{linkedinLabel}</span>
            </a>
          ) : null}
        </div>

        <p className="mt-3 font-manrope text-[0.9rem] font-light leading-[1.38] text-[#626260]">
          {member.role}
        </p>
        <p className="mt-4 font-label text-[0.65rem] font-light uppercase tracking-[0.12em] text-[#626260]">
          {member.focusAreas.join(" · ")}
        </p>
      </div>
    </motion.article>
  );
}
