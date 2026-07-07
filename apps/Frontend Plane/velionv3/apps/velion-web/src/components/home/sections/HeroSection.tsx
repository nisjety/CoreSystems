import { VelionMarkOutline } from "./VelionMark";

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
			className="relative isolate flex min-h-svh flex-col justify-end overflow-hidden bg-velion-h-ink-warm text-velion-c-white"
			id="top"
		>
			<div
				aria-hidden="true"
				className="absolute inset-0 z-0 overflow-hidden"
			>
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
						src="/velion-vibe/velion-hero-premiere-light.mp4"
						type="video/mp4"
					/>
				</video>
			</div>

			<div className="pointer-events-none absolute left-[70%] top-[50%] z-[1] w-[min(62vw,760px)] -translate-x-1/2 -translate-y-1/2 text-[color-mix(in_srgb,var(--velion-c-white)_13%,transparent)] mix-blend-screen max-[760px]:left-[66%] max-[760px]:top-[43%] max-[760px]:w-[128vw]">
				<VelionMarkOutline className="h-auto w-full" strokeWidth={2} />
			</div>

			<div className="relative z-[1] mx-auto flex w-full max-w-[1680px] flex-col px-[var(--velion-edge)] pb-[clamp(112px,15vh,178px)] pt-[clamp(150px,18vh,220px)] max-[760px]:px-[var(--velion-page-pad)] max-[760px]:pb-[96px]">
				<h1 className="max-w-[13ch] font-arbeit text-[clamp(2.85rem,5.6vw,6.9rem)] font-light leading-[0.96] tracking-[-0.045em] text-velion-c-white text-balance">
					Fra signal til handling.
				</h1>

				<p className="mt-6 max-w-[34rem] font-protokoll text-[clamp(0.9rem,0.9vw,1.03rem)] font-light leading-[1.42] text-[color-mix(in_srgb,var(--velion-c-white)_72%,transparent)] text-pretty">
					Kilder, svar og godkjenning i én arbeidsflyt.
				</p>

				<div className="mt-7">
					<a
						className="group inline-flex w-fit items-center gap-3 font-protokoll text-[clamp(0.94rem,0.95vw,1.08rem)] font-light leading-none text-[color-mix(in_srgb,var(--velion-c-white)_72%,transparent)] transition-colors duration-300 hover:text-velion-c-white"
						href="#produkt"
					>
						<span className="whitespace-nowrap">
							Se arbeidsflyten
						</span>
						<span
							aria-hidden="true"
							className="h-px w-7 origin-left bg-current opacity-70 transition-transform duration-300 group-hover:scale-x-125"
						/>
					</a>
				</div>
			</div>

			<a
				aria-label="Scroll til produktseksjonen"
				className="velion-hero-scroll-cue absolute bottom-[clamp(28px,4vh,48px)] right-[var(--velion-edge)] z-[2] inline-flex items-end gap-3 font-protokoll text-[0.72rem] font-light leading-none text-[color-mix(in_srgb,var(--velion-c-white)_64%,transparent)] max-[760px]:right-[var(--velion-page-pad)]"
				href="#produkt"
			>
				<span
					aria-hidden="true"
					className="velion-hero-scroll-cue__line h-[42px] w-px"
				/>
				<span className="pb-0.5">Scroll</span>
			</a>
		</section>
	);
}

export default HeroSection;
