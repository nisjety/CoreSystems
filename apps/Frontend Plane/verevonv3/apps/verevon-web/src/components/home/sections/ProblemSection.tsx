"use client";

import Image from "next/image";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import type { KeyboardEvent } from "react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { ArrowButton } from "@/components/ui/ArrowButton";
import { Eyebrow } from "@/components/ui/SectionHeading";
import { Reveal } from "@/components/home/sections/Reveal";
import { SignalPathLayer } from "@/components/home/sections/SignalPathLayer";
import { cn } from "@/lib/utils";
import {
	controlProblemShots,
	scatteredProblemShots,
	waitingProblemShots,
	type ProblemVideoShot,
} from "@/components/home/sections/problemVideoSequences";

gsap.registerPlugin(ScrollTrigger);

type IndexCard = {
	body: string;
	href: string;
	image: string;
	/** Object-position for the crop — needed for the tall portrait renders. */
	imagePosition?: string;
	kicker: string;
	label: string;
	title: string;
	/** Silent hover-preview sequence; each shot advances only after the previous one ends. */
	videos: ProblemVideoShot[];
};

type GalaxyImage = {
	alt: string;
	/** Larger, higher-opacity "anchor" photo vs. the small faded ambient shots. */
	anchor?: boolean;
	aspect: string;
	className: string;
	rotate: number;
	src: string;
	width: number;
};

// A Wolverine-style "image galaxy" scattered THROUGH the credo's frame (not
// clustered in the margins): photos threaded through big statement type.
//
// Thinned from 8 stills to 5 deliberately. The old set was almost entirely
// abstract renders (soft-orb, dune light, clouds, a twisted ribbon) AND it
// double-printed the backlit bird — /27aab72a…jpg here and /warm-flight.png
// on card 02 below are the same photograph. Abstraction only reads as
// premium while it stays rare, and this section sits next to an already very
// abstract PreFooter, so the scatter now leans on grounded stills and keeps
// exactly one soft render for air.
//
// Desktop-only (max-[760px]:hidden below) — there isn't room to scatter
// imagery once the statement wraps to full width.
const galaxyImages: GalaxyImage[] = [
	{
		src: "/a95a19e05613baa759395e7dfc3241e5.jpg",
		alt: "",
		anchor: true,
		width: 130,
		aspect: "aspect-[3/4]",
		rotate: 2,
		className: "right-[6%] top-[4%] opacity-90",
	},
	{
		src: "/verevon-mood/desk-vast-white.jpg",
		alt: "",
		width: 100,
		aspect: "aspect-[3/4]",
		rotate: -3,
		className: "left-[13%] top-[8%] opacity-45",
	},
	{
		src: "/verevon-mood/window-city-dusk.jpg",
		alt: "",
		width: 95,
		aspect: "aspect-[3/4]",
		rotate: -4,
		className: "right-[19%] top-[24%] opacity-38",
	},
	{
		src: "/soft-orb.png",
		alt: "",
		width: 75,
		aspect: "aspect-[4/3]",
		rotate: 4,
		className: "left-[5%] top-[46%] opacity-28",
	},
	{
		src: "/agent-run-console-running.jpg",
		alt: "",
		width: 85,
		aspect: "aspect-[3/4]",
		rotate: 5,
		className: "bottom-[10%] right-[13%] opacity-26",
	},
];

// Renders one galaxy photo as an absolutely positioned, faded thumbnail.
// Purely decorative (aria-hidden, no alt text needed) — the statement and
// cards below already carry the section's meaning for assistive tech.
function GalaxyThumb({ image }: { image: GalaxyImage }) {
	return (
		<div
			className={cn(
				"group-hover/galaxy:opacity-100 absolute overflow-hidden rounded-[2px] shadow-[0_18px_48px_rgba(23,23,23,0.12)] transition-opacity duration-700",
				image.aspect,
				image.className,
			)}
			style={{
				transform: `rotate(${image.rotate}deg)`,
				width: image.width,
			}}
		>
			<Image
				alt={image.alt}
				className={cn(
					"select-none object-cover",
					image.anchor
						? "saturate-[0.62] contrast-[1.02] sepia-[0.06]"
						: "saturate-[0.35] grayscale-[0.35]",
				)}
				draggable={false}
				fill
				sizes={`${image.width}px`}
				src={image.src}
			/>
		</div>
	);
}

