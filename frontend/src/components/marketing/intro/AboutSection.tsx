"use client";

import * as React from "react";
import Image from "next/image";
import {
  motion,
  useScroll,
  useTransform,
  type MotionValue,
} from "motion/react";

type Chapter = {
  id: string;
  surtile: string;
  titleLines: string[];
  text: string;
  highlight: string;
  imagePosition: "left" | "right";
  images: [string, string, string];
  tech: string;
};

const chapters: Chapter[] = [
  {
    id: "intro",
    surtile: "Om Oss",
    titleLines: ["Strategisk", "Rådgivning", "& Teknisk", "Implementering"],
    text: "Vi hjelper bedrifter med digital transformasjon gjennom strategisk rådgivning og teknisk implementering. Basert i Oslo, grunnlagt 2025. Vårt mål er å være den pålitelige partneren som guider deg gjennom kompleksiteten.",
    highlight: "Digital Transformasjon",
    imagePosition: "right",
    images: [
      "/imagens/arched-interior-modern.png",
      "/imagens/curved-concrete-space.png",
      "/imagens/arched-hallway-symmetry.jpeg",
    ],
    tech: "Oslo, 2025, Digital Partner",
  },
  {
    id: "abdifatah",
    surtile: "Frontend & UX",
    titleLines: ["Abdifatah", "Hassan"],
    text: "Ferdig utdannet frontend- og mobilutvikler med ekspertise innen design og utvikling av nettsider og apper. Har hovedansvar for frontend-delen og brukeropplevelse, med fokus på estetikk og funksjon.",
    highlight: "Brukeropplevelse",
    imagePosition: "left",
    images: [
      "/imagens/curved-interior-sculpture.png",
      "/imagens/arched-corridor-1.jpeg",
      "/imagens/arched-interior-modern.png",
    ],
    tech: "React, Next.js, TypeScript",
  },
  {
    id: "ima",
    surtile: "Backend Dev",
    titleLines: ["Ima", "Da Costa"],
    text: "Ferdig utdannet programmerer med solid teknisk forståelse. Programmerer strukturert med vekt på skalerbare løsninger. Har hovedansvar for backend-mikrotjenester, Docker-oppsett og arkitektur.",
    highlight: "Backend-arkitektur",
    imagePosition: "right",
    images: [
      "/imagens/curved-concrete-space.png",
      "/imagens/arched-hallway-symmetry.jpeg",
      "/imagens/curved-interior-sculpture.png",
    ],
    tech: "Python, Docker, API Development",
  },
  {
    id: "jack",
    surtile: "Fullstack Dev",
    titleLines: ["Jack", "Delamou"],
    text: "Ferdig utdannet fullstack-utvikler med sterkt engasjement for å skape løsninger som kombinerer estetikk og funksjonalitet. Har hovedansvar for backend-frontend kommunikasjon og systemintegrasjon.",
    highlight: "System-integrasjon",
    imagePosition: "left",
    images: [
      "/imagens/arched-corridor-1.jpeg",
      "/imagens/arched-interior-modern.png",
      "/imagens/curved-concrete-space.png",
    ],
    tech: "Fullstack, Authentication, Deployment",
  },
];

const CHAPTER_WEIGHTS = chapters.map(() => 0.8);
CHAPTER_WEIGHTS[0] = 1.1;
CHAPTER_WEIGHTS[CHAPTER_WEIGHTS.length - 1] = 0.6;
const TOTAL_WEIGHT = CHAPTER_WEIGHTS.reduce((a, b) => a + b, 0);

const CHAPTER_RANGES = CHAPTER_WEIGHTS.reduce((acc, weight, i) => {
  const start = i === 0 ? 0 : acc[i - 1][1];
  const end = start + weight / TOTAL_WEIGHT;
  acc.push([start, end]);
  return acc;
}, [] as [number, number][]);

