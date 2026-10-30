"use client";

import Image from "next/image";
import { useLayoutEffect, useRef, useState } from "react";
import { motion, useScroll, useTransform } from "framer-motion";
import { ArrowRight, ChevronDown } from "@/components/icons";
import { easing } from "@/lib/motion";
import { usePrefersReducedMotion } from "@/lib/use-prefers-reduced-motion";
import { scrollToElement } from "@/lib/utils";
import type { Locale } from "@/lib/i18n/locales";
import { localizeHref } from "@/lib/i18n/routes";
import { heroContent } from "./hero-content";

const mediaHandoffProgress = 1.16;

export function HeroSection({ locale }: { locale: Locale }) {
  const content = heroContent[locale];
  const advisoryHref = localizeHref(
    "/tjenester/radgivning-og-konsulentbistand",
    locale,
  );
  const heroRef = useRef<HTMLElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const reduceMotion = usePrefersReducedMotion();

  // Default guess for the first paint; corrected as soon as we can
  // measure the real text block so the image never covers the text
  // on short/narrow viewports.
  const [mediaStart, setMediaStart] = useState(66);

  useLayoutEffect(() => {
    const updateMediaStart = () => {
      const contentEl = contentRef.current;
      if (!contentEl) {
        return;
      }
      const contentHeight = contentEl.getBoundingClientRect().height;
      const viewportHeight = window.innerHeight;
      const buffer = 16;
      const percent = ((contentHeight + buffer) / viewportHeight) * 100;
      setMediaStart(Math.min(80, Math.max(50, percent)));
    };

    updateMediaStart();
    window.addEventListener("resize", updateMediaStart);
    return () => window.removeEventListener("resize", updateMediaStart);
  }, []);

  const { scrollYProgress } = useScroll({
    target: heroRef,
    offset: ["start start", "end end"],
  });
  const mediaClip = useTransform(
    scrollYProgress,
    [0, mediaHandoffProgress],
    [`inset(${mediaStart}% 0% 0% 0%)`, "inset(0% 0% 0% 0%)"],
  );
  const mediaScale = useTransform(scrollYProgress, [0, 1], [1.04, 1]);
  const mediaX = useTransform(scrollYProgress, [0, 1], ["-0.75%", "0%"]);
  const titleY = useTransform(scrollYProgress, [0, 1], ["0svh", "-26svh"]);
  const supportingTextY = useTransform(
    scrollYProgress,
    [0, 1],
    ["0svh", "-7svh"],
  );
  const entrance = (delay: number, duration = 0.75, y = 18) => ({
    initial: reduceMotion ? false : { opacity: 0, y },
    animate: { opacity: 1, y: 0 },
    transition: {
      delay: reduceMotion ? 0 : delay,
      duration: reduceMotion ? 0 : duration,
      ease: easing.perfect,
    },
  });

  return (
    <section
      id="hero"
      ref={heroRef}
      className={
        reduceMotion
          ? "relative isolate min-h-[100svh] bg-[#FCFCFA] text-[#161616]"
          : "relative isolate h-[158svh] min-h-[940px] bg-[#FCFCFA] text-[#161616] max-[760px]:min-h-[920px]"
      }
    >
      <div
        className={
          reduceMotion
            ? "relative min-h-[100svh] overflow-hidden"
            : "sticky top-0 min-h-[100svh] overflow-hidden"
        }
      >
        <div className="hero-stage grid h-[100svh] grid-rows-[minmax(min-content,1fr)_clamp(230px,34svh,410px)] min-[761px]:grid-rows-[minmax(min-content,1fr)_clamp(300px,34svh,500px)]">
          <div
            ref={contentRef}
            className="hero-content grid min-h-0 grid-cols-1 gap-5 px-[clamp(24px,4vw,56px)] pb-5 pt-[104px] min-[761px]:gap-8 min-[761px]:pb-[clamp(30px,5vh,56px)] min-[761px]:pt-[clamp(122px,15vh,168px)] lg:grid-cols-12 lg:gap-x-8"
          >
            <motion.div
              className="hero-kicker max-w-[17rem] self-end will-change-transform lg:col-span-3 lg:pb-1"
              style={reduceMotion ? undefined : { y: supportingTextY }}
            >
              <motion.div
                {...entrance(0.06, 0.65)}
                className="home-ui text-[#626260]"
              >
                {content.kicker.map((line) => (
                  <p className="font-manrope" key={line}>
                    {line}
                  </p>
                ))}
              </motion.div>
            </motion.div>

            <div className="hero-primary grid min-h-0 min-w-0 content-between gap-6 min-[761px]:gap-10 lg:col-span-7 lg:col-start-6">
              <motion.h1
                className="home-display-heading max-w-[16ch] will-change-transform"
                style={reduceMotion ? undefined : { y: titleY }}
              >
                {content.headlineLines.map((line, index) => (
                  <span className="hero-title-line block" key={line}>
                    <motion.span
                      initial={reduceMotion ? false : { opacity: 0, y: "92%" }}
                      animate={{ opacity: 1, y: 0 }}
                      transition={{
                        delay: reduceMotion ? 0 : 0.13 + index * 0.11,
                        duration: reduceMotion ? 0 : 0.95,
                        ease: easing.perfect,
                      }}
                      className="block will-change-transform"
                    >
                      {line}
                    </motion.span>
                  </span>
                ))}
              </motion.h1>

              <motion.div
                className="will-change-transform"
                style={reduceMotion ? undefined : { y: supportingTextY }}
              >
                <motion.div
                  {...entrance(0.2, 0.82)}
                  className="hero-copy grid max-w-[43rem] gap-6 font-manrope text-[#626260] sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end sm:gap-9"
                >
                  <p className="home-body hidden max-w-[39rem] sm:block">
                    {content.bodyBeforeLink}
                    <a
                      className="underline decoration-[#626260]/40 underline-offset-4 hover:decoration-current"
                      href={advisoryHref}
                    >
                      {content.bodyLink}
                    </a>
                    {content.bodyAfterLink}
                  </p>
                  <p className="home-body max-w-[25rem] sm:hidden">
                    <a
                      className="underline decoration-[#626260]/40 underline-offset-4 hover:decoration-current"
                      href={advisoryHref}
                    >
                      {content.mobileBodyLink}
                    </a>
                    {content.mobileBodyAfterLink}
                  </p>
                  <a
                    href={localizeHref("/kontakt", locale)}
                    className="site-cta site-cta--quiet order-first whitespace-nowrap sm:order-none"
                  >
                    {content.cta}
                    <ArrowRight aria-hidden="true" className="h-3.5 w-3.5" />
                  </a>
                </motion.div>
              </motion.div>
            </div>
          </div>
        </div>

        <motion.div
          className="hero-media absolute inset-0 overflow-hidden bg-[#B8C8CA]"
          style={{
            clipPath: reduceMotion
              ? `inset(${mediaStart}% 0% 0% 0%)`
              : mediaClip,
          }}
        >
          <motion.div
            className="absolute inset-0"
            style={reduceMotion ? undefined : { scale: mediaScale, x: mediaX }}
          >
            <Image
              src="/images/hero-glacier-new.png"
              alt=""
              fill
              priority
              fetchPriority="high"
              sizes="100vw"
              className="object-cover object-[center_82%]"
            />
          </motion.div>
          <div
            aria-hidden="true"
            className="absolute inset-0 bg-[linear-gradient(180deg,rgba(16,38,44,0.02),rgba(9,26,32,0.2))]"
          />
          <div
            aria-hidden="true"
            className="pointer-events-none absolute inset-0 z-10 grid place-items-center overflow-hidden px-5 text-center"
          >
            <span className="font-manrope text-[clamp(2.45rem,7.8vw,8.8rem)] font-extralight uppercase leading-none tracking-[0.18em] text-white [text-shadow:0_2px_28px_rgba(8,24,29,0.16)]">
              Triodelab
            </span>
          </div>

          <div className="absolute inset-x-0 bottom-0 z-20 flex items-center justify-between px-[clamp(24px,4vw,56px)] py-5 font-label text-[0.68rem] font-light uppercase tracking-[0.12em] text-white [text-shadow:0_1px_8px_rgba(0,0,0,0.35)]">
            <span>{content.location}</span>
            <a
              href="#tjenester"
              onClick={(event) => {
                event.preventDefault();
                scrollToElement("tjenester");
              }}
              className="group inline-flex items-center gap-2 transition-opacity hover:opacity-70 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-white"
            >
              {content.scroll}
              <ChevronDown
                aria-hidden="true"
                className="hero-scroll-icon h-3.5 w-3.5"
              />
            </a>
          </div>
        </motion.div>
      </div>
    </section>
  );
}