// Three problem cards below the credo — each one points into a later section
// (or a dedicated page, like /trust or /produkt/svartid) instead of
// restating the thesis, so this section stays a single, uncluttered move:
// state the problem, then hand off.
//
// Third pass on these images. Round 2 leaned into cohere.com's dark
// generative-render register (light through cloud, a liquid terrain) but
// that made the cards read as mood rather than explanation — a reader has to
// decode "smooth liquid = control" before the claim lands, which is exactly
// the indirection we don't want here. These are the images that carry the
// argument, so each one now shows a real figure doing the thing the sentence
// describes — figurative and narrative, not a visual metaphor to solve —
// while still allowing a stylised or surreal touch (a body dissolving into
// scattered photo fragments, an oversized floating keycap) rather than plain
// documentary photography:
// - 02: a man on a city street with his eyes obscured by a glitching,
//   iridescent visor — vision itself is blocked, which is the literal
//   picture "ingen har hele bildet" describes.
// - 03: a hand pressing Ctrl+Z — undo/rollback is one of Agents' own feature
//   chips ("Tilbakerulling"), so this is the literal control action the
//   copy describes, not a mood standing in for "control."
//
// Sixth pass, all three cards. The brief changed from "match the title's
// meaning" to three literal, fully-specified scenes (conveyor belt with one
// small object; a wall of monitors around one seated person; an analogue
// control panel with a hand on it) — plus an explicit list of what to avoid
// (neon-blue "AI," floating UI graphics, smiling office teams, generic
// laptops, literal puzzle pieces, obvious gears). None of the previously
// harvested Pinterest boards (curated around abstract/premium/sci-fi) carry
// industrial or bureaucratic imagery like this, so this round sources from
// Unsplash instead — see IMAGE-DIRECTION.md Runde 7 for the search trail.
// - 01: an otherwise-empty airport baggage carousel with exactly one
//   suitcase on it. The belt is the whole visible world; the object riding
//   it is small — "the thing itself is simple, the journey is what takes
//   time," made literal instead of argued for.
// - 02: a lone operator, back to camera, facing a real wall of ~30
//   disconnected camera and system feeds (a model-railway control room) —
//   nobody in that chair has one picture, they have thirty partial ones.
// - 03: a hand on an analogue pressure-gauge dial, "LINE PRESSURE" labelled
//   in shot — a real hand on a real control, not a metaphor for control.
//
// Seventh pass, card 01 only. Higgsfield/Kling exploration produced five
// "waiting caused by process" metaphors (empty waiting room, institutional
// corridor, an escalator, a red traffic light in fog, the baggage carousel
// already in use as this card's still). The escalator was dropped — a
// moving staircase has a top and a bottom, so it reads as progress toward a
// destination, not delay; that's closer to card 03's "a system acting
// without your input" than to card 01. The five selected clips now play as
// one ordered sequence, advancing only when each shot ends. It plays on
// hover (card click still just switches tabs, as before); the CTA now
// navigates to /produkt/svartid, which uses the same file as its hero.
// (First cut of that page argued source-traceability, because "kilde"
// appears in this card's body copy — wrong argument, that belongs to card
// 02. The page now stays on this card's actual thesis: the four steps cost
// time, not the answer itself.)
const cards: IndexCard[] = [
	{
		kicker: "01",
		title: "Det er ikke svaret som tar tid",
		body: "Det er å finne kilden, sjekke reglene, formulere svaret og gjøre neste steg riktig. Et svar uten synlig kilde er ren gjetning med god selvtillit.",
		href: "/produkt/svartid",
		label: "Se tidsbesparelsen",
		image: "/verevon-vibe/problem-waiting/waiting-room-ai-poster.jpg",
		videos: waitingProblemShots,
	},
	{
		kicker: "02",
		title: "Ett spørsmål. Flere systemer. Ingen har hele bildet",
		body: "AI kan formulere et svar, men arbeidet krever også søk, dokumenter, interne regler, historikk, vurdering og systemhandlinger før neste steg kan tas. Kunnskap hjelper ingen før den er koblet til arbeidet.",
		href: "/plattform/felles-kontekst",
		label: "Se felles kontekst",
		image: "/verevon-vibe/problem-scattered/archive-catalog-6549263-poster.jpg",
		videos: scatteredProblemShots,
	},
	{
		kicker: "03",
		title: "Automatisering krever kontroll",
		body: "Automatisering som sender selv, er en risiko ingen har bedt om. Når AI bruker verktøy eller endrer noe, må dere kunne se hvor svaret kommer fra, følge stegene og godkjenne før neste handling.",
		href: "/plattform/kontrollert-arbeid",
		label: "Se kontrollert arbeid",
		image: "/verevon-vibe/problem-control/autonomous-car-ai-poster.jpg",
		imagePosition: "center",
		videos: controlProblemShots,
	},
];

