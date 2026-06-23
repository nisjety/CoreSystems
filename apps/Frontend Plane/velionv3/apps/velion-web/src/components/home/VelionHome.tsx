"use client";

import { useEffect, useRef, useState } from "react";
import { Footer } from "@/components/core/footer/Footer";
import { Navbar } from "@/components/core/navbar/Navbar";
import { MenuModal } from "@/components/ui/MenuModal";
import { BrandLogosSection } from "./sections/BrandLogosSection";
import { FeatureCardsSection } from "./sections/FeatureCardsSection";
import { HeroSection } from "./sections/HeroSection";
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
				const navProbeY = Math.min(96, window.innerHeight * 0.12);

				const darkSectionIsActive = Array.from(
					document.querySelectorAll<HTMLElement>("[data-nav-dark-section]"),
				).some((section) => {
					const rect = section.getBoundingClientRect();

					return rect.top <= navProbeY && rect.bottom >= navProbeY;
				});

				return current === darkSectionIsActive ? current : darkSectionIsActive;
			});

			frame = null;
		};

		const onScrollOrResize = () => {
			if (frame !== null) {
				return;
			}

			frame = window.requestAnimationFrame(updateScrollState);
		};

		updateScrollState();

		window.addEventListener("scroll", onScrollOrResize, { passive: true });
		window.addEventListener("resize", onScrollOrResize);

		return () => {
			if (frame !== null) {
				window.cancelAnimationFrame(frame);
			}

			window.removeEventListener("scroll", onScrollOrResize);
			window.removeEventListener("resize", onScrollOrResize);
		};
	}, []);

	useEffect(() => {
		let animationContext: { revert: () => void } | null = null;
		let disposed = false;

		async function setupParallax() {
			const [{ default: gsap }, { ScrollTrigger }] = await Promise.all([
				import("gsap"),
				import("gsap/ScrollTrigger"),
			]);

			if (disposed || !homeRef.current) {
				return;
			}

			gsap.registerPlugin(ScrollTrigger);

			animationContext = gsap.context(() => {
				const reduceMotion = window.matchMedia(
					"(prefers-reduced-motion: reduce)",
				).matches;

				const hero =
					homeRef.current?.querySelector<HTMLElement>("[data-hero-parallax]");
				const heroMedia = homeRef.current?.querySelector<HTMLElement>(
					"[data-hero-parallax-media]",
				);
				const heroContent = homeRef.current?.querySelector<HTMLElement>(
					"[data-hero-parallax-content]",
				);

				if (hero && heroMedia && heroContent) {
					gsap.set([heroMedia, heroContent], {
						yPercent: 0,
						force3D: true,
					});

					if (!reduceMotion) {
						const heroScrollTrigger = {
							trigger: hero,
							start: "top top",
							end: "bottom top",
							scrub: true,
							invalidateOnRefresh: true,
						} as const;

						gsap.to(heroMedia, {
							yPercent: 80,
							ease: "none",
							overwrite: "auto",
							scrollTrigger: {
								...heroScrollTrigger,
								id: "velion-hero-media-parallax",
							},
						});

						gsap.to(heroContent, {
							yPercent: 40,
							ease: "none",
							overwrite: "auto",
							scrollTrigger: {
								...heroScrollTrigger,
								id: "velion-hero-content-parallax",
							},
						});
					}
				}

				const footer =
					homeRef.current?.querySelector<HTMLElement>("[data-footer-parallax]");
				const footerMedia = homeRef.current?.querySelector<HTMLElement>(
					"[data-footer-parallax-media]",
				);

				if (footer && footerMedia) {
					gsap.set(footerMedia, {
						yPercent: reduceMotion ? 0 : -8,
						force3D: true,
					});

					if (!reduceMotion) {
						gsap.to(footerMedia, {
							yPercent: 0,
							ease: "none",
							scrollTrigger: {
								trigger: footer,
								start: "top bottom",
								end: "bottom bottom",
								scrub: true,
								id: "velion-footer-parallax",
								invalidateOnRefresh: true,
							},
						});
					}
				}

				window.requestAnimationFrame(() => ScrollTrigger.refresh());
			}, homeRef);
		}

		setupParallax();

		return () => {
			disposed = true;
			animationContext?.revert();
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

export default VelionHome;