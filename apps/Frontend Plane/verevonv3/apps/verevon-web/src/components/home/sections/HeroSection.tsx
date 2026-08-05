import { HeroParallax } from "./HeroParallax";
import { SignalPathLayer } from "./SignalPathLayer";
import { VerevonMarkOutline } from "./VerevonMark";
import { ArrowButton } from "@/components/ui/ArrowButton";

/**
 * HeroSectionV2 — calm, ambient, fast.
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
					autoPlay
					className="size-full object-cover object-center max-[760px]:object-left"
					loop
					muted
					playsInline
					preload="metadata"
				>
					<source
						src="/verevon-vibe/verevon-hero-premiere-light.mp4"
						type="video/mp4"
					/>
				</video>
			</div>

			<div className="pointer-events-none absolute left-[70%] top-[50%] z-[1] w-[min(62vw,760px)] -translate-x-1/2 -translate-y-1/2 text-[color-mix(in_srgb,var(--verevon-c-white)_13%,transparent)] mix-blend-screen max-[760px]:left-[66%] max-[760px]:top-[43%] max-[760px]:w-[128vw]">
				<VerevonMarkOutline className="h-auto w-full" strokeWidth={2} />
			</div>

			<SignalPathLayer variant="hero" />
			<HeroParallax />

			<div
				className="relative z-[1] mx-auto flex w-full max-w-[1680px] flex-col px-[var(--verevon-edge)] pb-[clamp(260px,29vh,380px)] pt-[clamp(150px,18vh,220px)] max-[760px]:px-[var(--verevon-page-pad)] max-[760px]:pb-[180px]"
				data-hero-parallax-copy=""
			>
				<div className="max-w-[50rem]">
					<h1 className="verevon-hero-text-shadow max-w-none font-arbeit text-[clamp(2.3rem,3.9vw,4.7rem)] font-normal leading-[0.98] tracking-[-0.045em] text-verevon-c-white text-balance">
							<span className="block whitespace-nowrap max-[760px]:whitespace-normal">AI-plattformen for</span>
							<span className="block whitespace-nowrap max-[760px]:whitespace-normal">kunnskap og handling</span>
					</h1>

							<p className="ml-[clamp(0rem,4.5vw,4rem)] mt-6 max-w-[38rem] font-protokoll text-[clamp(0.9rem,0.9vw,1.03rem)] font-light leading-[1.42] text-[color-mix(in_srgb,var(--verevon-c-white)_80%,transparent)] text-pretty max-[760px]:ml-0">
							Verevon kobler virksomhetens kunnskap og systemer til agenter som finner, forstår og får arbeidet gjort — med synlige kilder og kontroll.
						</p>

						<div className="ml-[clamp(0rem,4.5vw,4rem)] mt-7 flex flex-wrap items-center gap-x-10 gap-y-4 max-[760px]:ml-0">
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

			<a
				aria-label="Scroll til produktseksjonen"
				className="verevon-hero-scroll-cue absolute bottom-[clamp(28px,4vh,48px)] right-[var(--verevon-edge)] z-[2] inline-flex items-end gap-3 font-protokoll text-[0.72rem] font-light leading-none text-[color-mix(in_srgb,var(--verevon-c-white)_64%,transparent)] max-[760px]:right-[var(--verevon-page-pad)]"
				href="#produkt"
			>
				<span
					aria-hidden="true"
					className="verevon-hero-scroll-cue__line h-[42px] w-px"
				/>
				<span className="pb-0.5">Scroll</span>
			</a>
		</section>
	);
}

export default HeroSection;
