"use client";

import Image from "next/image";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { usePrefersReducedMotion } from "@/lib/use-prefers-reduced-motion";
import type { Localized, Locale } from "@/lib/i18n/locales";
import {
  siClaude,
  siDocker,
  siFigma,
  siFramer,
  siGooglecloud,
  siKubernetes,
  siNextdotjs,
  siReact,
  siSanity,
  siVercel,
  siWordpress,
  type SimpleIcon,
} from "simple-icons";
import { Globe, Settings } from "@/components/icons";

type Technology = {
  name: string;
  discipline: Localized<string>;
  logo: ReactNode;
};

const copy: Localized<{
  heading: string;
  pause: string;
  resume: string;
  tagline: string;
}> = {
  nb: {
    heading: "Teknologier",
    tagline: "Vi jobber med teknologien som funker deg best.",
    pause: "Pause bevegelse",
    resume: "Fortsett bevegelse",
  },
  en: {
    heading: "Technologies",
    tagline: "We work with the technology that suits you best.",
    pause: "Pause motion",
    resume: "Resume motion",
  },
};

const carouselCopies = 3;
const carouselSpeed = 0.035;

function getCarouselLoopWidth(carousel: HTMLDivElement) {
  return carousel.scrollWidth / carouselCopies;
}

function wrapCarouselPosition(carousel: HTMLDivElement) {
  const loopWidth = getCarouselLoopWidth(carousel);
  if (!loopWidth) return;

  if (carousel.scrollLeft >= loopWidth * 2) {
    carousel.scrollLeft -= loopWidth;
  } else if (carousel.scrollLeft <= 0) {
    carousel.scrollLeft += loopWidth;
  }
}

function SimpleMark({ icon }: { icon: SimpleIcon }) {
  return (
    <svg aria-hidden="true" className="size-full" viewBox="0 0 24 24">
      <path d={icon.path} fill={`#${icon.hex}`} />
    </svg>
  );
}

function SvgMark({ alt, src }: { alt: string; src: string }) {
  return (
    <Image
      alt={alt}
      className="size-full object-contain object-left"
      height="96"
      src={src}
      sizes="(max-width: 760px) 74px, 144px"
      width="144"
    />
  );
}

function BuildMark() {
  return (
    <div className="grid h-full grid-cols-2 place-items-center gap-3">
      <span
        aria-label="Frontend"
        role="img"
        className="block size-[clamp(1.65rem,2.35vw,2.75rem)]"
      >
        <Globe aria-hidden="true" className="size-full" strokeWidth={1.4} />
      </span>
      <span
        aria-label="Backend"
        role="img"
        className="block size-[clamp(1.65rem,2.35vw,2.75rem)]"
      >
        <Settings aria-hidden="true" className="size-full" strokeWidth={1.4} />
      </span>
    </div>
  );
}

function LlmMark() {
  return (
    <svg aria-hidden="true" className="size-full" viewBox="0 0 24 24">
      <path
        d="M12 2.7 21 19H3L12 2.7Z"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.7"
      />
      <path
        d="M8.2 14.8V9.7h1.3v4h2.2v1.1H8.2Zm4.5 0V9.7H14v4h2.2v1.1h-3.5Z"
        fill="currentColor"
      />
    </svg>
  );
}

function AiMark() {
  return (
    <div className="grid h-full scale-[1.18] grid-cols-3 items-center gap-2">
      <span className="block aspect-square text-[#d97757]">
        <SimpleMark icon={siClaude} />
      </span>
      <span className="block aspect-square text-[#161616]">
        <SvgMark
          alt="ChatGPT"
          src="https://cdn.jsdelivr.net/npm/@lobehub/icons-static-svg@latest/icons/openai.svg"
        />
      </span>
      <span className="block aspect-square text-[#161616]">
        <LlmMark />
      </span>
    </div>
  );
}

