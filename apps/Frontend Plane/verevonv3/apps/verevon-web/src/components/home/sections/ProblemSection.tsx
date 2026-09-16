"use client";

import Image from "next/image";
import { useLayoutEffect, useRef } from "react";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { ArrowButton } from "@/components/ui/ArrowButton";
import { Eyebrow } from "@/components/ui/SectionHeading";
import { SignalPathLayer } from "@/components/home/sections/SignalPathLayer";
import { cn } from "@/lib/utils";

gsap.registerPlugin(ScrollTrigger);

type GalaxyAsset = {
  fit?: "contain" | "cover";
  objectPosition?: string;
  src: string;
};

type ParticleDefinition = {
  asset: GalaxyAsset;
  opacity: number;
  speed: number;
  x: number;
  y: number;
  z: number;
};

type ParticleRuntime = {
  currentScale: number;
  element: HTMLElement;
  extra: number;
  height: number;
  position: number;
  speed: number;
  top: number;
  z: number;
};

const PARTICLE_COUNT = 32;
const PARTICLE_SPEEDS = [0.8, 0.9, 1, 1.1, 1.2] as const;
const PARTICLE_DEPTHS = [-200, -150, -100, -50, 0, 50, 100, 150, 200] as const;
const PARTICLE_MOTION_SCALE = 0.45;
const PARTICLE_SCROLL_IMPULSE = 0.05 * PARTICLE_MOTION_SCALE;
const PARTICLE_AMBIENT_DRIFT = 0.15 * PARTICLE_MOTION_SCALE;
const WORD_REVEAL_BASE_OPACITY = 0.55;

const galaxyAssets: GalaxyAsset[] = [
  // Arbeidsflater og eksperter
  { src: "/verevon-mood/problem-research/active/tools-phone-desk.jpg" },
  { src: "/verevon-mood/problem-research/active/expert-white-suit.jpg" },
  { src: "/verevon-mood/problem-research/active/expert-white-chair.jpg" },
  // Kunnskapskilder
  { src: "/verevon-mood/problem-research/active/knowledge-books-people.jpg" },
  { src: "/verevon-mood/problem-research/active/library-ladder.jpg" },
  { src: "/verevon-mood/problem-research/active/archive-card-catalog.jpg" },
  { src: "/verevon-mood/problem-research/active/knowledge-reading-sculpture.jpg" },
  // Samarbeid og praktisk arbeid
  { src: "/verevon-mood/problem-research/active/collaboration-laptops.jpg" },
  { src: "/verevon-mood/problem-research/active/engineering-orange-helmet.jpg" },
  // Teknikk og matematikk
  { src: "/verevon-mood/problem-research/active/math-blackboard.jpg" },
  { src: "/verevon-mood/problem-research/active/math-abacus.jpg" },
  // Rådgivning og gjennomlesning
  { src: "/verevon-mood/problem-research/active/advisory-pen-paper.jpg" },
  { src: "/verevon-mood/problem-research/active/advisory-eyeglasses.jpg" },
  { src: "/verevon-mood/problem-research/active/operations-chaotic-office.jpg" },
  // Verevon i arbeid
  { src: "/verevon-mood/problem-research/active/verevon-library-silhouette.jpg" },
  { src: "/verevon-mood/problem-research/active/verevon-expert-corridor.jpg" },
  { src: "/verevon-mood/problem-research/active/verevon-focused-workspace.jpg" },
  { src: "/verevon-mood/problem-research/active/verevon-research-table.jpg" },
  { src: "/verevon-mood/problem-research/active/verevon-library-source.jpg" },
  { src: "/verevon-mood/problem-research/active/verevon-library-dialogue.jpg" },
  { src: "/verevon-mood/problem-research/active/verevon-team-work.jpg" },
];

function seededUnit(seed: number) {
  const value = Math.sin(seed * 12.9898) * 43758.5453;
  return value - Math.floor(value);
}

