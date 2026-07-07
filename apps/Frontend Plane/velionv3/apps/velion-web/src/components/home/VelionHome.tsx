"use client";

import { useEffect, useRef, useState } from "react";
import { Footer } from "@/components/core/footer/Footer";
import { Navbar } from "@/components/core/navbar/Navbar";
import { MenuModal } from "@/components/ui/MenuModal";
import { HeroSection } from "./sections/HeroSection";
import { BrandLogosSection } from "./sections/BrandLogosSection";
import { FeatureCardsSection } from "./sections/FeatureCardsSection";
import { ProductLoopSection } from "./sections/ProductLoopSection";
import { Testimonials } from "./sections/TestimonialSection";
import { LayerSection } from "./sections/layer-section";
import { SensesSection } from "./sections/SensesSection";
import { PreFooterStatementSection } from "./sections/PreFooterStatementSection";

export function VelionHome() {
	const homeRef = useRef<HTMLDivElement>(null);
	const heroFogScrolledRef = useRef(false);
	const lastFogScrollY = useRef(0);
	const [isMenuOpen, setIsMenuOpen] = useState(false);
	const [isScrolled, setIsScrolled] = useState(false);
	const [isHeroFogScrolled, setIsHeroFogScrolled] = useState(false);

	useEffect(() => {
		let frame: number | null = null;
		lastFogScrollY.current = window.scrollY;

		const updateScrollState = () => {
			const nextScrollY = window.scrollY;
			const delta = nextScrollY - lastFogScrollY.current;
			const reduceMotion = window.matchMedia(
				"(prefers-reduced-motion: reduce)",
			).matches;
			const fogTrigger = Math.max(246, window.innerHeight * 0.66);
			let nextHeroFogScrolled = heroFogScrolledRef.current;

			if (reduceMotion) {
				nextHeroFogScrolled = false;
			} else if (Math.abs(delta) > 1) {
				if (delta > 0 && nextScrollY > fogTrigger) {
					nextHeroFogScrolled = true;
				}

				if (delta < 0) {
					nextHeroFogScrolled = false;
				}
			}

			heroFogScrolledRef.current = nextHeroFogScrolled;

			setIsHeroFogScrolled((current) =>
				current === nextHeroFogScrolled ? current : nextHeroFogScrolled,
			);

			setIsScrolled((current) => {
				const next = reduceMotion
					? nextScrollY > fogTrigger
					: nextHeroFogScrolled;
				return current === next ? current : next;
			});

			lastFogScrollY.current = nextScrollY;
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
		let animationContext: { revert: () => void } | undefined;
		let disposed = false;

		async function setupFooterParallax() {
			const root = homeRef.current;

			if (!root) {
				return;
			}

			const footer = root.querySelector<HTMLElement>(
				"[data-footer-parallax]",
			);
			const footerMedia = root.querySelector<HTMLElement>(
				"[data-footer-parallax-media]",
			);

			if (!footer || !footerMedia) {
				return;
			}

			const [{ default: gsap }, { ScrollTrigger }] = await Promise.all([
				import("gsap"),
				import("gsap/ScrollTrigger"),
			]);

			if (disposed) {
				return;
			}

			gsap.registerPlugin(ScrollTrigger);

			animationContext = gsap.context(() => {
				const reduceMotion = window.matchMedia(
					"(prefers-reduced-motion: reduce)",
				).matches;

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
			}, root);

			window.requestAnimationFrame(() => ScrollTrigger.refresh());
		}

		setupFooterParallax();

		return () => {
			disposed = true;
			animationContext?.revert();
		};
	}, []);

	useEffect(() => {
		const root = homeRef.current;

		if (!root) {
			return;
		}

		const fadeTargets = Array.from(
			root.querySelectorAll<HTMLElement>("[data-fade-out-top]"),
		);
		let frame: number | null = null;

		const updateTextFades = () => {
			for (const element of fadeTargets) {
				const rect = element.getBoundingClientRect();
				const maskY = -rect.top + 90;
				const position = `0px ${maskY.toFixed(3)}px`;

				element.style.setProperty("mask-position", position);
				element.style.setProperty("-webkit-mask-position", position);
			}

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
			const reduceMotion = window.matchMedia(
				"(prefers-reduced-motion: reduce)",
			).matches;

			destination.scrollIntoView({
				behavior: reduceMotion ? "auto" : "smooth",
				block: "start",
			});
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
				isOnDark={false}
				isScrolled={isScrolled}
				onOpen={() => setIsMenuOpen(true)}
			/>

			<MenuModal onClose={() => setIsMenuOpen(false)} open={isMenuOpen} />

			<main className="relative z-[2] bg-background">
				<HeroSection />
				<div
					aria-hidden="true"
					className="pointer-events-none relative z-20 h-0"
				>
					<div
						className={[
							"velion-hima-fog",
							isHeroFogScrolled ? "scrolled" : "",
						]
							.filter(Boolean)
							.join(" ")}
					/>
				</div>
				<BrandLogosSection />
				<FeatureCardsSection />
				<SensesSection />
				<ProductLoopSection />
				<Testimonials />
				<LayerSection />
				<PreFooterStatementSection />
			</main>

			<div className="relative z-[1] overflow-clip bg-velion-footer-bg">
				<Footer />
			</div>
		</div>
	);
}

export default VelionHome;
