import { ArrowButton } from "@/components/ui/ArrowButton";

/**
 * HeroSectionV2 — calm, ambient, fast.
 *
 * One continuous video loop as ambient background (TYDE/Terminal pattern) —
 * NO staged reveal choreography (video→image→copy stagger). All copy is
 * server-rendered and visible at first paint, so the LCP element paints
 * immediately instead of being gated by an animation timeline.
 *
 * Above the fold: one claim + one proof point + two shared ArrowButton CTAs.
 */
export function HeroSectionV2() {
	return (
		<section
			className="relative isolate flex min-h-svh flex-col justify-center overflow-hidden bg-velion-h-ink-warm text-velion-c-white"
			id="top"
		>
			<div aria-hidden="true" className="absolute inset-0 z-0 overflow-hidden">
				<video
					autoPlay
					className="size-full object-cover object-center max-[760px]:object-left"
					loop
					muted
					playsInline
					poster="/velion-vibe/velion-signal-drone-short-poster.jpg"
					preload="metadata"
				>
					<source
						src="/velion-vibe/velion-signal-drone-short.mp4"
						type="video/mp4"
					/>
				</video>

			</div>

			<div className="relative z-[1] mx-auto flex w-full max-w-[1680px] flex-col px-[var(--velion-edge)] py-[clamp(120px,16vh,200px)] max-[760px]:px-[var(--velion-page-pad)]">
				<p className="velion-eyebrow text-[color-mix(in_srgb,var(--velion-c-white)_64%,transparent)]">
					Norsk AI-arbeidsbenk
				</p>

				<h1 className="mt-7 max-w-[17ch] font-arbeit text-[clamp(3rem,6vw,7rem)] font-light leading-[0.95] tracking-[-0.04em] text-velion-c-white text-balance">
					Fra kundesignal til godkjent handling.
				</h1>

				<p className="mt-8 max-w-[52ch] font-protokoll text-[clamp(1.05rem,1.15vw,1.32rem)] font-light leading-[1.5] text-[color-mix(in_srgb,var(--velion-c-white)_80%,transparent)] text-pretty">
					Velion finner kilder, skriver forslag og venter på din godkjenning før
					noe sendes — og dataene blir i EU.
				</p>

				<div className="mt-10 flex flex-wrap items-center gap-x-8 gap-y-4">
					<ArrowButton href="#produkt" variant="light">
						Se arbeidsflyten
					</ArrowButton>

					<ArrowButton href="/trust" variant="light">
						Åpne tillitssenteret
					</ArrowButton>
				</div>

				<p className="mt-9 inline-flex w-fit items-center gap-2.5 font-protokoll text-[0.86rem] font-light text-[color-mix(in_srgb,var(--velion-c-white)_66%,transparent)]">
					<span
						aria-hidden="true"
						className="size-1.5 rounded-full bg-velion-coral"
					/>
					Forankret i Enhetsregisteret (Brreg) · Data i EU/EØS (Sweden Central)
				</p>
			</div>
		</section>
	);
}

export default HeroSectionV2;