function useChapterT(scrollYProgress: MotionValue<number>, index: number) {
  const [start, end] = CHAPTER_RANGES[index];
  const duration = end - start;
  const isLast = index === CHAPTER_RANGES.length - 1;

  const t = useTransform(scrollYProgress, [start, end], [0, 1], {
    clamp: true,
  });

  let opacityInput: number[];
  let opacityOutput: number[];

  if (index === 0) {
    opacityInput = [-1, 0, end - 0.05 * duration, end];
    opacityOutput = [1, 1, 1, 0];
  } else if (isLast) {
    opacityInput = [start, start + 0.10 * duration];
    opacityOutput = [0, 1];
  } else {
    opacityInput = [start, start + 0.10 * duration, end - 0.10 * duration, end];
    opacityOutput = [0, 1, 1, 0];
  }

  const opacity = useTransform(scrollYProgress, opacityInput, opacityOutput, {
    clamp: true,
  });

  return { t, opacity, start, end };
}

function MaskedLine({
  children,
  t,
  delay = 0,
  className = "",
}: {
  children: React.ReactNode;
  t: MotionValue<number>;
  delay?: number;
  className?: string;
}) {
  const inStart = 0 + delay;
  const inEnd = 0.25 + delay;

  const opacity = useTransform(t, [inStart, inEnd], [0, 1], {
    clamp: true,
  });

  return (
    <span className={`relative block ${className}`}>
      <motion.span className="relative block will-change-transform" style={{ opacity }}>
        {children}
      </motion.span>
    </span>
  );
}

function renderHighlightText(text: string, highlight: string) {
  const parts = text.split(highlight);
  if (parts.length === 1) return text;

  return (
    <>
      {parts.map((p, idx) => (
        <React.Fragment key={idx}>
          {p}
          {idx < parts.length - 1 && (
            <span className="text-blue-600">{highlight}</span>
          )}
        </React.Fragment>
      ))}
    </>
  );
}

function MediaFigure({
  chapter,
  index,
  scrollYProgress,
}: {
  chapter: Chapter;
  index: number;
  scrollYProgress: MotionValue<number>;
}) {
  const { t, opacity } = useChapterT(scrollYProgress, index);
  const [opacityValue, setOpacityValue] = React.useState(0);

  React.useEffect(() => {
    return opacity.on("change", (v) => setOpacityValue(v));
  }, [opacity]);

  const dynamicZIndex = 100 + Math.round(opacityValue * 50);

  const scaleFirst = useTransform(t, [0, 1], [1.2, 1.0], { clamp: true });
  const yFirst = useTransform(t, [0, 0.8], ["-15vh", "0vh"], { clamp: true });
  const scaleDefault = useTransform(t, [0, 1], [1.3, 1.15], { clamp: true });
  const yDefault = useTransform(t, [0, 1], ["40px", "-40px"], { clamp: true });

  const isFirst = index === 0;
  const scale = isFirst ? scaleFirst : scaleDefault;
  const y = isFirst ? yFirst : yDefault;

  const sideClass =
    chapter.imagePosition === "right"
      ? "right-6 md:right-10 lg:right-16"
      : "left-8 md:left-16 lg:left-24";

  const translateClass =
    chapter.imagePosition === "right"
      ? "translate-x-[60px]"
      : "-translate-x-[60px]";

  const hoverTranslateClass =
    chapter.imagePosition === "right"
      ? "group-hover:-translate-x-8"
      : "group-hover:translate-x-8";

  const hoverOriginClass =
    chapter.imagePosition === "right"
      ? "origin-right"
      : "origin-left";

  const stackDirection = chapter.imagePosition === "right" ? -1 : 1;
  const stackOffsets = [0, 8, 16];
  const hoverOffsets = [0, 60, 120];
  const stackRotations = [0, -2, 2];

  return (
    <motion.figure
      key={`${chapter.id}-media`}
      className={`group absolute top-1/2 -translate-y-1/2 ${translateClass} ${sideClass} hidden lg:block pointer-events-auto transition-transform duration-500 ease-out ${hoverTranslateClass}`}
      style={{ opacity, zIndex: dynamicZIndex }}
    >
      <div
        className={`relative rounded-3xl transition-transform duration-500 ease-out group-hover:scale-[1.08] ${hoverOriginClass}`}
        style={{
          width: "min(280px, 18vw)",
          aspectRatio: "220 / 275",
        }}
      >
        <motion.div
          className="block h-full w-full object-cover will-change-transform relative"
          style={{ y, scale }}
        >
          <div className="absolute inset-0" style={{ overflow: 'visible' }}>
            {[0, 1, 2].map((idx) => (
              <div
                key={`${chapter.id}-stack-${idx}`}
                className="absolute inset-0 rounded-3xl overflow-hidden shadow-[0_24px_60px_rgba(0,0,0,0.16)] transition-transform duration-500 ease-out translate-x-(--stack-x) group-hover:translate-x-(--stack-x-hover) rotate-(--stack-rot)"
                style={{
                  zIndex: 10 + idx,
                  ["--stack-x" as string]: `${stackDirection * stackOffsets[idx]}px`,
                  ["--stack-x-hover" as string]: `${stackDirection * hoverOffsets[idx]}px`,
                  ["--stack-rot" as string]: `${stackDirection * stackRotations[idx]}deg`,
                }}
              >
                <Image
                  src={chapter.images[idx]}
                  alt={`${chapter.titleLines.join(" ")} ${idx + 1}`}
                  fill
                  unoptimized
                  className="object-cover"
                  loading="lazy"
                  sizes="(max-width: 768px) 0px, 22vw"
                />
              </div>
            ))}
          </div>
        </motion.div>
      </div>
    </motion.figure>
  );
}