const particles: ParticleDefinition[] = Array.from(
  { length: PARTICLE_COUNT },
  (_, index) => {
    const z = PARTICLE_DEPTHS[index % PARTICLE_DEPTHS.length];
    const opacity = z < 0 ? Math.max(0.5, 1 + z / 250) : 1;

    return {
      asset: galaxyAssets[index % galaxyAssets.length],
      opacity,
      speed: PARTICLE_SPEEDS[index % PARTICLE_SPEEDS.length],
      x: Math.floor(seededUnit(index + 11) * 95),
      y: Math.floor(seededUnit(index + 67) * 100),
      z,
    };
  },
);

const problemLines = [
  "Kunnskapen finnes.".split(" "),
  "Men den er spredt.".split(" "),
];
const problemCopy = problemLines.map((line) => line.join(" ")).join(" ");
const supportingCopy =
  "Verevon samler kildene, forstår sammenhengen og gjør neste steg klart — med dere i kontroll.";

function clamp01(value: number) {
  return Math.min(1, Math.max(0, value));
}

function interpolateWordOpacity(progress: number, index: number, count: number) {
  const step = count > 1 ? index / count : 0;
  const width = count > 1 ? 0.62 : 1;
  const localProgress = clamp01((progress - step) / width);

  return (
    WORD_REVEAL_BASE_OPACITY +
    (1 - WORD_REVEAL_BASE_OPACITY) *
      (1 - Math.pow(1 - localProgress, 3))
  );
}

function lerp(from: number, to: number, amount: number) {
  return (1 - amount) * from + amount * to;
}

function ProblemLine({ line, lineIndex }: { line: string[]; lineIndex: number }) {
  return (
    <span aria-hidden="true" className="block">
      {line.map((word, wordIndex) => (
        <span
          className="mr-[0.22em] inline-block opacity-55 will-change-[opacity]"
          data-problem-title-word=""
          key={`${lineIndex}-${wordIndex}-${word}`}
        >
          {word}
        </span>
      ))}
    </span>
  );
}

function GalaxyParticle({
  particle,
  particleRef,
}: {
  particle: ParticleDefinition;
  particleRef: (element: HTMLDivElement | null) => void;
}) {
  return (
    <div
      className="absolute aspect-square w-[35px] will-change-transform min-[700px]:w-[73px]"
      data-problem-particle=""
      ref={particleRef}
      style={{
        left: `${particle.x}%`,
        top: `${particle.y}%`,
        transformStyle: "preserve-3d",
      }}
    >
      <div className="relative size-full overflow-hidden bg-background">
        <Image
          alt=""
          aria-hidden="true"
          className={cn(
            "select-none",
            particle.asset.fit === "contain"
              ? "object-contain"
              : "object-cover saturate-[0.84] contrast-[1.02]",
          )}
          draggable={false}
          fill
          loading="lazy"
          quality={65}
          sizes="(max-width: 699px) 35px, 73px"
          src={particle.asset.src}
          style={{ objectPosition: particle.asset.objectPosition }}
        />
        <span
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 bg-background"
          data-problem-particle-overlay=""
          style={{ opacity: 1 - particle.opacity }}
        />
      </div>
    </div>
  );
}