// The credo, split into words for the scroll-linked reveal below — each
// word is its own inline-block span so GSAP can animate opacity per word
// as the section scrolls into view.
const creditLines: string[][] = [
	"Kunnskapen finnes.".split(" "),
	"Men den er spredt.".split(" "),
];
const creditLabel = creditLines.map((line) => line.join(" ")).join("\n");

const supportingCopy =
	"Det som skal til for å løse en oppgave, ligger spredt i dokumenter, systemer, innbokser, nettsider og offentlige kilder. Når konteksten ikke henger sammen, blir AI-svaret bare begynnelsen på jobben.";
const WORD_REVEAL_BASE_OPACITY = 0.1;

function clamp01(value: number) {
	return Math.min(1, Math.max(0, value));
}

function interpolateWordOpacity(progress: number, index: number, count: number) {
	const step = count > 1 ? index / count : 0;
	const width = count > 1 ? 0.62 : 1;
	const localProgress = clamp01((progress - step) / width);

	return (
		WORD_REVEAL_BASE_OPACITY +
		(1 - WORD_REVEAL_BASE_OPACITY) *
			(1 - Math.pow(1 - localProgress, 3))
	);
}

function ProblemLine({ line, lineIndex }: { line: string[]; lineIndex: number }) {
	return (
		<span aria-hidden="true" className="block">
			{line.map((word, wordIndex) => (
				<span
					className="mr-[0.22em] inline-block opacity-10 will-change-[opacity]"
					data-problem-title-word=""
					key={`${lineIndex}-${wordIndex}-${word}`}
				>
					{word}
				</span>
			))}
		</span>
	);
}

/**
 * ProblemSection — "the problem", stated in almost no words.
 *
 * Sits right after the brand-logos strip. NOT pinned — this is a plain,
 * normal-flow section like every other one on the page. Each word of the
 * credo starts dim, then brightens in sequence as the frame enters the
 * viewport. A small image galaxy is scattered through that same frame
 * (Wolverine's "portfolio" device: photos threaded through big statement
 * type, not confined to the margins) and does not participate in the fade.
 * Reduced motion skips the animation and shows the finished, fully legible
 * state. The supporting paragraph and the three problem cards follow in normal
 * flow — the whole job here is to land the thesis, then hand off into the
 * rest of the page.
 */
