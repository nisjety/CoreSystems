"use client";

import { useEffect, useRef, useState } from "react";
import { Navbar } from "@/components/core/navbar/Navbar";
import { MenuModal } from "@/components/ui/MenuModal";
import { Footer } from "@/components/core/footer/Footer";
import { HeroSection } from "./sections/HeroSection";
import { BrandLogosSection } from "./sections/BrandLogosSection";
import { FeatureCardsSection } from "./sections/FeatureCardsSection"
import { PreFooterStatementSection } from "./sections/PreFooterStatementSection";


export function VelionHome() {
  const homeRef = useRef<HTMLDivElement>(null);
  const [isMenuOpen, setIsMenuOpen] = useState(false);
  const [isNavOnDark, setIsNavOnDark] = useState(false);
  const [isScrolled, setIsScrolled] = useState(false);

  useEffect(() => {
    let frame: number | null = null;

    const updateScrollState = () => {
      setIsScrolled((current) => {
        const next = window.scrollY > window.innerHeight * 0.86;
        return current === next ? current : next;
      });

      setIsNavOnDark((current) => {
        const prefooter = document.querySelector<HTMLElement>("[data-prefooter-scroll]");

        if (!prefooter) {
          return current ? false : current;
        }

        const rect = prefooter.getBoundingClientRect();
        const travel = Math.max(rect.height - window.innerHeight, 1);
        const progress = Math.min(Math.max(-rect.top / travel, 0), 1);
        const next = rect.top <= 0 && rect.bottom >= window.innerHeight && progress >= 0.32;

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
        const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
        const hero = homeRef.current?.querySelector<HTMLElement>("[data-hero-parallax]");
        const media = homeRef.current?.querySelector<HTMLElement>("[data-hero-parallax-media]");
        const content = homeRef.current?.querySelector<HTMLElement>("[data-hero-parallax-content]");
        const productLoop = homeRef.current?.querySelector<HTMLElement>("[data-product-loop]");
        const prefooter = homeRef.current?.querySelector<HTMLElement>("[data-prefooter-scroll]");
        const footerReveal = homeRef.current?.querySelector<HTMLElement>("[data-footer-reveal]");
        const footer = footerReveal?.querySelector<HTMLElement>("[data-footer-parallax]");
        const footerMedia = footer?.querySelector<HTMLElement>("[data-footer-parallax-media]");
        const footerContent = footer?.querySelector<HTMLElement>("[data-footer-parallax-content]");
        const footerBrand = footer?.querySelector<HTMLElement>("[data-footer-parallax-brand]");

        if (hero && media && content) {
          gsap.set([media, content], { yPercent: 0, force3D: true });

          if (!reduceMotion) {
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
          }
        }

        if (productLoop) {
          const steps = Array.from(productLoop.querySelectorAll<HTMLElement>("[data-product-step]"));
          const panels = Array.from(productLoop.querySelectorAll<HTMLElement>("[data-product-panel]"));

          if (steps.length > 0 && panels.length > 0) {
            const setActiveStep = (nextIndex: number) => {
              const activeIndex = Math.min(Math.max(nextIndex, 0), steps.length - 1);

              productLoop.style.setProperty("--product-active-index", String(activeIndex));

              steps.forEach((step, index) => {
                step.classList.toggle("is-active", index === activeIndex);
              });

              panels.forEach((panel, index) => {
                panel.classList.toggle("is-active", index === activeIndex);

                if (!reduceMotion) {
                  gsap.to(panel, {
                    autoAlpha: index === activeIndex ? 1 : 0,
                    duration: 0.42,
                    ease: "power2.out",
                    overwrite: "auto",
                    scale: index === activeIndex ? 1 : 0.965,
                    y: index === activeIndex ? 0 : 34,
                  });
                }
              });
            };

            if (reduceMotion) {
              setActiveStep(0);
            } else {
              gsap.set(panels, { autoAlpha: 0, scale: 0.965, y: 34, force3D: true });
              gsap.set(panels[0], { autoAlpha: 1, scale: 1, y: 0 });
              setActiveStep(0);

              ScrollTrigger.create({
                trigger: productLoop,
                start: "top top",
                end: "bottom bottom",
                scrub: true,
                id: "velion-product-loop",
                invalidateOnRefresh: true,
                onUpdate: (self) => {
                  const progress = Math.min(Math.max(self.progress, 0), 1);
                  const activeIndex = Math.min(steps.length - 1, Math.floor(progress * steps.length));

                  productLoop.style.setProperty("--product-progress", progress.toFixed(4));
                  setActiveStep(activeIndex);
                },
              });
            }
          }
        }

        if (prefooter) {
          const lightPhrase = prefooter.querySelector<HTMLElement>("[data-prefooter-light-phrase]");
          const darkPanel = prefooter.querySelector<HTMLElement>("[data-prefooter-dark-panel]");
          const darkPhrase = prefooter.querySelector<HTMLElement>("[data-prefooter-dark-phrase]");
          const finalStatement = prefooter.querySelector<HTMLElement>("[data-prefooter-final]");

          if (lightPhrase && darkPanel && darkPhrase && finalStatement) {
            if (reduceMotion) {
              gsap.set(darkPanel, { autoAlpha: 1 });
              gsap.set([lightPhrase, darkPhrase], { autoAlpha: 0 });
              gsap.set(finalStatement, { autoAlpha: 1, y: 0 });
            } else {
              gsap.set(lightPhrase, { autoAlpha: 1, scale: 1 });
              gsap.set(darkPanel, { autoAlpha: 0 });
              gsap.set(darkPhrase, { autoAlpha: 0, scale: 1 });
              gsap.set(finalStatement, { autoAlpha: 0, y: () => window.innerHeight * 0.22 });

              gsap
                .timeline({
                  scrollTrigger: {
                    trigger: prefooter,
                    start: "top top",
                    end: "bottom bottom",
                    scrub: 0.45,
                    id: "velion-prefooter-scroll-shift",
                    invalidateOnRefresh: true,
                  },
                })
                .to(lightPhrase, { autoAlpha: 0, scale: 0.995, ease: "none", duration: 0.16 }, 0.16)
                .to(darkPanel, { autoAlpha: 1, ease: "none", duration: 0.22 }, 0.19)
                .to(darkPhrase, { autoAlpha: 1, scale: 1, ease: "none", duration: 0.18 }, 0.28)
                .to(
                  darkPhrase,
                  {
                    y: () => -window.innerHeight * 0.48,
                    autoAlpha: 0.92,
                    ease: "none",
                    duration: 0.22,
                  },
                  0.52,
                )
                .to(finalStatement, { autoAlpha: 1, y: 0, ease: "none", duration: 0.3 }, 0.56);
            }
          }
        }

        if (footerReveal && footer && footerMedia && footerContent) {
          if (reduceMotion) {
            const footerTargets = [footer, footerMedia, footerContent, footerBrand].filter(
              (target): target is HTMLElement => Boolean(target),
            );

            gsap.set(footerTargets, {
              autoAlpha: 1,
              yPercent: 0,
              y: 0,
            });
          } else {
            gsap.set(footerMedia, { yPercent: -44, force3D: true });
            gsap.set(footerContent, { autoAlpha: 0.94, yPercent: 18, force3D: true });

            const footerScrollTrigger = {
              trigger: footerReveal,
              start: "top bottom",
              end: "top top",
              scrub: true,
              invalidateOnRefresh: true,
            } as const;

            gsap.to(footerMedia, {
              yPercent: 0,
              ease: "none",
              overwrite: "auto",
              scrollTrigger: {
                ...footerScrollTrigger,
                id: "velion-footer-media-parallax",
              },
            });

            gsap.to(footerContent, {
              autoAlpha: 1,
              yPercent: 0,
              ease: "none",
              overwrite: "auto",
              scrollTrigger: {
                ...footerScrollTrigger,
                id: "velion-footer-content-parallax",
              },
            });
          }
        }

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
    <div
      className="min-h-screen bg-background text-velion-text [--velion-edge:clamp(56px,5.55vw,208px)] [--velion-page-pad:clamp(24px,4vw,56px)] [--velion-section-gap:clamp(86px,8.9vw,330px)] [--velion-section-vpad:clamp(96px,15vh,180px)]"
      ref={homeRef}
    >
      <Navbar
        isMenuOpen={isMenuOpen}
        isOnDark={isNavOnDark}
        isScrolled={isScrolled}
        onOpen={() => setIsMenuOpen(true)}
      />
      <MenuModal onClose={() => setIsMenuOpen(false)} open={isMenuOpen} />

      <main className="relative z-[2] bg-background">
        <HeroSection />
        <BrandLogosSection />
        <FeatureCardsSection />
        <PreFooterStatementSection />
      </main>

      <div className="relative z-[1] overflow-clip bg-velion-footer-bg">
	<Footer />
</div>
    </div>
  );
}