export function ProblemSection() {
  const sectionRef = useRef<HTMLElement>(null);
  const particleContainerRef = useRef<HTMLDivElement>(null);
  const particleRefs = useRef<Array<HTMLDivElement | null>>([]);

  useLayoutEffect(() => {
    const section = sectionRef.current;
    const particleContainer = particleContainerRef.current;

    if (!section || !particleContainer) return;

    const media = gsap.matchMedia();
    const forceFullMotion =
      document.documentElement.dataset.motion === "full";

    media.add("(prefers-reduced-motion: reduce)", () => {
      if (forceFullMotion) return;

      const heading = section.querySelector<HTMLElement>(
        "[data-problem-heading]",
      );
      const words = gsap.utils.toArray<HTMLElement>(
        "[data-problem-title-word]",
        section,
      );

      gsap.set(words, {
        clearProps: "all",
        opacity: 1,
      });
      gsap.set(heading, { clearProps: "all", y: 0 });
      gsap.set(particleRefs.current, {
        clearProps: "transform",
      });
    });

    media.add(
      forceFullMotion ? "all" : "(prefers-reduced-motion: no-preference)",
      () => {
        const context = gsap.context(() => {
        const words = gsap.utils.toArray<HTMLElement>(
          "[data-problem-title-word]",
          section,
        );
        const heading = section.querySelector<HTMLElement>(
          "[data-problem-heading]",
        );

        if (heading && words.length > 0) {
          gsap.set(words, { opacity: WORD_REVEAL_BASE_OPACITY });
          gsap.set(heading, { y: 0, force3D: true });

          ScrollTrigger.create({
            trigger: section,
            start: "top 46%",
            end: "top 12%",
            scrub: 0.16,
            invalidateOnRefresh: true,
            id: "verevon-problem-word-reveal",
            onUpdate: ({ progress }) => {
              gsap.set(heading, {
                y: -24 * progress,
                force3D: true,
              });

              words.forEach((word, index) => {
                gsap.set(word, {
                  opacity: interpolateWordOpacity(
                    progress,
                    index,
                    words.length,
                  ),
                });
              });
            },
            onLeave: () => {
              gsap.set(heading, { y: -24 });
              gsap.set(words, { opacity: 1 });
            },
            onLeaveBack: () => {
              gsap.set(heading, { y: 0 });
              gsap.set(words, { opacity: WORD_REVEAL_BASE_OPACITY });
            },
          });
        }

        const scrollState = {
          current: 0,
          direction: "up" as "down" | "up",
          directionSign: 1,
          last: 0,
          lastScrollY: window.scrollY,
          target: 0,
        };
        let containerHeight = 0;
        let containerOffsetHeight = 0;
        let isNearViewport = false;
        let runtimes: ParticleRuntime[] = [];

        const handleResize = () => {
          const containerRect = particleContainer.getBoundingClientRect();
          const isSmall = window.innerWidth < 700;

          runtimes = particleRefs.current.flatMap((element, index) => {
            if (!element) return [];

            const definition = particles[index];
            const left = isSmall
              ? (index % 4) * 25 + (definition.x % 20)
              : definition.x;

            element.style.left = `${left}%`;
            element.style.top = `${definition.y}%`;
            element.style.transform = "translate3d(0, 0px, 0)";

            const rect = element.getBoundingClientRect();

            return [
              {
                currentScale: 1,
                element,
                extra: 0,
                height: rect.height,
                position: 0,
                speed: definition.speed,
                top: rect.top - containerRect.top,
                z: definition.z,
              },
            ];
          });

          containerHeight = particleContainer.clientHeight;
          containerOffsetHeight = containerHeight * 0.1;
          scrollState.current = 0;
          scrollState.last = 0;
          scrollState.target = 0;
        };

        const handleScroll = () => {
          const nextScrollY = window.scrollY;
          const velocity = nextScrollY - scrollState.lastScrollY;

          if (velocity !== 0) {
            scrollState.target += velocity * PARTICLE_SCROLL_IMPULSE;
            scrollState.directionSign = Math.sign(velocity);
          }

          scrollState.lastScrollY = nextScrollY;
        };

        const updateParticles = (_time: number, deltaTime: number) => {
          if (!isNearViewport || containerHeight === 0) return;

          scrollState.target +=
            PARTICLE_AMBIENT_DRIFT * deltaTime * scrollState.directionSign;
          scrollState.current = lerp(
            scrollState.current,
            scrollState.target,
            0.1,
          );
          scrollState.direction =
            scrollState.current < scrollState.last ? "down" : "up";

          runtimes.forEach((runtime) => {
            runtime.position =
              -scrollState.current * runtime.speed - runtime.extra;

            const bottom =
              runtime.position + runtime.top + runtime.height;
            const isBefore = bottom < -containerOffsetHeight;
            const isAfter =
              bottom > containerHeight + containerOffsetHeight;

            if (scrollState.direction === "up" && isBefore) {
              runtime.extra -=
                containerHeight + containerOffsetHeight;
            }

            if (scrollState.direction === "down" && isAfter) {
              runtime.extra += containerHeight;
            }

            const position = runtime.position + runtime.top;
            const progress = Math.max(
              0,
              Math.min(1, position / containerHeight),
            );
            const scale = 0.5 + progress * 0.9;

            runtime.currentScale = lerp(
              runtime.currentScale,
              scale,
              0.25,
            );
            runtime.element.style.transform =
              `translate3d(0, ${runtime.position}px, ${runtime.z}px) scale(${runtime.currentScale})`;
          });

          scrollState.last = scrollState.current;
        };

        const viewportObserver = new IntersectionObserver(
          ([entry]) => {
            isNearViewport = entry.isIntersecting;
            scrollState.lastScrollY = window.scrollY;
          },
          { rootMargin: "50% 0px" },
        );
        const resizeObserver = new ResizeObserver(handleResize);

        handleResize();
        viewportObserver.observe(section);
        resizeObserver.observe(particleContainer);
        window.addEventListener("resize", handleResize);
        window.addEventListener("scroll", handleScroll, { passive: true });
        gsap.ticker.add(updateParticles);

        document.fonts?.ready.then(() => {
          handleResize();
          ScrollTrigger.refresh();
        });

        return () => {
          viewportObserver.disconnect();
          resizeObserver.disconnect();
          window.removeEventListener("resize", handleResize);
          window.removeEventListener("scroll", handleScroll);
          gsap.ticker.remove(updateParticles);
        };
        }, section);

        return () => context.revert();
      },
    );

    return () => media.revert();
  }, []);

  return (
    <section
      aria-labelledby="problem-title"
      className="relative isolate flex h-[75svh] min-h-[630px] w-full items-center overflow-hidden bg-background text-verevon-j-text"
      id="problemet"
      ref={sectionRef}
    >
      <SignalPathLayer variant="problem" />

      <div
        aria-hidden="true"
        className="pointer-events-none absolute left-0 top-[-25%] z-0 h-[150%] w-full [perspective:800px]"
        ref={particleContainerRef}
      >
        {particles.map((particle, index) => (
          <GalaxyParticle
            key={index}
            particle={particle}
            particleRef={(element) => {
              particleRefs.current[index] = element;
            }}
          />
        ))}
      </div>

      <div
        aria-hidden="true"
        className="pointer-events-none absolute left-[-10%] top-0 z-[2] h-40 w-[120%] bg-gradient-to-b from-background to-transparent"
      />

      <Eyebrow
        className="absolute left-[var(--verevon-edge)] top-[clamp(28px,4vh,48px)] z-20 max-[760px]:left-[var(--verevon-page-pad)] max-[760px]:top-5"
        marker
      >
        01 / Problemet
      </Eyebrow>

      <div className="relative z-10 flex w-full flex-col items-center justify-center gap-[clamp(1.6rem,2vw,2rem)] px-[var(--verevon-edge)] max-[760px]:px-[var(--verevon-page-pad)]">
        <h2
          aria-label={problemCopy}
          className="verevon-home-heading verevon-problem-text-shadow m-0 mx-auto max-w-[16ch] text-center max-[760px]:mx-0 max-[760px]:max-w-none max-[760px]:text-left"
          data-problem-heading=""
          id="problem-title"
        >
          {problemLines.map((line, lineIndex) => (
            <ProblemLine
              line={line}
              lineIndex={lineIndex}
              key={lineIndex}
            />
          ))}
        </h2>

        <div className="flex max-w-[512px] flex-col items-center text-center max-[760px]:items-start max-[760px]:self-start max-[760px]:text-left">
          <p className="verevon-body-lg text-pretty text-verevon-text-muted">
            {supportingCopy}
          </p>
          <ArrowButton className="mt-7" href="#produkt" variant="dark">
            Se hvordan Verevon virker
          </ArrowButton>
        </div>
      </div>
    </section>
  );
}

export default ProblemSection;