export function ProblemSection() {
	const sectionRef = useRef<HTMLDivElement>(null);
	const tabRefs = useRef<Array<HTMLButtonElement | null>>([]);
	const videoRefs = useRef<Array<HTMLVideoElement | null>>([]);
	const playingCardIndex = useRef<number | null>(null);
	const advancedShotKey = useRef<string | null>(null);
	const [activeIndex, setActiveIndex] = useState(0);
	const [previousIndex, setPreviousIndex] = useState<number | null>(null);
	const [sequenceIndices, setSequenceIndices] = useState<number[]>(() =>
		cards.map(() => 0),
	);
	const shouldReduceMotion = useReducedMotion() ?? false;

	function selectTab(index: number, moveFocus = false) {
		if (index !== activeIndex) {
			setPreviousIndex(activeIndex);
			setActiveIndex(index);
		}

		if (moveFocus) {
			tabRefs.current[index]?.focus();
		}
	}

	function handleTabKeyDown(
		event: KeyboardEvent<HTMLButtonElement>,
		index: number,
	) {
		let nextIndex: number | null = null;

		switch (event.key) {
			case "ArrowRight":
				nextIndex = (index + 1) % cards.length;
				break;
			case "ArrowLeft":
				nextIndex = (index - 1 + cards.length) % cards.length;
				break;
			case "Home":
				nextIndex = 0;
				break;
			case "End":
				nextIndex = cards.length - 1;
				break;
			default:
				return;
		}

		event.preventDefault();
		selectTab(nextIndex, true);
	}

	function playCardVideo(index: number) {
		if (shouldReduceMotion || cards[index].videos.length === 0) {
			return;
		}

		playingCardIndex.current = index;
		advancedShotKey.current = null;
		videoRefs.current[index]?.play().catch(() => {
			// Autoplay can be rejected before the user has interacted with the
			// page at all; the poster stays visible either way, so there's
				// nothing to recover from here.
			});
	}

	function pauseCardVideo(index: number) {
		if (playingCardIndex.current === index) {
			playingCardIndex.current = null;
		}
		advancedShotKey.current = null;

		const video = videoRefs.current[index];

		if (!video) {
			return;
		}

		video.pause();
		video.currentTime = 0;
		setSequenceIndices((current) =>
			current.map((sequenceIndex, cardIndex) =>
				cardIndex === index ? 0 : sequenceIndex,
			),
		);
	}

	function advanceCardVideo(index: number) {
		const sequenceLength = cards[index].videos.length;
		const shotKey = `${index}-${sequenceIndices[index]}`;

		if (
			sequenceLength < 2 ||
			playingCardIndex.current !== index ||
			advancedShotKey.current === shotKey
		) {
			return;
		}

		advancedShotKey.current = shotKey;
		setSequenceIndices((current) =>
			current.map((sequenceIndex, cardIndex) =>
				cardIndex === index
					? (sequenceIndex + 1) % sequenceLength
					: sequenceIndex,
			),
		);
	}

	function handleCardVideoTimeUpdate(
		index: number,
		video: HTMLVideoElement,
	) {
		const shot = cards[index].videos[sequenceIndices[index]];

		if (video.currentTime >= shot.duration) {
			advanceCardVideo(index);
		}
	}

	useEffect(() => {
		const index = playingCardIndex.current;

		if (index === null) {
			return;
		}

		const video = videoRefs.current[index];

		if (!video) {
			return;
		}

		video.load();
		video.play().catch(() => {
			// Keep the still poster if playback is unavailable in this browser.
		});
	}, [sequenceIndices]);

	useLayoutEffect(() => {
		const section = sectionRef.current;

		if (!section) {
			return;
		}

		const reduceMotion = window.matchMedia(
			"(prefers-reduced-motion: reduce)",
		).matches;
		const heading = section.querySelector<HTMLElement>(
			"[data-problem-heading]",
		);
		const words = gsap.utils.toArray<HTMLElement>(
			"[data-problem-title-word]",
			section,
		);

		if (reduceMotion || !heading || words.length === 0) {
			gsap.set(words, {
				opacity: 1,
			});
			return;
		}

		const context = gsap.context(() => {
			gsap.set(words, { opacity: WORD_REVEAL_BASE_OPACITY });
			gsap.set(heading, { y: 0, force3D: true });

			ScrollTrigger.create({
				trigger: section,
				start: "top 46%",
				end: "top 12%",
				scrub: 0.16,
				invalidateOnRefresh: true,
				id: "verevon-problem-word-reveal",
				onUpdate: ({ progress }) => {
					gsap.set(heading, {
						y: -24 * progress,
						force3D: true,
					});

					words.forEach((word, index) => {
						gsap.set(word, {
							opacity: interpolateWordOpacity(
								progress,
								index,
								words.length,
							),
						});
					});
				},
				onLeave: () => {
					gsap.set(heading, { y: -24 });
					gsap.set(words, { opacity: 1 });
				},
				onLeaveBack: () => {
					gsap.set(heading, { y: 0 });
					gsap.set(words, { opacity: WORD_REVEAL_BASE_OPACITY });
				},
			});
		}, section);

		document.fonts?.ready.then(() => ScrollTrigger.refresh());

		return () => context.revert();
	}, []);

	return (
		<section
			className="relative isolate overflow-visible bg-background text-verevon-j-text"
			id="problemet"
		>
			{/* Plain, normal-flow frame — no sticky, no pin, no extra scroll
			    room. min-h gives the galaxy room to scatter; the section
			    otherwise scrolls exactly like any other on the page. */}
			<div
				className="group/galaxy relative flex min-h-[85svh] flex-col items-center justify-center px-[var(--verevon-edge)] py-24 max-[760px]:min-h-0 max-[760px]:items-start max-[760px]:px-[var(--verevon-page-pad)] max-[760px]:py-16"
				ref={sectionRef}
			>
				<SignalPathLayer variant="problem" />

				<div
					aria-hidden="true"
					className="pointer-events-none absolute inset-0 z-0 max-[760px]:hidden"
				>
					{galaxyImages.map((image) => (
						<GalaxyThumb image={image} key={image.src} />
					))}
				</div>

				<div className="relative z-[1] w-full max-w-[1680px]">
					<Eyebrow className="mb-8" marker>
						01 / Problemet
					</Eyebrow>

					<h2
						aria-label={creditLabel}
						className="m-0 mx-auto max-w-[16ch] text-center font-arbeit text-[clamp(2.6rem,5.2vw,6.2rem)] font-light leading-[1.04] tracking-[-0.05em] max-[760px]:mx-0 max-[760px]:max-w-none max-[760px]:text-left"
						data-problem-heading=""
					>
						{creditLines.map((line, lineIndex) => (
							<ProblemLine
								line={line}
								lineIndex={lineIndex}
								key={lineIndex}
							/>
						))}
					</h2>

					<p className="verevon-body-lg mx-auto mt-[clamp(32px,4vw,56px)] max-w-[640px] text-center text-pretty max-[760px]:mx-0 max-[760px]:text-left">
						{supportingCopy}
					</p>
				</div>
			</div>

			<div className="relative z-10 px-[var(--verevon-edge)] pb-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]">
				<Reveal delay={90}>
					<div
						className="flex flex-wrap items-end justify-center gap-x-8 gap-y-2 max-[760px]:justify-start"
						aria-label="Problemområder"
						role="tablist"
					>
						{cards.map((card, index) => (
							<button
								aria-controls={`problemet-panel-${card.kicker}`}
								aria-selected={index === activeIndex}
								className={cn(
									"relative flex min-h-12 items-center justify-start border-b-2 border-transparent px-0 py-3 text-left font-protokoll text-[0.92rem] font-medium transition-colors duration-300 focus-visible:outline-2 focus-visible:outline-offset-3 focus-visible:outline-verevon-j-text",
									index === activeIndex
										? "text-verevon-j-text"
										: "text-verevon-text-muted hover:text-verevon-j-text",
								)}
								id={`problemet-tab-${card.kicker}`}
								key={card.kicker}
								data-problem-tab={index}
								onClick={() => selectTab(index)}
								onFocus={() => selectTab(index)}
								onKeyDown={(event) => handleTabKeyDown(event, index)}
								onMouseEnter={() => selectTab(index)}
								ref={(element) => {
									tabRefs.current[index] = element;
								}}
								role="tab"
								tabIndex={index === activeIndex ? 0 : -1}
								type="button"
							>
								{card.title}
								<AnimatePresence initial={false}>
									{index === activeIndex ? (
										<motion.span
											aria-hidden="true"
											className={cn(
												"absolute bottom-[-2px] h-[2px] bg-verevon-j-text",
												previousIndex === null || activeIndex > previousIndex
													? "left-0 origin-left"
													: "right-0 origin-right",
											)}
											initial={
												shouldReduceMotion
													? { opacity: 1, width: "100%" }
													: { opacity: 1, width: "0%" }
											}
											animate={{ opacity: 1, width: "100%" }}
											key={`incoming-underline-${activeIndex}`}
											transition={
												shouldReduceMotion
													? { duration: 0 }
													: {
														duration: 0.24,
														ease: [0.1, 1, 0.7, 1],
													}
											}
										/>
									) : null}

									{index === previousIndex && index !== activeIndex ? (
										<motion.span
											aria-hidden="true"
											className={cn(
												"absolute bottom-[-2px] h-[2px] bg-verevon-j-text",
												activeIndex > (previousIndex ?? activeIndex)
													? "right-0 origin-right"
													: "left-0 origin-left",
											)}
											initial={{ opacity: 1, width: "100%" }}
											animate={
												shouldReduceMotion
													? { opacity: 1, width: "0%" }
													: { opacity: 1, width: "0%" }
											}
											key={`outgoing-underline-${previousIndex}-${activeIndex}`}
											onAnimationComplete={() => {
												setPreviousIndex((current) =>
													current === index ? null : current,
												);
											}}
											transition={
												shouldReduceMotion
													? { duration: 0 }
													: {
														duration: 0.3,
														ease: [0.1, 1, 0.7, 1],
													}
											}
										/>
									) : null}
								</AnimatePresence>
							</button>
						))}
					</div>
				</Reveal>

				{/* One spot of 4 each by default, the active tab's card takes 2 —
				    same "one big, rest small" proportion as the reference, just
				    with a 3rd slot. flex-grow (not grid-column) so the resize
				    between tabs is a smooth width transition, not an instant
				    snap. Inactive cards drop their text entirely (image only,
				    per the reference) since the tab label above already names
				    them; only the active card carries kicker/title/body/CTA. */}
				<Reveal delay={160}>
					<div className="mt-8 flex h-[420px] gap-5 max-[760px]:h-auto max-[760px]:flex-col">
						{cards.map((card, index) => {
							const isActive = index === activeIndex;

							return (
								<div
									className={cn(
										"relative overflow-hidden rounded-[12px] transition-[flex-grow] duration-500 ease-out max-[760px]:w-full max-[760px]:flex-none max-[760px]:rounded-[10px]",
										isActive
											? "flex-[2] max-[760px]:h-[300px]"
											: "flex-1 max-[760px]:h-[150px]",
									)}
									aria-labelledby={`problemet-tab-${card.kicker}`}
									aria-label={!isActive ? `Vis ${card.title}` : undefined}
									id={`problemet-panel-${card.kicker}`}
									key={card.kicker}
									onClick={() => selectTab(index)}
									onKeyDown={(event) => {
										if (!isActive && (event.key === "Enter" || event.key === " ")) {
											event.preventDefault();
											selectTab(index);
										}
									}}
									onMouseEnter={() => playCardVideo(index)}
									onMouseLeave={() => pauseCardVideo(index)}
									role={isActive ? "tabpanel" : "button"}
									tabIndex={isActive ? undefined : 0}
								>
					{card.videos.length > 0 ? (
						<video
								className="absolute inset-0 size-full object-cover"
								muted
								onEnded={() => advanceCardVideo(index)}
								onTimeUpdate={(event) =>
									handleCardVideoTimeUpdate(index, event.currentTarget)
								}
								playsInline
								poster={card.image}
								preload="metadata"
								ref={(element) => {
									videoRefs.current[index] = element;
								}}
								src={card.videos[sequenceIndices[index]].src}
								style={{
									objectPosition: card.imagePosition ?? "center",
								}}
							>
							</video>
					) : (
										<Image
											alt=""
											className="object-cover"
											fill
											sizes="(max-width: 760px) 100vw, 50vw"
											src={card.image}
											style={{
												objectPosition: card.imagePosition ?? "center",
											}}
										/>
									)}

									<div
										aria-hidden="true"
										className="absolute inset-0 bg-[linear-gradient(180deg,transparent_38%,rgba(15,15,15,0.85)_100%)]"
									/>

									{isActive && (
										<div className="absolute inset-x-0 bottom-0 p-[clamp(20px,2.2vw,32px)]">
											<span className="font-protokoll text-[0.68rem] uppercase tracking-[0.16em] text-white/60">
												{card.kicker}
											</span>

											<h3 className="mt-2 max-w-[24ch] font-arbeit text-[clamp(1.3rem,1.7vw,1.9rem)] font-light leading-[1.08] tracking-[-0.03em] text-white text-pretty">
												{card.title}
											</h3>

											<p className="verevon-body mt-3 max-w-[36ch] text-white/78 text-pretty">
												{card.body}
											</p>

											<ArrowButton
												className="mt-5"
												href={card.href}
												variant="light"
											>
												{card.label}
											</ArrowButton>
										</div>
									)}
								</div>
							);
						})}
					</div>
				</Reveal>
			</div>
		</section>
	);
}

export default ProblemSection;
