"use client";

import Image from "next/image";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import type { KeyboardEvent } from "react";
import { useLayoutEffect, useRef, useState } from "react";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { ArrowButton } from "@/components/ui/ArrowButton";
import { Eyebrow } from "@/components/ui/SectionHeading";
import { EditorialGrid } from "@/components/home/extras/EditorialGrid";
import { Reveal } from "@/components/home/sections/Reveal";
import { cn } from "@/lib/utils";

gsap.registerPlugin(ScrollTrigger);

type IndexCard = {
	body: string;
	href: string;
	image: string;
	kicker: string;
	label: string;
	title: string;
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

// A Wolverine-style "image galaxy" scattered THROUGH the credo's full pinned
// frame (not clustered in the margins) — one grounded anchor photo (a lone
// door on a hillside, a threshold/decision image, not a stock "office"
// cliché) plus small faded stills that echo the brand's own abstract
// renders (soft-orb.png, a winter light-path, a twisted ribbon, a backlit
// bird, dune light, clouds). Desktop-only (max-[760px]:hidden below) —
// there isn't room to scatter imagery once the statement wraps to full
// width and the pin gives way to a plain stacked layout.
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
		src: "/0c49ef4d22d3f1ad1b49e59e374e1921.jpg",
		alt: "",
		width: 100,
		aspect: "aspect-[3/4]",
		rotate: -3,
		className: "left-[13%] top-[8%] opacity-45",
	},
	{
		src: "/b41e54607ac9cc5f6424268082c19bec.jpg",
		alt: "",
		width: 95,
		aspect: "aspect-[3/4]",
		rotate: -4,
		className: "right-[19%] top-[24%] opacity-40",
	},
	{
		src: "/00631c87cef97a24798fbee7c406201d.jpg",
		alt: "",
		width: 60,
		aspect: "aspect-square",
		rotate: -7,
		className: "left-[45%] top-[3%] opacity-25",
	},
	{
		src: "/soft-orb.png",
		alt: "",
		width: 75,
		aspect: "aspect-[4/3]",
		rotate: 4,
		className: "left-[5%] top-[46%] opacity-30",
	},
	{
		src: "/agent-run-console-running.jpg",
		alt: "",
		width: 85,
		aspect: "aspect-[3/4]",
		rotate: 5,
		className: "right-[13%] top-[48%] opacity-28",
	},
	{
		src: "/27aab72a25a11d3d63d1302d8d310515.jpg",
		alt: "",
		width: 90,
		aspect: "aspect-[3/4]",
		rotate: 6,
		className: "bottom-[10%] left-[23%] opacity-25",
	},
	{
		src: "/71fc97238af756817bf76c9ad6230a99.jpg",
		alt: "",
		width: 80,
		aspect: "aspect-[4/3]",
		rotate: -5,
		className: "bottom-[8%] right-[27%] opacity-22",
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

// The 3 index cards below the credo — each one points into a later section
// (or /trust) instead of restating the thesis, so this section stays a
// single, uncluttered move: state the problem, then hand off. Rendered as a
// tab-switched trio (see ProblemSection below): each `image` was picked for
// its own card's specific claim, not reused from the galaxy above — mist for
// an answer with no ground underneath it, a bird breaking into flight for
// action taken on its own, and a quiet, empty desk for knowledge that's
// sitting there unused.
const cards: IndexCard[] = [
	{
		kicker: "01",
		title: "Svar uten kilder",
		body: "Et svar uten synlig kilde er en gjetning med god selvtillit.",
		href: "#kunnskap",
		label: "Se hvordan kildene vises",
		image: "/mist-bloom.png",
	},
	{
		kicker: "02",
		title: "Handling uten godkjenning",
		body: "Automatisering som sender selv, er en risiko ingen har bedt om.",
		href: "/trust",
		label: "Se godkjenningsmodellen",
		image: "/warm-flight.png",
	},
	{
		kicker: "03",
		title: "Kunnskap som ikke brukes",
		body: "Det som ligger i dokumenter og innbokser, hjelper ingen før det er koblet til arbeidet.",
		href: "#flyt",
		label: "Se arbeidsflyten",
		image: "/DESIGN.jpg",
	},
];

// The credo, split into words for the scroll-linked reveal below — each
// word is its own inline-block span so GSAP can animate color/blur/opacity
// per word as the pinned section scrolls past (the same mechanic
// Wonderful.ai uses for its manifesto statement).
const creditLines: string[][] = [
	"Svar er billige nå.".split(" "),
	"Handling som kan godkjennes, er det som mangler.".split(" "),
];

function ProblemLine({ line, lineIndex }: { line: string[]; lineIndex: number }) {
	return (
		<span className="block">
			{line.map((word, wordIndex) => (
				<span
					className="mr-[0.22em] inline-block will-change-[color,filter,opacity]"
					data-problem-word=""
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
 * credo is dim and blurred, sharpens to full color/clarity as the heading
 * crosses the viewport's center, then dims and blurs again as it continues
 * past — a symmetric bell-curve reveal (Wonderful.ai's manifesto mechanic,
 * minus the pin), not a one-way fade that stays revealed. A small image
 * galaxy is scattered through that same frame (Wolverine's "portfolio"
 * device: photos threaded through big statement type, not confined to the
 * margins) and does not participate in the fade. Reduced motion skips the
 * animation and shows the finished, fully legible state. The supporting
 * paragraph and the 3 index cards follow in normal flow — the whole job
 * here is to land the thesis, then hand off into the rest of the page.
 */
export function ProblemSection() {
	const sectionRef = useRef<HTMLDivElement>(null);
	const tabRefs = useRef<Array<HTMLButtonElement | null>>([]);
	const [activeIndex, setActiveIndex] = useState(0);
	const [previousIndex, setPreviousIndex] = useState<number | null>(null);
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
			"[data-problem-word]",
			section,
		);

		if (reduceMotion || !heading || words.length === 0) {
			gsap.set(words, {
				color: "var(--velion-j-text)",
				filter: "blur(0px)",
				opacity: 1,
			});
			return;
		}

		// Keep the reveal atmospheric without making the statement unreadable.
		// A solid muted token preserves WCAG contrast in the dimmed state.
		const dimState = {
			color: "var(--velion-text-muted)",
			filter: "blur(0px)",
		};
		const sharpState = {
			color: "var(--velion-j-text)",
			filter: "blur(0px)",
		};

		const context = gsap.context(() => {
			gsap.set(words, { ...dimState, opacity: 1 });

			// trigger = the heading itself (not the taller section) — with
			// start "top bottom" / end "bottom top", the scroll distance
			// covered is exactly (viewport height + heading height), so
			// scroll-progress 0.5 always lands on the heading's own center
			// crossing the viewport's center, regardless of its height.
			//
			// Three-stage timeline: dim->sharp, a hold at full sharpness,
			// then sharp->dim. Stage 1 and stage 3 share the same
			// duration/stagger and neither has an explicit position, so
			// GSAP's default sequencing (a tween starts only once every
			// target in the previous one — including its stagger spread —
			// has fully finished) keeps them symmetric around the timeline's
			// own midpoint. The HOLD is inserted as a "+=" position offset
			// on stage 3 only, which pushes stage 3 out without touching
			// where stage 1 ends — so it just widens the peak into a
			// plateau centered on that same midpoint, instead of shifting
			// it. That's what keeps scroll-progress 0.5 landing in the
			// middle of the hold (deliberately avoided GSAP's percentage
			// `keyframes` object here instead: combined with `stagger`
			// across multiple targets it did not reliably animate in
			// testing).
			const HOLD = 0.3;

			const timeline = gsap.timeline({
				scrollTrigger: {
					trigger: heading,
					start: "top bottom",
					end: "bottom top",
					scrub: 0.5,
					invalidateOnRefresh: true,
					id: "velion-problem-word-reveal",
				},
			});

			timeline
				.to(words, {
					...sharpState,
					duration: 0.5,
					ease: "none",
					stagger: { each: 0.045, from: "start" },
				})
				.to(
					words,
					{
						...dimState,
						duration: 0.5,
						ease: "none",
						stagger: { each: 0.045, from: "start" },
					},
					`+=${HOLD}`,
				);
		}, section);

		document.fonts?.ready.then(() => ScrollTrigger.refresh());

		return () => context.revert();
	}, []);

	return (
		<section
			className="relative isolate overflow-visible bg-background text-velion-j-text"
			id="problemet"
		>
			<EditorialGrid className="absolute inset-0 z-0" tone="light" />

			{/* Plain, normal-flow frame — no sticky, no pin, no extra scroll
			    room. min-h gives the galaxy room to scatter; the section
			    otherwise scrolls exactly like any other on the page. */}
			<div
				className="group/galaxy relative flex min-h-[85svh] flex-col items-center justify-center px-[var(--velion-edge)] py-24 max-[760px]:min-h-0 max-[760px]:items-start max-[760px]:px-[var(--velion-page-pad)] max-[760px]:py-16"
				ref={sectionRef}
			>
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
				</div>
			</div>

			<div className="relative z-10 px-[var(--velion-edge)] pb-[var(--velion-section-vpad)] pt-[clamp(24px,3vw,44px)] max-[760px]:px-[var(--velion-page-pad)]">
				<Reveal>
					<p className="velion-body-lg mx-auto max-w-[640px] text-center text-pretty max-[760px]:mx-0 max-[760px]:text-left">
						Alle kan koble på en språkmodell. Færre kan vise kilden, la
						et menneske si ja — og likevel handle i tide.
					</p>
				</Reveal>

				<Reveal delay={90}>
					<div
						className="mt-[clamp(40px,5vw,64px)] flex flex-wrap items-end justify-center gap-x-8 gap-y-2 max-[760px]:justify-start"
						aria-label="Problemområder"
						role="tablist"
					>
						{cards.map((card, index) => (
							<button
								aria-controls={`problemet-panel-${card.kicker}`}
								aria-selected={index === activeIndex}
								className={cn(
									"relative flex min-h-12 items-center justify-start border-b-2 border-transparent px-0 py-3 text-left font-protokoll text-[0.92rem] font-medium transition-colors duration-300 focus-visible:outline-2 focus-visible:outline-offset-3 focus-visible:outline-velion-j-text",
									index === activeIndex
										? "text-velion-j-text"
										: "text-velion-text-muted hover:text-velion-j-text",
								)}
								id={`problemet-tab-${card.kicker}`}
								key={card.kicker}
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
												"absolute bottom-[-2px] h-[2px] bg-velion-j-text",
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
												"absolute bottom-[-2px] h-[2px] bg-velion-j-text",
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
					<div className="mt-8 flex h-[340px] gap-4 max-[760px]:h-auto max-[760px]:flex-col">
						{cards.map((card, index) => {
							const isActive = index === activeIndex;

							return (
								<div
									className={cn(
										"relative overflow-hidden rounded-[28px] transition-[flex-grow] duration-500 ease-out max-[760px]:w-full max-[760px]:flex-none",
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
									role={isActive ? "tabpanel" : "button"}
									tabIndex={isActive ? undefined : 0}
								>
									<Image
										alt=""
										className="object-cover"
										fill
										sizes="(max-width: 760px) 100vw, 50vw"
										src={card.image}
									/>

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

											<p className="velion-body mt-3 max-w-[36ch] text-white/78 text-pretty">
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
