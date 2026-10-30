"use client";

import Image from "next/image";
import {
  AnimatePresence,
  motion,
  useScroll,
  useTransform,
} from "framer-motion";
import {
  ArrowLeft,
  ArrowRight,
  Code,
  Layers,
  Search,
  ShoppingBag,
  Sparkles,
  TrendingUp,
} from "@/components/icons";
import type { KeyboardEvent, PointerEvent } from "react";
import type { LucideIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { easing } from "@/lib/motion";
import type { Localized, Locale } from "@/lib/i18n/locales";
import { localizeHref } from "@/lib/i18n/routes";
import { usePrefersReducedMotion } from "@/lib/use-prefers-reduced-motion";
import { getServiceCardPointerTransform } from "./service-card-motion";
import { getNextServiceIndex } from "./service-selection";

type Service = {
  description: Localized<string>;
  href: string;
  icon: LucideIcon;
  image: string;
  imagePosition?: string;
  title: Localized<string>;
};

const services: Service[] = [
  {
    title: { nb: "Webløsninger", en: "Web solutions" },
    icon: Code,
    image: "/images/Triodelab-weblosning.jpg",
    imagePosition: "50% 62%",
    description: {
      nb: "Vi lager nettsider og webapplikasjoner som er raske, brukervennlige og bygget for å vare. Fra en enkel nettside til større, skreddersydde løsninger – vi tilpasser både teknologi og omfang etter det dere faktisk trenger.",
      en: "We build websites and web applications that are fast, easy to use and built to last. From a simple website to larger, tailored solutions – we match both technology and scope to what you actually need.",
    },
    href: "/tjenester/web-losninger",
  },
  {
    title: { nb: "Nettbutikk", en: "E-commerce" },
    icon: ShoppingBag,
    image: "/images/Nettbutikk.jpg",
    imagePosition: "45% 48%",
    description: {
      nb: "En god nettbutikk skal gjøre det enkelt å både handle og drive. Vi bygger og videreutvikler nettbutikker i Shopify, WooCommerce og andre plattformer, og velger løsningen som passer best for virksomheten deres.",
      en: "A good online store should make it easy both to shop and to run. We build and develop online stores on Shopify, WooCommerce and other platforms, and choose the solution that best fits your business.",
    },
    href: "/tjenester/nettbutikk",
  },
  {
    title: { nb: "Organisk trafikk", en: "Organic traffic" },
    icon: Search,
    image: "/images/OrganiskTraffikk.jpg",
    imagePosition: "60% 50%",
    description: {
      nb: "Bli synlig når potensielle kunder søker etter det dere tilbyr. Vi jobber med SEO, innhold og tekniske forbedringer for å få flere av de riktige menneskene inn på nettsiden deres.",
      en: "Be visible when potential customers search for what you offer. We work with SEO, content and technical improvements to bring more of the right people to your website.",
    },
    href: "/tjenester/organisk-trafikk",
  },
  {
    title: { nb: "Integrasjoner", en: "Integrations" },
    icon: Layers,
    image: "/images/Integrasjoner.jpg",
    imagePosition: "50% 55%",
    description: {
      nb: "Systemene deres bør snakke sammen. Vi kobler nettbutikker og digitale løsninger mot blant annet Visma, Fiken og Tripletex, slik at informasjon flyter automatisk. Når det er nyttig, kan vi også koble AI til arbeidsflyten.",
      en: "Your systems should talk to each other. We connect online stores and digital solutions to Visma, Fiken, Tripletex and more, so information flows automatically. Where it helps, we can also bring AI into the workflow.",
    },
    href: "/tjenester/integrasjoner",
  },
  {
    title: { nb: "Google + Meta Ads", en: "Google + Meta Ads" },
    icon: TrendingUp,
    image: "/images/Google-meta-Ads.jpg",
    imagePosition: "50% 90%",
    description: {
      nb: "Vi hjelper dere med annonsering på Google og Meta – fra oppsett og målgrupper til sporing og videre optimalisering. Målet er ikke bare flere klikk, men annonser som faktisk bidrar til resultater.",
      en: "We help you advertise on Google and Meta – from setup and audiences to tracking and ongoing optimisation. The goal is not just more clicks, but ads that actually contribute to results.",
    },
    href: "/tjenester/google-og-meta-ads",
  },
  {
    title: { nb: "KI og automatisering", en: "AI and automation" },
    icon: Sparkles,
    image: "/images/AI-automation.jpg",
    imagePosition: "50% 52%",
    description: {
      nb: "Vi bruker KI (AI) og automatisering til konkrete oppgaver: finne svar i egen kunnskap, håndtere henvendelser og få rutiner til å flyte bedre. Vi bygger med mennesker, kvalitet og kontroll i sentrum.",
      en: "We use AI and automation for concrete tasks: finding answers in your own knowledge, handling enquiries and making routines run more smoothly. We build with people, quality and control at the centre.",
    },
    href: "/tjenester/ki-og-automatisering",
  },
];

const copy: Localized<{
  explore: string;
  intro: string;
  next: string;
  pager: string;
  previous: string;
  tablist: string;
}> = {
  nb: {
    intro: "Velg en tjeneste for å utforske hva vi kan gjøre sammen.",
    tablist: "Tjenester",
    pager: "Bla mellom tjenester",
    previous: "Forrige tjeneste",
    next: "Neste tjeneste",
    explore: "Utforsk tjenesten",
  },
  en: {
    intro: "Choose a service to explore what we can do together.",
    tablist: "Services",
    pager: "Browse services",
    previous: "Previous service",
    next: "Next service",
    explore: "Explore the service",
  },
};

export function ServicesSection({ locale }: { locale: Locale }) {
  const content = copy[locale];
  const frameRef = useRef<HTMLDivElement>(null);
  const buttonRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const railRef = useRef<HTMLDivElement>(null);
  const pointerFrame = useRef<number | null>(null);
  const pendingPointer = useRef({ x: 0, y: 0 });
  const [activeIndex, setActiveIndex] = useState(0);
  const [pointer, setPointer] = useState({ x: 0, y: 0 });
  const [isHydrated, setIsHydrated] = useState(false);
  const prefersReducedMotion = usePrefersReducedMotion();
  const activeService = services[activeIndex];
  const { scrollYProgress } = useScroll({
    target: frameRef,
    offset: ["start end", "start 56px"],
  });
  const frameHeight = useTransform(
    scrollYProgress,
    (progress) => `${80 + progress * 20}svh`,
  );
  const frameWidth = useTransform(scrollYProgress, (progress) => {
    const remaining = 1 - progress;
    return `min(calc(${remaining * 0.8} * (min(100vw, 1440px) - clamp(40px, 8vw, 128px)) + ${progress * 100}vw), 92vw, calc(100vw - 32px))`;
  });

  useEffect(() => setIsHydrated(true), []);

  useEffect(() => {
    return () => {
      if (pointerFrame.current !== null) {
        window.cancelAnimationFrame(pointerFrame.current);
      }
    };
  }, []);

  function handleSectionPointerMove(event: PointerEvent<HTMLElement>) {
    if (prefersReducedMotion) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    pendingPointer.current = {
      x: ((event.clientX - bounds.left) / bounds.width - 0.5) * 2,
      y: ((event.clientY - bounds.top) / bounds.height - 0.5) * 2,
    };
    if (pointerFrame.current !== null) return;

    pointerFrame.current = window.requestAnimationFrame(() => {
      setPointer(pendingPointer.current);
      pointerFrame.current = null;
    });
  }

  function selectService(
    index: number,
    shouldFocus = false,
    shouldScroll = false,
  ) {
    setActiveIndex(index);
    if (shouldFocus) buttonRefs.current[index]?.focus();
    if (shouldScroll) {
      window.requestAnimationFrame(() => {
        const rail = railRef.current;
        const button = buttonRefs.current[index];
        if (!rail || !button) return;

        const railBounds = rail.getBoundingClientRect();
        const buttonBounds = button.getBoundingClientRect();
        const centeredOffset =
          buttonBounds.left -
          railBounds.left -
          (railBounds.width - buttonBounds.width) / 2;

        rail.scrollTo({
          left: rail.scrollLeft + centeredOffset,
          behavior: prefersReducedMotion ? "auto" : "smooth",
        });
      });
    }
  }

  function handleSelectorKeyDown(
    event: KeyboardEvent<HTMLButtonElement>,
    index: number,
  ) {
    const nextIndex = getNextServiceIndex(event.key, index, services.length);
    if (nextIndex === null) return;
    event.preventDefault();
    selectService(nextIndex, true, true);
  }

  return (
    <section
      id="tjenester"
      aria-labelledby="services-heading"
      className="overflow-x-clip bg-[#FCFCFA] py-[clamp(32px,6vw,100px)] text-[#161616]"
      onPointerMove={handleSectionPointerMove}
    >
      <div
        className="flex h-[100svh] min-h-[700px] justify-center"
        ref={frameRef}
      >
        <motion.div
          data-capabilities-frame
          className="relative isolate flex min-h-[700px] w-[92vw] max-w-[1440px] flex-col overflow-hidden rounded-lg bg-[#272B29] text-white motion-reduce:!h-[max(700px,100svh)] motion-reduce:!w-[min(92vw,calc(100vw-32px))] max-[760px]:w-[calc(100%-32px)]"
          style={{
            height: frameHeight,
            width: frameWidth,
          }}
        >
          {services.map((service, index) => (
            <div
              aria-hidden="true"
              className={`absolute inset-0 transition-opacity duration-500 ease-out motion-reduce:!transition-none ${
                index === activeIndex ? "opacity-100" : "opacity-0"
              }`}
              data-service-background={service.title.nb}
              key={service.href}
            >
              <Image
                alt=""
                className="object-cover object-center"
                fill
                sizes="(min-width: 1440px) 1440px, 92vw"
                src={service.image}
                style={{ objectPosition: service.imagePosition }}
              />
            </div>
          ))}
          <div
            aria-hidden="true"
            className={`absolute inset-0 ${
              activeIndex === 3
                ? "bg-[linear-gradient(180deg,rgba(15,18,18,0.48)_0%,rgba(15,18,18,0.52)_47%,rgba(15,18,18,0.76)_100%)]"
                : "bg-[linear-gradient(180deg,rgba(15,18,18,0.48)_0%,rgba(15,18,18,0.62)_47%,rgba(15,18,18,0.9)_100%)]"
            }`}
          />

          <div className="relative z-10 flex flex-1 flex-col px-[clamp(20px,4vw,58px)] py-[clamp(28px,4vw,56px)]">
            <p className="home-ui m-0 max-w-[20rem] self-end text-right !text-white/75">
              {content.intro}
            </p>

            <div
              aria-label={content.tablist}
              className="mt-[clamp(24px,4svh,48px)] flex shrink-0 items-start gap-3 overflow-x-auto pb-4 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
              ref={railRef}
              role="tablist"
            >
              {services.map((service, index) => {
                const isActive = index === activeIndex;
                const Icon = service.icon;
                const cardPointerTransform = getServiceCardPointerTransform(
                  index,
                  pointer,
                  Boolean(prefersReducedMotion),
                );
                const rowPointerTransform = {
                  x: cardPointerTransform.x * 0.2,
                  y: cardPointerTransform.y * 0.2,
                  rotate: cardPointerTransform.rotate * 0.35,
                  scale: 1 + (cardPointerTransform.scale - 1) * 0.1,
                };

                return (
                  <motion.button
                    animate={rowPointerTransform}
                    aria-controls="services-panel"
                    aria-selected={isActive}
                    className={`group relative flex min-w-[154px] flex-col justify-between overflow-hidden rounded-[18px] border p-[clamp(15px,1.8vw,24px)] text-left transition-[background-color,border-color,color,box-shadow] duration-500 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-white max-[760px]:flex-none max-[760px]:basis-[172px] ${
                      isActive
                        ? "h-[clamp(154px,23svh,218px)] flex-[1.4] border-white bg-white text-[#161616] shadow-[0_24px_70px_rgba(0,0,0,0.25)]"
                        : "h-[clamp(138px,20svh,190px)] flex-1 border-white/25 bg-[rgba(39,43,41,0.38)] text-white hover:bg-[rgba(39,43,41,0.5)]"
                    }`}
                    id={`services-tab-${index}`}
                    key={service.href}
                    disabled={!isHydrated}
                    onClick={() => selectService(index)}
                    onFocus={() => selectService(index)}
                    onKeyDown={(event) => handleSelectorKeyDown(event, index)}
                    onMouseEnter={() => selectService(index)}
                    ref={(element) => {
                      buttonRefs.current[index] = element;
                    }}
                    role="tab"
                    tabIndex={isActive ? 0 : -1}
                    type="button"
                    whileHover={
                      prefersReducedMotion ? undefined : { y: -6, scale: 1.02 }
                    }
                    transition={
                      prefersReducedMotion
                        ? { duration: 0 }
                        : {
                            type: "spring",
                            stiffness: 48,
                            damping: 17,
                            mass: 0.8,
                          }
                    }
                  >
                    <span className="inline-flex w-fit will-change-transform">
                      <Icon
                        aria-hidden="true"
                        className="size-[clamp(2rem,3vw,3.4rem)]"
                        strokeWidth={1.35}
                      />
                    </span>
                    <span className="grid gap-2">
                      <span className="font-label text-[0.65rem] font-light tracking-[0.1em] opacity-65">
                        {String(index + 1).padStart(2, "0")}
                      </span>
                      <span className="font-manrope text-[clamp(0.9rem,1.13vw,1.15rem)] font-light leading-[1.15] tracking-[-0.035em]">
                        {service.title[locale]}
                      </span>
                    </span>
                  </motion.button>
                );
              })}
            </div>

            <div className="relative my-auto flex flex-col items-center gap-8 py-[clamp(28px,5svh,64px)] text-center">
              <AnimatePresence initial={false} mode="wait">
                <motion.div
                  animate={{ opacity: 1, y: 0 }}
                  aria-labelledby={`services-tab-${activeIndex}`}
                  aria-live="polite"
                  className="mx-auto max-w-[47rem]"
                  exit={
                    prefersReducedMotion ? undefined : { opacity: 0, y: -12 }
                  }
                  id="services-panel"
                  initial={prefersReducedMotion ? false : { opacity: 0, y: 18 }}
                  key={activeService.href}
                  role="tabpanel"
                  transition={{
                    duration: prefersReducedMotion ? 0 : 0.42,
                    ease: easing.perfect,
                  }}
                >
                  <h2 id="services-heading" className="home-section-heading">
                    {activeService.title[locale]}
                  </h2>
                  <p className="home-body mx-auto mb-0 mt-5 max-w-[41rem] !text-white/80 text-pretty">
                    {activeService.description[locale]}
                  </p>
                </motion.div>
              </AnimatePresence>

              <div
                className="flex items-center justify-center gap-3"
                role="group"
                aria-label={content.pager}
              >
                <button
                  aria-label={content.previous}
                  className="grid size-11 place-items-center rounded-lg border border-white/30 bg-white/15 transition-colors hover:bg-white/30 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-white"
                  disabled={!isHydrated}
                  onClick={() =>
                    selectService(
                      (activeIndex - 1 + services.length) % services.length,
                      false,
                      true,
                    )
                  }
                  type="button"
                >
                  <ArrowLeft aria-hidden="true" className="size-4" />
                </button>
                <a
                  className="site-cta site-cta--light"
                  href={localizeHref(activeService.href, locale)}
                >
                  {content.explore}
                  <ArrowRight aria-hidden="true" className="h-4 w-4" />
                </a>
                <button
                  aria-label={content.next}
                  className="grid size-11 place-items-center rounded-lg border border-white/30 bg-white/15 transition-colors hover:bg-white/30 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-white"
                  disabled={!isHydrated}
                  onClick={() =>
                    selectService(
                      (activeIndex + 1) % services.length,
                      false,
                      true,
                    )
                  }
                  type="button"
                >
                  <ArrowRight aria-hidden="true" className="size-4" />
                </button>
              </div>
            </div>
          </div>
        </motion.div>
      </div>
    </section>
  );
}