const technologies: Technology[] = [
  {
    name: "React",
    discipline: { nb: "Grensesnitt", en: "Interface" },
    logo: <SimpleMark icon={siReact} />,
  },
  {
    name: "Next.js",
    discipline: { nb: "Webplattform", en: "Web platform" },
    logo: <SimpleMark icon={siNextdotjs} />,
  },
  {
    name: "Framer",
    discipline: { nb: "Interaksjon", en: "Interaction" },
    logo: <SimpleMark icon={siFramer} />,
  },
  {
    name: "WordPress",
    discipline: { nb: "Innhold", en: "Content" },
    logo: <SimpleMark icon={siWordpress} />,
  },
  {
    name: "Figma",
    discipline: { nb: "Design", en: "Design" },
    logo: <SimpleMark icon={siFigma} />,
  },
  {
    name: "Vercel",
    discipline: { nb: "Leveranse", en: "Delivery" },
    logo: <SimpleMark icon={siVercel} />,
  },
  {
    name: "AWS",
    discipline: { nb: "Skyplattform", en: "Cloud platform" },
    logo: (
      <SvgMark
        alt="AWS"
        src="https://cdn.jsdelivr.net/gh/devicons/devicon@latest/icons/amazonwebservices/amazonwebservices-original-wordmark.svg"
      />
    ),
  },
  {
    name: "Microsoft Azure",
    discipline: { nb: "Skyplattform", en: "Cloud platform" },
    logo: (
      <SvgMark
        alt="Microsoft Azure"
        src="https://cdn.jsdelivr.net/gh/devicons/devicon@latest/icons/azure/azure-original.svg"
      />
    ),
  },
  {
    name: "Google Cloud Console",
    discipline: { nb: "Skyplattform", en: "Cloud platform" },
    logo: <SimpleMark icon={siGooglecloud} />,
  },
  {
    name: "Docker",
    discipline: { nb: "Containere", en: "Containers" },
    logo: <SimpleMark icon={siDocker} />,
  },
  {
    name: "Kubernetes",
    discipline: { nb: "Orkestrering", en: "Orchestration" },
    logo: <SimpleMark icon={siKubernetes} />,
  },
  {
    name: "Sanity",
    discipline: { nb: "Innhold", en: "Content" },
    logo: <SimpleMark icon={siSanity} />,
  },
  {
    name: "SuperOffice",
    discipline: { nb: "CRM", en: "CRM" },
    logo: (
      <SvgMark
        alt="SuperOffice"
        src="https://upload.wikimedia.org/wikipedia/commons/5/5a/Superoffice-logo.png"
      />
    ),
  },
  {
    name: "Visma",
    discipline: { nb: "ERP", en: "ERP" },
    logo: (
      <SvgMark
        alt="Visma"
        src="https://mb.cision.com/Public/21961/0/89fb0a05eb922892_org.png"
      />
    ),
  },
  {
    name: "Frontend / backend",
    discipline: { nb: "Utvikling", en: "Development" },
    logo: <BuildMark />,
  },
  {
    name: "LLM / AI",
    discipline: { nb: "Intelligens", en: "Intelligence" },
    logo: <AiMark />,
  },
];

