"use client";

import { useEffect, useRef } from "react";
import { HeroParallax } from "./HeroParallax";
import { SignalPathLayer } from "./SignalPathLayer";
import { ArrowButton } from "@/components/ui/ArrowButton";
import { usePrefersReducedMotion } from "@/shared/hooks/usePrefersReducedMotion";
import styles from "./HeroSection.module.css";

/**
 * HeroSection — calm, ambient, fast.
 *
 * One continuous video loop as ambient background (TYDE/Terminal pattern) —
 * NO staged reveal choreography (video→image→copy stagger). All copy is
 * server-rendered and visible at first paint, so the LCP element paints
 * immediately instead of being gated by an animation timeline.
 *
 * Above the fold: one claim, one quiet line, and one small action. Let the
 * media carry the atmosphere; keep explanatory copy for the sections below.
 */
export function HeroSection() {
	const videoRef = useRef<HTMLVideoElement>(null);
	const prefersReducedMotion = usePrefersReducedMotion();

	useEffect(() => {
		const video = videoRef.current;
		if (!video) {
			return;
		}

		if (prefersReducedMotion) {
			video.pause();
			return;
		}

		let visible = false;
		const syncPlayback = () => {
			if (!visible || document.hidden) video.pause();
			else void video.play().catch(() => {});
		};
		const observer = new IntersectionObserver(([entry]) => {
			visible = entry.isIntersecting;
			syncPlayback();
		});
		observer.observe(video);
		document.addEventListener("visibilitychange", syncPlayback);
		return () => {
			observer.disconnect();
			document.removeEventListener("visibilitychange", syncPlayback);
			video.pause();
		};
	}, [prefersReducedMotion]);

	return (
		<section
			className="relative isolate flex h-[100dvh] min-h-svh flex-col justify-end overflow-hidden bg-verevon-h-ink-warm text-verevon-c-white"
			data-hero-parallax=""
			id="top"
		>
			<div
				aria-hidden="true"
				className={`absolute inset-0 z-0 overflow-hidden ${styles.film}`}
				data-hero-parallax-media=""
			>
				<video
					className="size-full object-cover object-[91%_center] max-[760px]:object-[76%_center]"
					loop
					muted
					playsInline
					poster="/verevon-vibe/hero-verevon-poster.jpg"
					preload="metadata"
					ref={videoRef}
				>
					<source
						src="/verevon-vibe/hero-verevon-1080.mp4"
						type="video/mp4"
					/>
				</video>
				<div className={styles.filmShade} />
			</div>

			<SignalPathLayer variant="hero" />
			<HeroParallax />

			<div
				className="relative z-[1] mx-auto flex h-full min-h-svh w-full flex-col justify-center px-[var(--verevon-edge)] py-[clamp(72px,10vh,120px)] max-[760px]:px-[var(--verevon-page-pad)]"
				data-hero-parallax-copy=""
			>
				<div
					className="max-w-[39rem] -translate-x-[6%] translate-y-[7%] max-[760px]:translate-x-0 max-[760px]:translate-y-0"
					data-hero-copy=""
				>
					<h1 className="verevon-home-heading verevon-hero-heading verevon-hero-text-shadow max-w-none text-verevon-c-white text-balance">
							<span className="block whitespace-nowrap max-[760px]:whitespace-normal">En arbeidsflate.</span>
							<span className="block whitespace-nowrap max-[760px]:whitespace-normal">For mennesker og KI sammen.</span>
					</h1>

							<p className="ml-[clamp(0rem,3.6vw,3.2rem)] mt-6 max-w-[30ch] font-manrope text-[clamp(1rem,0.95vw,1.0625rem)] font-light leading-[1.5] tracking-[-0.015em] text-[color-mix(in_srgb,var(--verevon-c-white)_80%,transparent)] text-pretty max-[760px]:ml-0">
							Verevon samler virksomhetens systemer, kunnskap og arbeidslogikk i ett felles, styrt miljø. AI arbeider innenfor reglene dere setter, og menneskene tar beslutningene.
						</p>

						<div
							className="ml-[clamp(0rem,3.6vw,3.2rem)] mt-8 flex flex-wrap items-center gap-x-8 gap-y-4 max-[760px]:ml-0"
							data-hero-actions=""
						>
							<ArrowButton
								className="verevon-cta-drop verevon-hero-button-shadow verevon-hero-primary"
								href="#flyt"
								variant="light"
							>
								Se Verevon i arbeid
							</ArrowButton>
							<a className="verevon-hero-secondary" data-hero-secondary="" href="#kontakt">
								Snakk med oss <span aria-hidden="true">→</span>
							</a>
						</div>
				</div>
			</div>

			<div
				aria-label="Verevons systemområder"
				className="absolute bottom-[clamp(28px,4vh,48px)] right-[clamp(5.5rem,9vw,10rem)] z-[2] max-w-[min(55vw,760px)] pb-0.5 text-right font-geist text-[clamp(0.6875rem,0.72vw,0.75rem)] font-medium uppercase tracking-[0.08em] text-[color-mix(in_srgb,var(--verevon-c-white)_72%,transparent)] max-[760px]:right-[5.5rem] max-[760px]:max-w-[calc(100vw-7.5rem)]"
				data-hero-domains=""
			>
				Systemer <span className="px-1.5 text-[color-mix(in_srgb,var(--verevon-c-white)_42%,transparent)]">·</span> Kunnskap <span className="px-1.5 text-[color-mix(in_srgb,var(--verevon-c-white)_42%,transparent)]">·</span> Arbeidslogikk <span className="px-1.5 text-[color-mix(in_srgb,var(--verevon-c-white)_42%,transparent)]">·</span> AI <span className="px-1.5 text-[color-mix(in_srgb,var(--verevon-c-white)_42%,transparent)]">·</span> Kontroll
			</div>
		</section>
	);
}

export default HeroSection;
