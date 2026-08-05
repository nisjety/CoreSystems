"use client";

import Image from "next/image";
import * as React from "react";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { Section } from "./section";

gsap.registerPlugin(ScrollTrigger);

// One "chapter" of the editorial scroll story below.
type SenseChapter = {
	ambientImage: string;
	imageAlt: string;
	id: "search" | "trace" | "operate";
	subtitle: string;
	text: string;
	highlight: string;
	titleLines: string[];
};

// The three proof moments, rendered top to bottom as <li> items. On desktop
// each chapter's heading/body is pinned to the viewport and cross-fades into
// the next as you scroll past it; on mobile they become a plain stacked list.
const chapters: SenseChapter[] = [
	{
		ambientImage: "/3-guys-working.jpg",
		imageAlt:
			"Tre kolleger arbeider sammen ved et bord i et varmt, dempet rom.",
		id: "search",
		subtitle: "Søk",
		titleLines: ["Finn med", "grunnlag."],
		text: "Start med oppgaven. Verevon søker i virksomhetens kunnskap, norske datakilder og webben — og samler relevant kontekst før dere må lete selv.",
		highlight: "samler relevant kontekst",
	},
	{
		ambientImage: "/aruc-launcher-after.jpg",
		imageAlt:
			"Et menneske med teknologiske kodelinjer reflektert i ansiktet.",
		id: "trace",
		subtitle: "Grunnlag",
		titleLines: ["Se hva", "svaret", "bygger på."],
		text: "Åpne kilden, utdraget, tidspunktet og regelen bak forslaget. Se hva som er sikkert, hva som mangler og hvorfor Verevon foreslår neste steg.",
		highlight: "hva som mangler",
	},
	{
		ambientImage: "/man-talking-and-delegating.jpg",
		imageAlt:
			"En person forklarer noe med hånden foran et lyst prosjektrom.",
		id: "operate",
		subtitle: "Delegering",
		titleLines: ["La agenten", "gjøre", "mer."],
		text: "Gi agenten godkjente verktøy og tydelige rammer. Følg stegene, stopp underveis og godkjenn før noe viktig sendes eller endres.",
		highlight: "Følg stegene, stopp underveis og godkjenn",
	},
];

// Wraps the `highlight` substring of `text` in a coral <span>, if present.
function renderHighlightText(text: string, highlight: string) {
	const parts = text.split(highlight);

	if (parts.length === 1) {
		return text;
	}

	return (
		<>
			{parts.map((part, index) => (
				<React.Fragment key={`${highlight}-${index}`}>
					{part}
					{index < parts.length - 1 ? (
						<span className="font-normal text-[color-mix(in_srgb,var(--verevon-coral)_76%,var(--verevon-j-text))]">
							{highlight}
						</span>
					) : null}
				</React.Fragment>
			))}
		</>
	);
}

// Two faint vertical rules spanning the section, drifting slowly up/down as
// the whole section scrolls past (see the `grid` scrollTrigger below) —
// a subtle parallax backdrop behind everything else. Desktop-only motion;
// on mobile/reduced-motion it just sits still.
function SensesGrid() {
	return (
		<div
			aria-hidden="true"
			className="pointer-events-none absolute inset-0 z-0 overflow-hidden text-[color-mix(in_srgb,var(--verevon-j-text)_6%,transparent)]"
			data-editorial-grid=""
		>
			<div className="absolute inset-y-0 left-[var(--verevon-edge)] right-[var(--verevon-edge)] max-[760px]:left-[var(--verevon-page-pad)] max-[760px]:right-[var(--verevon-page-pad)]">
				<span className="absolute inset-y-0 left-0 w-px bg-current" />
				<span className="absolute inset-y-0 right-0 w-px bg-current" />
			</div>
		</div>
	);
}

