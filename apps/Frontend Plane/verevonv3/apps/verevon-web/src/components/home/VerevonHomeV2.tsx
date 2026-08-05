"use client";

import { useEffect, useRef, useState } from "react";
import { Footer } from "@/components/core/footer/Footer";
import { Navbar } from "@/components/core/navbar/Navbar";
import { MenuModal } from "@/components/ui/MenuModal";
import { BrandLogosSection } from "./sections/BrandLogosSection";
import { LayerSection } from "./sections/layer-section";
import { PreFooterStatementSection } from "./sections/PreFooterStatementSection";
import { FeatureCardsSectionV2 } from "./v2/FeatureCardsSectionV2";
import { HeroSectionV2 } from "./v2/HeroSectionV2";
import { ObservabilitySectionV2 } from "./v2/ObservabilitySectionV2";
import { Preloader } from "./v2/Preloader";
import { ProductLoopSectionV2 } from "./v2/ProductLoopSectionV2";
import { ProofSectionV2 } from "./v2/ProofSectionV2";
import { SensesSectionV2 } from "./v2/SensesSectionV2";
import { ShowcaseShortSectionV2 } from "./v2/ShowcaseShortSectionV2";
import { TechnologySectionV2 } from "./v2/TechnologySectionV2";
import { WordmarkSectionV2 } from "./v2/WordmarkSectionV2";

/**
 * VerevonHomeV2 — calmer, premium homepage variant.
 *
 * Same wedge story as V1 but with the cinematic choreography removed: one
 * ambient hero video, IntersectionObserver reveals, shared ArrowButton CTAs,
 * a proof row and an oversized wordmark. Reuses (does not modify) the Navbar,
 * MenuModal, pre-footer and Footer. The only GSAP island is the optional
 * short-form product showcase section.
 */
export function VerevonHomeV2() {
	const homeRef = useRef<HTMLDivElement>(null);
	const [isMenuOpen, setIsMenuOpen] = useState(false);
	const [isScrolled, setIsScrolled] = useState(false);
	const [isShowcaseActive, setIsShowcaseActive] = useState(false);

	useEffect(() => {
		let frame: number | null = null;

		const update = () => {
			setIsScrolled((current) => {
				const next = window.scrollY > window.innerHeight * 0.86;
				return current === next ? current : next;
			});
			frame = null;
		};

		const onScroll = () => {
			if (frame !== null) {
				return;
			}
			frame = window.requestAnimationFrame(update);
		};

		update();
		window.addEventListener("scroll", onScroll, { passive: true });
		window.addEventListener("resize", onScroll);

		return () => {
			if (frame !== null) {
				window.cancelAnimationFrame(frame);
			}
			window.removeEventListener("scroll", onScroll);
			window.removeEventListener("resize", onScroll);
		};
	}, []);

	useEffect(() => {
		document.body.classList.toggle("verevon-menu-open", isMenuOpen);

		const onKeyDown = (event: KeyboardEvent) => {
			if (event.key === "Escape") {
				setIsMenuOpen(false);
			}
		};

		window.addEventListener("keydown", onKeyDown);

		return () => {
			document.body.classList.remove("verevon-menu-open");
			window.removeEventListener("keydown", onKeyDown);
		};
	}, [isMenuOpen]);

	useEffect(() => {
		const root = homeRef.current;
		const showcase = root?.querySelector("#showcase-short");

		if (!showcase) {
			return;
		}

		const observer = new IntersectionObserver(
			([entry]) => {
				setIsShowcaseActive(entry.isIntersecting && entry.intersectionRatio > 0.55);
			},
			{ threshold: [0, 0.35, 0.55, 0.75] },
		);

		observer.observe(showcase);

		return () => observer.disconnect();
	}, []);

	useEffect(() => {
		const onClick = (event: MouseEvent) => {
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

		document.addEventListener("click", onClick);

		return () => document.removeEventListener("click", onClick);
	}, []);

	// V1 text-fade: dissolve [data-fade-out-top] text as it scrolls off the top.
	useEffect(() => {
		const root = homeRef.current;

		if (!root) {
			return;
		}

		const fadeTargets = Array.from(
			root.querySelectorAll<HTMLElement>("[data-fade-out-top]"),
		);
		let frame: number | null = null;

		const update = () => {
			for (const element of fadeTargets) {
				const maskY = -element.getBoundingClientRect().top + 90;
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
			frame = window.requestAnimationFrame(update);
		};

		update();
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

	return (
		<div
			className="min-h-screen bg-background text-verevon-text [--verevon-edge:clamp(56px,5.55vw,208px)] [--verevon-page-pad:clamp(24px,4vw,56px)] [--verevon-section-vpad:clamp(88px,12vh,156px)]"
			ref={homeRef}
		>
			<Preloader />

			<Navbar
				isHidden={isShowcaseActive && !isMenuOpen}
				isMenuOpen={isMenuOpen}
				isOnDark={false}
				isScrolled={isScrolled}
				onOpen={() => setIsMenuOpen(true)}
			/>

			<MenuModal onClose={() => setIsMenuOpen(false)} open={isMenuOpen} />

			<main className="relative z-[2] bg-background">
				<HeroSectionV2 />
				<BrandLogosSection />
				<ShowcaseShortSectionV2 />
				<FeatureCardsSectionV2 />
				<SensesSectionV2 />
				<TechnologySectionV2 />
				<ObservabilitySectionV2 />
				<ProductLoopSectionV2 />
				<ProofSectionV2 />
				<LayerSection />
				<WordmarkSectionV2 />
				<PreFooterStatementSection />
			</main>

			<div className="relative z-[1] overflow-clip bg-verevon-footer-bg">
				<Footer />
			</div>
		</div>
	);
}

export default VerevonHomeV2;
