"use client";

import { useEffect, useRef, useState } from "react";
import { Footer } from "@/components/core/footer/Footer";
import { Navbar } from "@/components/core/navbar/Navbar";
import { MenuModal } from "@/components/ui/modals/MenuModal";
import { DetailGallerySection } from "./sections/DetailGallerySection";
import { ExcellenceSection } from "./sections/ExcellenceSection";
import { FeatureCardsSection } from "./sections/FeatureCardsSection";
import { HeritageSection } from "./sections/HeritageSection";
import { HeroSection } from "./sections/HeroSection";
import { PartnershipSection } from "./sections/PartnershipSection";
import { SensesSection } from "./sections/SensesSection";
import { TechnologySection } from "./sections/TechnologySection";

export function VelionHome() {
  const homeRef = useRef<HTMLDivElement>(null);
  const [isMenuOpen, setIsMenuOpen] = useState(false);
  const [isScrolled, setIsScrolled] = useState(false);

  useEffect(() => {
    let frame: number | null = null;

    const updateScrollState = () => {
      setIsScrolled((current) => {
        const next = window.scrollY > 80;
        return current === next ? current : next;
      });
      frame = null;
    };

    const onScroll = () => {
      if (frame !== null) {
        return;
      }
      frame = window.requestAnimationFrame(updateScrollState);
    };

    updateScrollState();
    window.addEventListener("scroll", onScroll, { passive: true });

    return () => {
      if (frame !== null) {
        window.cancelAnimationFrame(frame);
      }
      window.removeEventListener("scroll", onScroll);
    };
  }, []);

  useEffect(() => {
    let animationContext: { revert: () => void } | null = null;
    let disposed = false;

    const setupHeroParallax = async () => {
      const [{ default: gsap }, { ScrollTrigger }] = await Promise.all([
        import("gsap"),
        import("gsap/ScrollTrigger"),
      ]);

      if (disposed || !homeRef.current) {
        return;
      }

      gsap.registerPlugin(ScrollTrigger);

      animationContext = gsap.context(() => {
        const hero = homeRef.current?.querySelector<HTMLElement>("[data-hero-parallax]");
        const media = homeRef.current?.querySelector<HTMLElement>("[data-hero-parallax-media]");
        const content = homeRef.current?.querySelector<HTMLElement>("[data-hero-parallax-content]");

        if (!hero || !media || !content) {
          return;
        }

        gsap.set([media, content], { yPercent: 0, force3D: true });

        if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
          return;
        }

        const scrollTrigger = {
          trigger: hero,
          start: "top top",
          end: "bottom top",
          scrub: true,
          invalidateOnRefresh: true,
        } as const;

        gsap.to(media, {
          yPercent: 80,
          ease: "none",
          overwrite: "auto",
          scrollTrigger: {
            ...scrollTrigger,
            id: "velion-hero-media-parallax",
          },
        });

        gsap.to(content, {
          yPercent: 40,
          ease: "none",
          overwrite: "auto",
          scrollTrigger: {
            ...scrollTrigger,
            id: "velion-hero-content-parallax",
          },
        });

        window.requestAnimationFrame(() => ScrollTrigger.refresh());
      }, homeRef);
    };

    setupHeroParallax();

    return () => {
      disposed = true;
      animationContext?.revert();
    };
  }, []);

  useEffect(() => {
    const observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            entry.target.classList.add("is-visible");
          }
        });
      },
      { rootMargin: "0px 0px -12% 0px", threshold: 0.18 },
    );

    document.querySelectorAll<HTMLElement>(".velion-reveal").forEach((element) => {
      observer.observe(element);
    });

    return () => {
      observer.disconnect();
    };
  }, []);

  useEffect(() => {
    let frame: number | null = null;

    const updateTextFades = () => {
      document.querySelectorAll<HTMLElement>("[data-fade-out-top]").forEach((element) => {
        const rect = element.getBoundingClientRect();
        const maskY = -rect.top + 90;
        const position = `0px ${maskY.toFixed(3)}px`;

        element.style.setProperty("mask-position", position);
        element.style.setProperty("-webkit-mask-position", position);
      });

      frame = null;
    };

    const schedule = () => {
      if (frame !== null) {
        return;
      }
      frame = window.requestAnimationFrame(updateTextFades);
    };

    updateTextFades();
    window.addEventListener("scroll", schedule, { passive: true });
    window.addEventListener("resize", schedule);

    return () => {
      if (frame !== null) {
        window.cancelAnimationFrame(frame);
      }
      window.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
    };
  }, []);

  useEffect(() => {
    document.body.classList.toggle("velion-menu-open", isMenuOpen);

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setIsMenuOpen(false);
      }
    };

    window.addEventListener("keydown", onKeyDown);

    return () => {
      document.body.classList.remove("velion-menu-open");
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [isMenuOpen]);

  useEffect(() => {
    const onDocumentClick = (event: MouseEvent) => {
      const target = event.target;
      if (!(target instanceof Element)) {
        return;
      }

      const anchor = target.closest<HTMLAnchorElement>('a[href^="#"]');
      const hash = anchor?.getAttribute("href");

      if (!anchor || !hash || hash === "#") {
        return;
      }

      const destination = document.querySelector(hash);
      if (!destination) {
        return;
      }

      event.preventDefault();
      destination.scrollIntoView({ behavior: "smooth", block: "start" });
      window.history.replaceState(null, "", hash);
    };

    document.addEventListener("click", onDocumentClick);

    return () => {
      document.removeEventListener("click", onDocumentClick);
    };
  }, []);

  return (
    <div className="velion-home" ref={homeRef}>
      <Navbar isMenuOpen={isMenuOpen} isScrolled={isScrolled} onOpen={() => setIsMenuOpen(true)} />
      <MenuModal onClose={() => setIsMenuOpen(false)} open={isMenuOpen} />

      <main>
        <HeroSection />
        <FeatureCardsSection />
        <PartnershipSection />
        <SensesSection />
        <DetailGallerySection />
        <ExcellenceSection />
        <TechnologySection />
        <HeritageSection />
      </main>

      <Footer />
    </div>
  );
}