export function SensesSection() {
	const containerRef = React.useRef<HTMLDivElement>(null);

	React.useLayoutEffect(() => {
		const container = containerRef.current;

		if (!container) {
			return;
		}

		const items = gsap.utils.toArray<HTMLElement>(".usp-item", container);
		const grid = container.querySelector<HTMLElement>(
			"[data-editorial-grid]",
		);
		const contents = items
			.map((item) => item.querySelector<HTMLElement>(".usp-content"))
			.filter(
				(target): target is HTMLElement => target instanceof HTMLElement,
			);
		const media = items
			.map((item) =>
				item.querySelector<HTMLElement>(".usp-media-wrapper > div"),
			)
			.filter(
				(target): target is HTMLElement => target instanceof HTMLElement,
			);

		// gsap.matchMedia() (not a one-time `window.matchMedia().matches` check)
		// so this whole setup properly reverts and rebuilds if the viewport
		// crosses the 1024px breakpoint mid-session — e.g. resizing a window
		// or rotating a tablet — instead of leaving stale desktop transforms
		// applied to what's now a plain mobile stacked layout.
		const mm = gsap.matchMedia();

		mm.add(
			{
				isDesktop: "(min-width: 1024px)",
				reduceMotion: "(prefers-reduced-motion: reduce)",
			},
			(context) => {
				const { isDesktop, reduceMotion } = context.conditions as {
					isDesktop: boolean;
					reduceMotion: boolean;
				};

				// prefers-reduced-motion: show everything statically, no scrub.
				if (reduceMotion) {
					gsap.set(contents, {
						clearProps: "transform",
						opacity: 1,
						position: "static",
						visibility: "inherit",
					});
					gsap.set([grid, ...media].filter(Boolean), {
						clearProps: "transform",
					});
					return;
				}

				// Below 1024px the CSS drops `.usp-content`'s `position: fixed` and
				// the grid columns, falling back to a plain scrolling stack — clear
				// any GSAP-applied transform/opacity so nothing here fights that.
				if (!isDesktop) {
					gsap.set(
						[grid, ...contents, ...media].filter(Boolean),
						{ clearProps: "all" },
					);
					return;
				}

				// Desktop: the background rule-lines drift slightly as the section
				// passes through the viewport (from just before it enters to just
				// after it fully exits — "top bottom" to "bottom top").
				if (grid) {
					gsap.fromTo(
						grid,
						{ yPercent: -0.55 },
						{
							yPercent: 0.65,
							ease: "none",
							force3D: true,
							scrollTrigger: {
								trigger: container,
								start: "top bottom",
								end: "bottom top",
								scrub: 0.85,
								id: "verevon-senses-grid",
								invalidateOnRefresh: true,
							},
						},
					);
				}

				items.forEach((item, index) => {
					const content = item.querySelector<HTMLElement>(".usp-content");
					const itemMedia = item.querySelector<HTMLElement>(
						".usp-media-wrapper > div",
					);
					const isFirst = index === 0;

					if (content) {
						// One timeline per chapter's fixed text block, scrubbed across
						// its own full pass-through range ("top bottom" to "bottom top" —
						// same range as its media below). Deliberately a single
						// ScrollTrigger with in-timeline keyframes, not two separate
						// fromTo tweens: running two independent ScrollTriggers against
						// the same element's same properties produced inconsistent,
						// hard-to-predict progress values in testing.
						//
						// Keyframes: fade in over 0-25%, hold fully visible 25-75%,
						// fade out over 75-100%. The first chapter skips the fade-in
						// (it's already on screen at page load) and starts straight
						// from the held/visible state.
						const contentTl = gsap.timeline({
							scrollTrigger: {
								trigger: item,
								start: "top bottom",
								end: "bottom top",
								scrub: true,
								id: `verevon-senses-copy-${index + 1}`,
								invalidateOnRefresh: true,
							},
						});

						if (isFirst) {
							contentTl.set(
								content,
								{ opacity: 1, visibility: "inherit", y: 0 },
								0,
							);
						} else {
							contentTl.fromTo(
								content,
								{ opacity: 0, visibility: "hidden", y: 50 },
								{
									opacity: 1,
									visibility: "inherit",
									y: 0,
									ease: "none",
									force3D: true,
									duration: 0.25,
								},
								0,
							);
						}

						contentTl.to(
							content,
							{
								opacity: 0,
								y: -50,
								ease: "none",
								force3D: true,
								duration: 0.25,
							},
							0.75,
						);
					}

					// The chapter's image: a slow scale+translate parallax across its
					// own full pass-through range. No opacity change here — as a
					// normal-flow grid item (not `fixed`, unlike the text above) it
					// naturally scrolls out of view on its own, no fade needed.
					if (itemMedia) {
						gsap.fromTo(
							itemMedia,
							{
								scale: 1.1,
								y: -100,
							},
							{
								scale: 1,
								y: 100,
								ease: "none",
								force3D: true,
								scrollTrigger: {
									trigger: item,
									start: "top bottom",
									end: "bottom top",
									scrub: true,
									id: `verevon-senses-media-${index + 1}`,
									invalidateOnRefresh: true,
								},
							},
						);
					}
				});

				return undefined;
			},
		);

		// This section's trigger positions depend on the final height of
		// everything above it on the page (Hero video, FeatureCards images,
		// etc.), so they need recalculating once that's actually settled —
		// not just once fonts are ready. `refreshPriority` is deliberately
		// left unset here (default = creation order) rather than given a
		// number: an earlier version set it *higher* than FeatureCardsSection's
		// pin above it, which made this section's positions get calculated
		// *before* that pin had finished reserving its scroll space — i.e.
		// against a page that was still temporarily too short. That produced
		// wrong, inconsistent trigger ranges (chapters stuck invisible) that
		// varied between page loads.
		document.fonts?.ready.then(() => ScrollTrigger.refresh());
		window.requestAnimationFrame(() => ScrollTrigger.refresh());

		const refreshOnceSettled = () => {
			ScrollTrigger.refresh();
			window.setTimeout(() => ScrollTrigger.refresh(), 500);
		};

		if (document.readyState === "complete") {
			refreshOnceSettled();
		} else {
			window.addEventListener("load", refreshOnceSettled, { once: true });
		}

		return () => mm.revert();
	}, []);

	return (
		<Section
			containerClassName="p-0 border-t border-verevon-j-text/8"
			id="kunnskap"
			title="Kunnskapen følger arbeidet"
			titleClassName="sr-only"
			variant="full-bleed-tight"
		>
			<div
				className="relative w-full overflow-clip bg-[color-mix(in_srgb,var(--background)_92%,var(--verevon-bg-soft))] text-verevon-j-text"
				ref={containerRef}
			>
				<SensesGrid />

				{/* This section shifts from what Verevon connects to what working with
				    Verevon feels like: context follows the task and stays inspectable. */}
				<div className="relative z-10 px-[var(--verevon-edge)] pb-[clamp(28px,4vw,64px)] pt-[clamp(104px,11vw,154px)] max-[760px]:px-[var(--verevon-page-pad)]">
					<span className="verevon-eyebrow block text-[color-mix(in_srgb,var(--verevon-a-earth)_72%,var(--verevon-j-text))]">
						02 / Fra søk til styring
					</span>
					<h2 className="mt-7 max-w-[11ch] font-arbeit text-[clamp(3rem,6.5vw,7rem)] font-light leading-[0.9] tracking-[-0.07em] text-verevon-j-text text-balance">
						Se Verevon i arbeid
					</h2>
					<p className="mt-7 max-w-[620px] font-arbeit text-[clamp(1.15rem,1.7vw,1.55rem)] font-light leading-[1.42] tracking-[-0.025em] text-verevon-j-text/66 text-pretty">
						Søk, kilder og agentsteg samles i samme arbeidsflate — slik at dere kan forstå, vurdere og gå videre.
					</p>
				</div>

				<ul className="relative z-10 m-0 flex list-none flex-col gap-0 p-0">
					{chapters.map((chapter, index) => {
						// Chapters alternate which side the image sits on
						// (left/right) as you scroll down the list.
						const mediaColumn =
							index % 2 === 0
								? "lg:col-start-3"
								: "lg:col-start-1";
						const mediaRuleSide =
							index % 2 === 0 ? "left-0" : "right-0";

						return (
							<li
								// Mobile (<1024px): a plain single-column stack, image
								// above text. Desktop (lg:): a 3-column grid — text
								// fixed/centered in the middle, image alternating
								// left/right in a side column — each item exactly one
								// viewport tall (`min-h-[75svh]`) so scrolling past one
								// chapter reliably hands off into the next.
								className="usp-item relative grid min-h-[auto] grid-cols-1 px-[var(--verevon-edge)] max-lg:gap-10 max-lg:py-[clamp(64px,11vw,104px)] max-[760px]:px-[var(--verevon-page-pad)] lg:min-h-[75svh] lg:grid-cols-[1fr_2fr_1fr]"
								key={chapter.id}
							>
								{/* Thin divider along the bottom of each chapter (mobile only look, but present at all sizes via z-index). */}
								<span
									aria-hidden="true"
									className="pointer-events-none absolute bottom-0 left-[var(--verevon-edge)] right-[var(--verevon-edge)] z-[1] h-[1px] bg-[color-mix(in_srgb,var(--verevon-j-text)_12%,transparent)] max-[760px]:left-[var(--verevon-page-pad)] max-[760px]:right-[var(--verevon-page-pad)]"
									data-senses-image-rule="horizontal"
								/>

								<div className="relative py-0 text-center max-lg:row-start-2 max-lg:text-left lg:col-start-2 lg:row-start-1 lg:[clip-path:border-box]">
									{/* On desktop this becomes `position: fixed; inset: 0` —
									    it's centered across the *whole* viewport (via the
									    flex centering below), not confined to this grid
									    cell's width. That's why the paragraph below has its
									    own `lg:max-w` clamp rather than relying on the grid
									    column to constrain it. */}
									<div className="usp-content flex flex-col items-center justify-center gap-[clamp(22px,2.5vw,42px)] px-4 max-lg:items-start lg:fixed lg:inset-0 lg:pointer-events-none">
										<p className="m-0 overflow-clip font-protokoll text-[0.68rem] font-light uppercase leading-none tracking-[0.26em] text-[color-mix(in_srgb,var(--verevon-a-earth)_72%,var(--verevon-j-text))]">
											Kapittel 0{index + 1} /{" "}
											{chapter.subtitle}
										</p>

										<h2 className="m-0 max-w-[14ch] py-2 font-arbeit text-[clamp(2.65rem,5.4vw,6.8rem)] font-light leading-[0.9] tracking-[-0.068em] text-verevon-j-text text-balance max-lg:max-w-[11ch] max-lg:text-[clamp(2.55rem,14vw,4.8rem)]">
											{chapter.titleLines.map((line) => (
												<span
													className="block"
													key={line}
												>
													{line}
												</span>
											))}
										</h2>

										{/* `lg:max-w-[min(540px,40vw)]`: at wide desktop widths
										    this is just 540px as before; at narrower desktop
										    widths (roughly 1024-1350px) it shrinks with the
										    viewport so the paragraph never grows wide enough to
										    reach into the image column next to it. */}
										<p className="m-0 max-w-[540px] font-arbeit text-[clamp(1rem,1.06vw,1.22rem)] font-light leading-[1.62] tracking-[-0.02em] text-verevon-j-text/66 text-pretty lg:max-w-[min(540px,40vw)]">
											{renderHighlightText(
												chapter.text,
												chapter.highlight,
											)}
										</p>

						{chapter.id === "search" ? (
												<a
													className="pointer-events-auto mt-1 font-protokoll text-[0.88rem] font-light text-verevon-j-text/58 underline-offset-4 transition-colors hover:text-verevon-j-text hover:underline"
													href="/plattform/felles-kontekst"
												>
													Se hvordan Verevon bygger kontekst
												</a>
										) : null}
									</div>
								</div>

								{/* `min-w-0` here is required: without it, this grid
								    item's implicit "auto" minimum width lets the image's
								    height-driven intrinsic size (aspect-ratio x 75svh)
								    force the "1fr" track wider than its real share,
								    overlapping the text column next to it. With it, the
								    track honors the declared 1fr_2fr_1fr split and the
								    image sizes itself to fit (see `.usp-media-wrapper`
								    below for the height/aspect-ratio side of that fix). */}
								<figure
									className={[
										"relative m-0 min-w-0 overflow-visible max-lg:row-start-1",
										"lg:row-start-1",
										mediaColumn,
									]
										.filter(Boolean)
										.join(" ")}
								>
									{/* `lg:h-auto lg:max-h-[75svh]` (not a forced `h-[75svh]`):
									    width comes first, from this column's real, grid-
									    constrained width; height is then derived from
									    `aspect-[4/5]` and only ever capped — never forced —
									    at 75% of the viewport height. Setting both width and
									    height explicitly (the previous approach) overrides
									    aspect-ratio entirely and stretches the image. */}
									<div className="usp-media-wrapper relative mx-auto aspect-[4/5] h-full w-full max-w-[88vw] overflow-clip bg-white/45 shadow-[inset_0_0_0_1px_rgba(26,26,26,0.05)] max-lg:max-w-[560px] lg:h-auto lg:max-h-[75svh] lg:max-w-full">
										<div className="relative size-full scale-110">
											<Image
												alt={chapter.imageAlt}
												className="object-cover object-center saturate-[0.9] contrast-[0.98]"
												fill
												priority={index === 0}
												sizes="(max-width: 1024px) 88vw, 33vw"
												src={chapter.ambientImage}
											/>
											<div
												aria-hidden="true"
												className="absolute inset-0 bg-[linear-gradient(180deg,rgba(15,18,18,0.08),transparent_42%,rgba(15,18,18,0.24))]"
											/>
										</div>
									</div>
									{/* Thin vertical rule along the image's inner edge (desktop only). */}
									<span
										aria-hidden="true"
										className={[
											"pointer-events-none absolute -bottom-[20%] -top-[20%] z-[1] w-[1px]",
											mediaRuleSide,
										].join(" ")}
										data-senses-image-rule="side"
										style={{
											background:
												"linear-gradient(180deg, transparent 0%, color-mix(in srgb, var(--verevon-j-text) 12%, transparent) 15%, color-mix(in srgb, var(--verevon-j-text) 12%, transparent) 85%, transparent 100%)",
										}}
									/>
								</figure>
							</li>
						);
					})}
				</ul>

				<div className="relative z-10 flex justify-end px-[var(--verevon-edge)] pb-[clamp(96px,12vw,220px)] pt-[clamp(52px,6vw,96px)] max-[760px]:px-[var(--verevon-page-pad)]">
					<p className="m-0 max-w-[540px] text-right font-arbeit text-[clamp(1rem,1.06vw,1.22rem)] font-light leading-[1.62] tracking-[-0.02em] text-verevon-j-text/66 text-pretty">
						Søk først. Se grunnlaget. Bestem neste steg.
					</p>
				</div>
			</div>
		</Section>
	);
}

export default SensesSection;