function FixedMediaLayer({
  scrollYProgress,
}: {
  scrollYProgress: MotionValue<number>;
}) {
  return (
    <div className="absolute inset-0 h-full w-full z-10" style={{ overflow: 'visible' }}>
      <div className="relative h-full w-full" style={{ overflow: 'visible' }}>
        {chapters.map((c, i) => (
          <MediaFigure
            key={c.id}
            chapter={c}
            index={i}
            scrollYProgress={scrollYProgress}
          />
        ))}
      </div>
    </div>
  );
}

function ScrollListScaffold() {
  return (
    <ul className="relative flex flex-col gap-0 pb-32 pt-16 lg:pb-24 lg:pt-24">
      {chapters.map((c, i) => (
        <li
          key={c.id}
          className={`relative ${
            i === chapters.length - 1
              ? "min-h-[50vh] lg:min-h-[60vh]"
              : "min-h-[80vh] lg:min-h-screen"
          }`}
        ></li>
      ))}
    </ul>
  );
}

function ChapterItem({
  chapter,
  index,
  scrollYProgress,
}: {
  chapter: Chapter;
  index: number;
  scrollYProgress: MotionValue<number>;
}) {
  const { t, opacity } = useChapterT(scrollYProgress, index);

  const defaultY = useTransform(t, [0, 0.5, 1], ["14px", "0px", "-14px"], {
    clamp: true,
  });

  const firstY = useTransform(t, [0, 0.85], ["-35vh", "0vh"], {
    clamp: true,
  });

  const y = index === 0 ? firstY : defaultY;

  const scale = useTransform(t, [0, 1], [1.1, 1.0], { clamp: true });
  const imageY = useTransform(t, [0, 1], ["10px", "-10px"], { clamp: true });

  return (
    <motion.div
      className="absolute left-1/2 top-1/2 w-full max-w-2xl -translate-x-1/2 -translate-y-1/2"
      style={{ opacity, y }}
    >
      <div className="mb-4 sm:mb-6 overflow-clip font-mono text-[10px] sm:text-xs uppercase tracking-[0.28em] text-neutral-500">
        <MaskedLine t={t}>{chapter.surtile}</MaskedLine>
      </div>

      <h2
        className="font-medium tracking-tight text-neutral-950"
        style={{
          fontSize: "clamp(20px, 4vw, 52px)",
          lineHeight: 1.02,
        }}
      >
        {chapter.titleLines.map((line, idx) => (
          <MaskedLine
            key={idx}
            t={t}
            delay={idx * 0.05}
            className="text-balance"
          >
            {line}
          </MaskedLine>
        ))}
      </h2>

      <div className="mx-auto mt-4 sm:mt-6 max-w-xl overflow-clip text-sm leading-relaxed text-neutral-600 md:text-base">
        <MaskedLine t={t} delay={0.08} className="text-balance">
          {renderHighlightText(chapter.text, chapter.highlight)}
        </MaskedLine>
      </div>

      <div className="mt-6 flex justify-center gap-2 flex-wrap overflow-clip">
        <MaskedLine t={t} delay={0.12}>
          <div className="inline-flex gap-2">
            {chapter.tech.split(', ').map((tech, i) => (
              <span
                key={i}
                className="px-3 py-1 bg-neutral-100 text-neutral-700 rounded-full text-xs font-medium"
              >
                {tech}
              </span>
            ))}
          </div>
        </MaskedLine>
      </div>

      <div className="mt-8 sm:mt-12 flex justify-center pointer-events-auto lg:hidden">
        <motion.div
          className="relative rounded-2xl overflow-clip"
          style={{
            width: "min(180px, 50vw)",
            aspectRatio: "220 / 275",
            scale,
            y: imageY,
          }}
        >
          <Image
            src={chapter.images[0]}
            alt={chapter.titleLines.join(" ")}
            fill
            unoptimized
            className="object-cover"
            loading="lazy"
            sizes="(max-width: 640px) 50vw, 180px"
          />
        </motion.div>
      </div>
    </motion.div>
  );
}

