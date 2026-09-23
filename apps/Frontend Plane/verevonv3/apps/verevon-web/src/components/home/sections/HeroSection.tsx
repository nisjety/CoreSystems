"use client";

import { useEffect, useRef } from "react";
import { HeroParallax } from "./HeroParallax";
import { SignalPathLayer } from "./SignalPathLayer";
import { ArrowButton } from "@/components/ui/ArrowButton";
import { usePrefersReducedMotion } from "@/shared/hooks/usePrefersReducedMotion";

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

		void video.play().catch(() => {
			// The supporting copy remains usable when autoplay is blocked.
		});
	}, [prefersReducedMotion]);

	return (
		<section
			className="relative isolate flex min-h-svh flex-col justify-end overflow-hidden bg-verevon-h-ink-warm text-verevon-c-white"
			data-hero-parallax=""
			id="top"
		>
			<div
				aria-hidden="true"
				className="absolute inset-0 z-0 overflow-hidden"
				data-hero-parallax-media=""
			>
				<video
					className="size-full object-cover object-[91%_center] max-[760px]:object-[76%_center]"
					loop
					muted
					playsInline
					preload="metadata"
					ref={videoRef}
				>
					<source
						src="/verevon-vibe/hero-candidate-01-silhouette-4k.mp4"
						type="video/mp4"
					/>
				</video>
				<div className="pointer-events-none absolute inset-0 bg-black/15" />
			</div>

			<SignalPathLayer variant="hero" />
			<HeroParallax />

			<div
				className="relative z-[1] mx-auto flex min-h-svh w-full flex-col justify-center px-[var(--verevon-edge)] py-[clamp(72px,10vh,120px)] max-[760px]:px-[var(--verevon-page-pad)]"
				data-hero-parallax-copy=""
			>
				<div className="max-w-[40rem] -translate-x-[17%] translate-y-[10%] max-[760px]:translate-x-0 max-[760px]:translate-y-0">
					<h1 className="verevon-home-heading verevon-hero-heading verevon-hero-text-shadow max-w-none text-verevon-c-white text-balance">
							<span className="block whitespace-nowrap max-[760px]:whitespace-normal">Én arbeidsflate.</span>
							<span className="block whitespace-nowrap max-[760px]:whitespace-normal">For mennesker og AI.</span>
					</h1>

							<p className="ml-[clamp(0rem,3.6vw,3.2rem)] mt-6 max-w-[30.4rem] font-protokoll text-[clamp(0.9rem,0.9vw,1.03rem)] font-light leading-[1.42] text-[color-mix(in_srgb,var(--verevon-c-white)_80%,transparent)] text-pretty max-[760px]:ml-0">
							Verevon samler kunnskap, verktøy og oppgaver. Jobb selv, eller la Verevon ta neste steg — innenfor rammene dere setter.
						</p>

						<div className="ml-[clamp(0rem,3.6vw,3.2rem)] mt-7 flex flex-wrap items-center gap-x-10 gap-y-4 max-[760px]:ml-0">
							<ArrowButton className="verevon-hero-button-shadow" href="#produkt" variant="light">
								Se arbeidsflyten
							</ArrowButton>
							<ArrowButton
								className="verevon-hero-button-shadow opacity-70 transition-opacity duration-300 hover:opacity-100"
								href="#kontakt"
								variant="light"
							>
								Be om tidlig tilgang
							</ArrowButton>
						</div>
				</div>
			</div>

			<div
				aria-label="Verevons systemområder"
				className="absolute bottom-[clamp(28px,4vh,48px)] right-[clamp(5.5rem,9vw,10rem)] z-[2] max-w-[min(55vw,760px)] pb-0.5 text-right font-protokoll text-[clamp(0.62rem,0.7vw,0.78rem)] font-light uppercase tracking-[0.14em] text-[color-mix(in_srgb,var(--verevon-c-white)_72%,transparent)] max-[760px]:right-[5.5rem] max-[760px]:max-w-[calc(100vw-7.5rem)]"
			>
				Kilder <span className="px-1.5 text-[color-mix(in_srgb,var(--verevon-c-white)_42%,transparent)]">·</span> Kunnskap <span className="px-1.5 text-[color-mix(in_srgb,var(--verevon-c-white)_42%,transparent)]">·</span> Søk <span className="px-1.5 text-[color-mix(in_srgb,var(--verevon-c-white)_42%,transparent)]">·</span> Agenter <span className="px-1.5 text-[color-mix(in_srgb,var(--verevon-c-white)_42%,transparent)]">·</span> Systemer <span className="px-1.5 text-[color-mix(in_srgb,var(--verevon-c-white)_42%,transparent)]">·</span> Kontroll
			</div>
		</section>
	);
}

export default HeroSection;
