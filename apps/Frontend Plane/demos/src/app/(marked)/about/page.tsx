"use client";

import * as React from "react";
import Image from "next/image";
import Link from "next/link";
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
    id: "qualai",
    surtile: "Kvalitet",
    titleLines: ["QualAI for", "nettsider"],
    text: "Helhetlig plattform for lenkevalidering, WCAG-tilsyn og LIKS-lesbarhetsanalyse for Prokom. Resultat: modulær mikrotjenestearkitektur og AI-støttet analyse.",
    highlight: "AI-støttet analyse",
    imagePosition: "right",
    images: [
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e4de903e6acfdfc0e4a395_work-thumbnail.png",
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e064fd2efe8e60820645f9_dashboard-1.png",
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d843629cf6600f4d48_dashboard.png",
    ],
    tech: "Next.js, Python, AI/ML",
  },
  {
    id: "vaero",
    surtile: "Mobil",
    titleLines: ["Væro", "AI Vær", "Assistant"],
    text: "Smart værapplikasjon som kombinerer nøyaktige data fra Yr med AI-drevne klesforslag og personlige anbefalinger. Tilbyr intelligente varsler og prediktiv analyse basert på brukerens preferanser og aktivitetsmønstre.",
    highlight: "AI-drevne klesforslag",
    imagePosition: "right",
    images: [
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e9b414b8428bcce621e898_Work%20Thumbnail.png",
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e4dfafaa8988f379a7d113_moblie-desk.png",
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e4dfbf094585887d921aa1_web-landing-pages.png",
    ],
    tech: "React Native, AI, Yr API",
  },
  {
    id: "denvanskelige",
    surtile: "Dialog",
    titleLines: ["Den Vanskelige", "Samtalen"],
    text: "Et kunstnerisk prosjekt som utforsker dialogens potensiale. Åtte podkastepisoder med åpne, ærlige og utfordrende samtaler om identitet, traumer, polarisering og kritisk tenkning. Støttet av Fritt Ord og Nansen Fredssenter.",
    highlight: "dialogens potensiale",
    imagePosition: "right",
    images: [
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65ac380540612cf79cbccdfd_about-thumbnail.png",
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d9ecc6441985cea84d_mental%20health%20(3).png",
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d928e19e693ab06105_mental%20health%20app%20(1).png",
    ],
    tech: "Next.js, WebSocket, Audio Streaming",
  },
  {
    id: "agenci",
    surtile: "Eiendom",
    titleLines: ["Agenci", "AI Megler", "Plattform"],
    text: "Moderne eiendomsplattform med AI-assistert saksbehandling og automatisering. Sømløs integrasjon med Finn.no og eiendomsregistre. Strømlinjeformer hele salgsprosessen fra verdivurdering til avslutning.",
    highlight: "AI-assistert saksbehandling",
    imagePosition: "right",
    images: [
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d9a2389a96025c1cb4_community%20app.png",
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d9937ba2778c1b4aec_roasters%202.png",
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d8a1d590815357bf63_perfect%20roast%202.png",
    ],
    tech: "Next.js, Clerk, AI Integration",
  },
  {
    id: "domain",
    surtile: "Monitoring",
    titleLines: ["Domain", "Tracking", "System"],
    text: "Real-time domeneovervåkning med WebSocket-tilkoblinger. Sporer domenestatus, registrar-informasjon og tilgjengelighet. Øyeblikkelige varsler og omfattende analyser for domenehåndtering.",
    highlight: "Real-time domeneovervåkning",
    imagePosition: "right",
    images: [
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e064fd2efe8e60820645f9_dashboard-1.png",
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d8463d2760f87554ba_Quotes%20App%202.png",
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d98d4ce33501e8cafe_Quotes%20App%201.png",
    ],
    tech: "Next.js, WebSocket, Real-time",
  },
  {
    id: "draktboden",
    surtile: "E-handel",
    titleLines: ["Draktboden", "Nettbutikk"],
    text: "Moderne e-handelsplattform for sportsbekledning. Responsiv design, integrert betalingsløsning og lagerstyring. Optimalisert for mobil shopping med sømløs brukeropplevelse.",
    highlight: "sportsbekledning",
    imagePosition: "right",
    images: [
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d843629cf6600f4d48_dashboard.png",
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d812defb2609fdd955_certora%202.png",
      "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d8af5752f6c7a209c5_certora%201.png",
    ],
    tech: "Next.js, Stripe, Tailwind CSS",
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

  let scale: MotionValue<number>;
  let y: MotionValue<string>;

  if (index === 0) {
    scale = useTransform(t, [0, 1], [1.2, 1.0], { clamp: true });
    y = useTransform(t, [0, 0.8], ["-15vh", "0vh"], { clamp: true });
  } else {
    scale = useTransform(t, [0, 1], [1.3, 1.15], { clamp: true });
    y = useTransform(t, [0, 1], ["40px", "-40px"], { clamp: true });
  }

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
                className="absolute inset-0 rounded-3xl overflow-hidden shadow-[0_24px_60px_rgba(0,0,0,0.16)] transition-transform duration-500 ease-out translate-x-[var(--stack-x)] group-hover:translate-x-[var(--stack-x-hover)] rotate-[var(--stack-rot)]"
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
              : "min-h-[80vh] lg:min-h-[100vh]"
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

function Sidebar({ scrollYProgress }: { scrollYProgress: MotionValue<number> }) {
  const [activeIndex, setActiveIndex] = React.useState(0);

  React.useEffect(() => {
    return scrollYProgress.on("change", (v) => {
      const clamped = Math.min(0.9999, Math.max(0, v));
      const index = CHAPTER_RANGES.findIndex(
        ([start, end]) => clamped >= start && clamped < end
      );
      setActiveIndex(index === -1 ? chapters.length - 1 : index);
    });
  }, [scrollYProgress]);

  return (
    <div className="fixed left-8 top-1/2 -translate-y-1/2 z-30 hidden xl:block">
      <div className="flex flex-col gap-3">
        <Link
          href="/intro"
          className="mb-6 text-xs font-bold tracking-tight hover:text-blue-600 transition-colors"
        >
          ← TILBAKE
        </Link>
        {chapters.map((chapter, i) => (
          <button
            key={chapter.id}
            onClick={() => {
              const container = document.querySelector('[data-scroll-container]');
              if (container) {
                const [start, end] = CHAPTER_RANGES[i];
                const scrollHeight = container.scrollHeight - window.innerHeight;
                const isLast = i === chapters.length - 1;
                const scrollProgress = isLast ? start + (end - start) * 0.3 : start;
                window.scrollTo({
                  top: scrollProgress * scrollHeight,
                  behavior: 'smooth',
                });
              }
            }}
            className={`group flex items-center gap-3 transition-all ${
              activeIndex === i ? 'opacity-100' : 'opacity-30 hover:opacity-60'
            }`}
          >
            <div
              className={`h-px transition-all ${
                activeIndex === i ? 'w-8 bg-blue-600' : 'w-4 bg-neutral-400'
              }`}
            />
            <div
              className={`text-xs font-medium transition-all ${
                activeIndex === i ? 'text-neutral-950' : 'text-neutral-500'
              }`}
            >
              {chapter.surtile}
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}

export default function AboutPage() {
  const containerRef = React.useRef<HTMLDivElement>(null);

  const { scrollYProgress } = useScroll({
    target: containerRef,
    offset: ["start end", "end end"],
  });

  return (
    <div className="min-h-screen overflow-visible relative" data-scroll-container>
      <Sidebar scrollYProgress={scrollYProgress} />
      
      <div className="relative mx-auto max-w-[85vw]">
        <div ref={containerRef} className="relative">
          <div className="sticky top-0 h-screen overflow-visible z-10">
            <CombinedTextLayer scrollYProgress={scrollYProgress} />
            <FixedMediaLayer scrollYProgress={scrollYProgress} />
          </div>

          <div className="-mt-[100vh]">
            <ScrollListScaffold />
          </div>
        </div>
      </div>
    </div>
  );
}