function CombinedTextLayer({
  scrollYProgress,
}: {
  scrollYProgress: MotionValue<number>;
}) {
  return (
    <div className="pointer-events-none absolute inset-0 h-full w-full z-20">
      <div className="flex h-full w-full items-center justify-center px-6">
        <div className="w-full max-w-2xl text-center">
          {chapters.map((c, i) => (
            <ChapterItem
              key={c.id}
              chapter={c}
              index={i}
              scrollYProgress={scrollYProgress}
            />
          ))}
        </div>
      </div>
    </div>
  );
}

// Export constant logic for external use if needed
export const ABOUT_CHAPTER_RANGES = CHAPTER_RANGES;

// Modified component to accept external state handler and removed internal sidebar
interface AboutSectionProps {
  onChapterChange?: (index: number) => void;
}

export default function AboutSection({ onChapterChange }: AboutSectionProps) {
  const containerRef = React.useRef<HTMLDivElement>(null);
  const lastChapterRef = React.useRef<number>(0);

  const { scrollYProgress } = useScroll({
    target: containerRef,
    offset: ["start end", "end end"],
  });

  // Report active chapter index up to parent
  React.useEffect(() => {
    return scrollYProgress.on("change", (v) => {
      if (onChapterChange) {
        const clamped = Math.min(0.9999, Math.max(0, v));
        const index = CHAPTER_RANGES.findIndex(
          ([start, end]) => clamped >= start && clamped < end
        );
        const nextIndex = index === -1 ? chapters.length - 1 : index;
        if (nextIndex !== lastChapterRef.current) {
          lastChapterRef.current = nextIndex;
          onChapterChange(nextIndex);
        }
      }
    });
  }, [scrollYProgress, onChapterChange]);

  return (
    // Replaced 'min-h-screen' with relative container to fit in flow
    <div className="relative overflow-visible" id="about-scroll-container" ref={containerRef}>
      
      <div className="relative mx-auto max-w-[85vw]">
        <div className="relative">
          <div className="sticky top-0 h-screen overflow-visible z-30 hidden lg:block">
             {/* Sticky container hidden on mobile to avoid layout issues, using standard stack instead */}
            <CombinedTextLayer scrollYProgress={scrollYProgress} />
            <FixedMediaLayer scrollYProgress={scrollYProgress} />
          </div>
          
          {/* Mobile Fallback View (simplified from design logic which hides many elements on lg) */}
          <div className="lg:hidden">
             {/* The layout for mobile is handled by ChapterItem's logic which conditionally shows image/text */}
             <CombinedTextLayer scrollYProgress={scrollYProgress} />
          </div>

          <div className="lg:-mt-[100vh]">
            <ScrollListScaffold />
          </div>
        </div>
      </div>
    </div>
  );
}