export function TechnologiesSection({ locale }: { locale: Locale }) {
  const content = copy[locale];
  const prefersReducedMotion = usePrefersReducedMotion();
  const carouselRef = useRef<HTMLDivElement>(null);
  const hasPositionedCarousel = useRef(false);
  const [isInteractionPaused, setIsInteractionPaused] = useState(false);
  const [isManuallyPaused, setIsManuallyPaused] = useState(false);

  useEffect(() => {
    const carousel = carouselRef.current;
    if (!carousel) return;

    if (!hasPositionedCarousel.current) {
      carousel.scrollLeft = getCarouselLoopWidth(carousel);
      hasPositionedCarousel.current = true;
    }

    if (prefersReducedMotion || isInteractionPaused || isManuallyPaused) return;

    let animationFrame = 0;
    let previousTime = performance.now();

    const advanceCarousel = (time: number) => {
      const elapsed = Math.min(time - previousTime, 48);
      previousTime = time;
      carousel.scrollLeft += elapsed * carouselSpeed;
      wrapCarouselPosition(carousel);
      animationFrame = window.requestAnimationFrame(advanceCarousel);
    };

    animationFrame = window.requestAnimationFrame(advanceCarousel);

    return () => window.cancelAnimationFrame(animationFrame);
  }, [isInteractionPaused, isManuallyPaused, prefersReducedMotion]);

  return (
    <section
      aria-labelledby="technologies-heading"
      id="teknologier"
      className="overflow-hidden bg-[#FCFCFA] text-[#161616]"
    >
      <h2 className="sr-only" id="technologies-heading">
        {content.heading}
      </h2>
      <div className="mx-auto w-[calc(100%-48px)] max-w-[1600px] py-[var(--home-section-space-compact)] sm:w-[92vw]">
        {!prefersReducedMotion && (
          <div className="mb-3 flex justify-end">
            <button
              aria-pressed={isManuallyPaused}
              className="inline-flex min-h-11 items-center border border-[#CAD7DB] bg-white px-4 py-2 font-label text-sm text-[#153845] transition-colors hover:bg-[#DCECF0] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#153845]"
              onClick={() => setIsManuallyPaused((paused) => !paused)}
              type="button"
            >
              {isManuallyPaused ? content.resume : content.pause}
            </button>
          </div>
        )}
        <div className="flex overflow-hidden border-y border-[#E6E6E2]">
          <p className="flex w-[clamp(9.5rem,17vw,18rem)] shrink-0 items-center border-r border-[#E6E6E2] px-[clamp(16px,2vw,32px)] font-label text-[clamp(1.05rem,1.35vw,1.35rem)] font-semibold uppercase leading-[1.18] tracking-[0.04em] text-[#5C5C58]">
            {content.tagline}
          </p>
          <div
            ref={carouselRef}
            aria-label={content.heading}
            className="flex min-w-0 flex-1 overflow-x-auto overscroll-x-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
            onBlur={(event) => {
              if (!event.currentTarget.contains(event.relatedTarget)) {
                setIsInteractionPaused(false);
              }
            }}
            onFocus={() => setIsInteractionPaused(true)}
            onMouseEnter={() => setIsInteractionPaused(true)}
            onMouseLeave={() => setIsInteractionPaused(false)}
            onScroll={(event) => wrapCarouselPosition(event.currentTarget)}
            onTouchEnd={() => setIsInteractionPaused(false)}
            onTouchStart={() => setIsInteractionPaused(true)}
          >
            {Array.from({ length: carouselCopies }, (_, copyIndex) =>
              technologies.map((technology) => (
                <article
                  aria-hidden={copyIndex !== 1 ? true : undefined}
                  key={`${technology.name}-${copyIndex}`}
                  className="group relative grid min-h-[clamp(190px,19vw,290px)] w-[min(78vw,360px)] shrink-0 snap-start content-between overflow-hidden border-r border-[#E6E6E2] p-[clamp(20px,2.4vw,38px)] transition-colors duration-300 hover:bg-white/70"
                >
                  <div className="absolute left-[clamp(20px,2.4vw,38px)] top-1/2 h-[clamp(38px,4.6vw,70px)] w-[clamp(74px,9vw,142px)] -translate-y-1/2">
                    {technology.logo}
                  </div>
                  <div className="absolute bottom-[clamp(20px,2.4vw,38px)] left-[clamp(20px,2.4vw,38px)] right-[clamp(20px,2.4vw,38px)] flex items-end justify-between gap-3">
                    <h3 className="font-manrope text-[clamp(1.1rem,1.4vw,1.45rem)] font-light leading-none tracking-[-0.04em]">
                      {technology.name}
                    </h3>
                    <span className="hidden font-label text-[0.65rem] font-light uppercase tracking-[0.1em] text-[#626260] sm:block">
                      {technology.discipline[locale]}
                    </span>
                  </div>
                </article>
              )),
            )}
          </div>
        </div>
      </div>
    </section>
  );
}
