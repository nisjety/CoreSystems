"use client";

import { motion, useScroll, useSpring, useTransform } from "framer-motion";
import Image from "next/image";
import { useRef } from "react";
import { usePrefersReducedMotion } from "@/lib/use-prefers-reduced-motion";
import type { Locale } from "@/lib/i18n/locales";
import { aboutContent } from "./about-page-data";
import { AboutReveal } from "./about-reveal";
import {
  ABOUT_HERO_COMPACT_HEIGHT_VH,
  ABOUT_HERO_COMPACT_WIDTH_VW,
  ABOUT_HERO_SCROLL_DISTANCE_VH,
  ABOUT_HERO_START_HEIGHT_VH,
} from "./about-hero-motion";

export function AboutHeroSection({ locale }: { locale: Locale }) {
  const content = aboutContent[locale].hero;
  const heroRef = useRef<HTMLElement>(null);
  const reducedMotion = usePrefersReducedMotion();
  const { scrollYProgress } = useScroll({
    target: heroRef,
    offset: ["start start", "end end"],
  });
  const smoothScrollProgress = useSpring(scrollYProgress, {
    stiffness: 105,
    damping: 28,
    mass: 0.2,
  });
  const compactWidth = ABOUT_HERO_COMPACT_WIDTH_VW / 100;
  const initialHeight = `${ABOUT_HERO_START_HEIGHT_VH}svh`;
  const compactHeight = `${ABOUT_HERO_COMPACT_HEIGHT_VH}svh`;
  const imageScale = useTransform(
    smoothScrollProgress,
    [0, 0.7, 1],
    [1, 1, 1.08],
  );
  const imageY = useTransform(smoothScrollProgress, [0, 1], [0, 24]);
  const frameHeight = useTransform(
    smoothScrollProgress,
    [0, 0.52, 1],
    [initialHeight, initialHeight, compactHeight],
  );
  const frameWidth = useTransform(
    smoothScrollProgress,
    [0, 0.52, 1],
    [
      "calc(100% - clamp(8px, 1vw, 16px))",
      "calc(100% - clamp(8px, 1vw, 16px))",
      `${compactWidth * 100}vw`,
    ],
  );
  const frameY = useTransform(smoothScrollProgress, [0, 1], [0, 8]);
  const frameRadius = useTransform(
    smoothScrollProgress,
    [0, 0.52, 1],
    ["1.25rem", "1.25rem", "1.75rem"],
  );
  const titleOpacity = useTransform(
    smoothScrollProgress,
    [0, 0.35, 0.56],
    [1, 1, 0],
  );
  const titleY = useTransform(smoothScrollProgress, [0, 0.56], [0, -34]);
  const introductionOpacity = useTransform(
    smoothScrollProgress,
    [0.4, 0.72, 1],
    [0, 1, 1],
  );
  const introductionY = useTransform(
    smoothScrollProgress,
    [0.4, 0.72],
    [36, 0],
  );

  return (
    <section
      ref={heroRef}
      id="hero"
      className={
        reducedMotion
          ? "relative min-h-[100svh] bg-[#FCFCFA] text-[#FCFCFA]"
          : "relative min-h-[900px] bg-[#FCFCFA] text-[#FCFCFA] max-[760px]:min-h-[780px]"
      }
      style={
        reducedMotion
          ? undefined
          : { height: `${ABOUT_HERO_SCROLL_DISTANCE_VH}svh` }
      }
      aria-labelledby="about-page-heading"
    >
      <div
        className={
          reducedMotion
            ? "relative min-h-[80svh] overflow-hidden"
            : "sticky top-0 h-[100svh] overflow-hidden"
        }
      >
        <motion.div
          className="absolute left-1/2 top-[clamp(4px,0.5vw,8px)] z-10 overflow-hidden bg-[#191918]"
          style={
            reducedMotion
              ? {
                  width: "calc(100% - clamp(8px, 1vw, 16px))",
                  height: initialHeight,
                  x: "-50%",
                  borderRadius: "1.25rem",
                }
              : {
                  width: frameWidth,
                  height: frameHeight,
                  x: "-50%",
                  y: frameY,
                  borderRadius: frameRadius,
                }
          }
        >
          <motion.div
            className="absolute inset-[-6%]"
            style={reducedMotion ? undefined : { scale: imageScale, y: imageY }}
          >
            <Image
              src="/images/Triodelab-bilder/Gruppebilde.jpg"
              alt={content.imageAlt}
              fill
              priority
              sizes="100vw"
              className="object-cover object-center brightness-[0.85]"
            />
          </motion.div>
          <div className="absolute inset-0 bg-[linear-gradient(180deg,rgba(20,20,19,0.28),rgba(20,20,19,0.04)_28%,rgba(20,20,19,0.04)_72%,rgba(20,20,19,0.32))]" />
          <motion.div
            style={
              reducedMotion ? undefined : { opacity: titleOpacity, y: titleY }
            }
            className="absolute inset-0 grid place-items-center px-6 text-center"
          >
            <AboutReveal>
              <h1
                id="about-page-heading"
                className="hero-title font-manrope !text-white [text-shadow:0_2px_24px_rgba(0,0,0,0.35)]"
              >
                {content.title}
              </h1>
            </AboutReveal>
          </motion.div>
        </motion.div>
        <motion.div
          aria-hidden="true"
          className="pointer-events-none absolute inset-x-[clamp(24px,4vw,56px)] top-[72svh] z-0 grid grid-cols-2 gap-6 text-[#161616] min-[761px]:grid-cols-3"
          style={
            reducedMotion
              ? undefined
              : { opacity: introductionOpacity, y: introductionY }
          }
        >
          <span className="hero-title font-manrope text-[#161616]">
            {content.titleParts[0]}
          </span>
          <span className="hero-title font-manrope text-[#161616] min-[761px]:text-center">
            {content.titleParts[1]}
          </span>
          <span className="hidden font-label text-[0.68rem] font-light uppercase tracking-[0.14em] text-[#626260] min-[761px]:block min-[761px]:self-end min-[761px]:pb-2 min-[761px]:text-right">
            {content.location}
          </span>
        </motion.div>
      </div>
    </section>
  );
}
