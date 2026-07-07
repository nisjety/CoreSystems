"use client";

import Image from "next/image";
import { useLayoutEffect, useRef } from "react";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { ArrowButtonLabel } from "@/components/ui/ArrowButton";
import { VelionComposerPreview } from "@/components/ui/VelionComposerPreview";

gsap.registerPlugin(ScrollTrigger);

// One tile in the 4-card grid that appears at the end of the pinned sequence.
type WorkflowCard = {
	href: string;
	image: string;
	kicker: string;
	link: string;
	title: string;
	text: string;
};

// The prompt the typing animation plays inside the composer overlay.
const promptText =
	"Lag en løsning for dagens uløste kundesamtaler: svarutkast, kilder, prioritet og godkjenning.";

// The 4 result cards revealed once the "send" beat completes.
const workflowCards: WorkflowCard[] = [
	{
		href: "#kunnskap",
		image: "/velion-product-shots/chat-draft-answer-top.png",
		kicker: "Svar",
		link: "Skriv utkast",
		title: "Skriver utkast",
		text: "Lager utkast til e-post, chat og meldinger med synlige kildespor før noe sendes.",
	},
	{
		href: "#plattform",
		image: "/velion-product-shots/dashboard-composer-prompt-state.png",
		kicker: "Kunnskap",
		link: "Samle kilder",
		title: "Bygger kunnskap",
		text: "Kobler til nettsider, dokumenter og integrasjoner, og bygger et arbeidsminne mennesker kan inspisere.",
	},
	{
		href: "#produksjon",
		image: "/velion-product-shots/chat-agent-steps.png",
		kicker: "Styr",
		link: "Vurder hastegrad",
		title: "Ruter risiko",
		text: "Finner trege saker, klassifiserer hastegrad og spør riktig menneske før eskalering.",
	},
	{
		href: "/trust",
		image: "/velion-product-shots/inbox-empty-workspace.png",
		kicker: "Spor",
		link: "Revider handling",
		title: "Reviderer alt",
		text: "Registrerer godkjenninger, policy-sjekker, koblingsstatus og tilbakerulling for hver arbeidsflyt.",
	},
];

// Full-viewport headline overlay ("Tenking, satt i arbeid"). Revealed by
// introTimeline just before the pin engages, then sits behind everything
// else for the rest of the pinned sequence. Desktop-only (hidden <900px).
function FeatureIntro() {
	return (
		<div
			className="pointer-events-none absolute inset-0 z-10 opacity-0 max-[899px]:hidden"
			data-feature-intro=""
		>
			{/* Soft background wash behind the headline, faded in alongside it. */}
			<div className="absolute inset-0 opacity-0" data-feature-bg="">
				<div className="absolute inset-0 bg-background/92" />
				<div className="absolute inset-0 bg-[radial-gradient(circle_at_50%_34%,rgba(238,122,80,0.06),transparent_26%),radial-gradient(circle_at_12%_18%,rgba(79,125,243,0.08),transparent_24%),linear-gradient(180deg,rgba(23,23,23,0.018),transparent_44%)]" />
			</div>

			{/* Eyebrow + heading + subtext. Children are staggered in by introTimeline. */}
			<div
				className="absolute left-1/2 top-[clamp(120px,15vh,168px)] w-[min(760px,calc(100%-48px))] -translate-x-1/2 text-center"
				data-feature-copy=""
			>
				<p className="mb-4 font-protokoll text-[0.72rem] font-medium uppercase leading-none tracking-[0.34em] text-velion-j-text/38">
					Forankret handling
				</p>

				<h2 className="m-0 font-arbeit text-[clamp(2.75rem,4vw,5.35rem)] font-light leading-[0.94] tracking-[-0.068em] text-velion-j-text">
					Tenking, satt i arbeid
				</h2>

				<p className="mx-auto mt-4 max-w-[540px] font-protokoll text-[clamp(0.95rem,0.95vw,1.08rem)] font-light leading-[1.42] text-velion-text-muted">
					Ett kundesignal kan bli et svar, en rute, en policy-sjekk og
					en revidert handling — alt med kildene synlige.
				</p>
			</div>
		</div>
	);
}

// The small "signal" card — represents raw customer context arriving,
// before Velion turns it into a draft. First thing to appear once the pin
// engages; later shrinks and fades as the prompt composer takes over.
function FeatureSourceCard() {
	const corners = [
		"left-[-3.5px] top-[-3.5px]",
		"right-[-3.5px] top-[-3.5px]",
		"bottom-[-3.5px] left-[-3.5px]",
		"bottom-[-3.5px] right-[-3.5px]",
	];

	return (
		<div
			className="absolute left-1/2 top-[clamp(330px,40vh,420px)] z-30 aspect-[3/4] w-[min(24vh,240px)] -translate-x-1/2 opacity-0 max-[899px]:hidden"
			data-feature-source=""
		>
			<div className="absolute inset-x-0 top-[-20px] flex h-[15px] select-none items-center justify-between font-protokoll text-[10px] leading-none text-velion-j-text/38">
				<span className="flex items-center gap-1">
					<span className="size-[5px] rounded-full bg-velion-coral/70" />
					signal
				</span>
				<span>kundekontekst</span>
			</div>

			<div className="relative h-full w-full overflow-hidden border border-velion-j-text/10 bg-[#f8f8f7] shadow-[0_10px_28px_rgba(23,23,23,0.035)]">
				<Image
					alt="Kundekontekst visualisert som et signal før Velion lager forslag."
					className="object-cover opacity-70 saturate-[0.72]"
					fill
					sizes="260px"
					src="/velion-vibe/human-haze.png"
				/>
			</div>

			{/* Decorative corner brackets, purely visual. */}
			{corners.map((position) => (
				<span
					aria-hidden="true"
					className={[
						"absolute size-[6px] border border-velion-j-text/14 bg-background",
						position,
					].join(" ")}
					key={position}
				/>
			))}
		</div>
	);
}

// The chat-composer overlay showing the prompt "typing" itself out. `top` is
// derived from the source card's own position + size (not an independent
// clamp) so the two can never overlap or drift apart at any viewport size —
// see FeatureSourceCard above for the values this chains off of.
function FeaturePromptComposer() {
	return (
		<div
			className="absolute left-1/2 top-[calc(clamp(330px,40vh,420px)_+_min(32vh,320px)_+_20px)] z-40 w-[min(91vw,880px)] origin-top -translate-x-1/2 opacity-0 max-[899px]:hidden"
			data-feature-prompt=""
		>
			<VelionComposerPreview animateCharacters prompt={promptText} />
		</div>
	);
}

// One tile in the 4-card result grid. `animated` controls whether it's
// wired into the scroll-scrubbed reveal (desktop) or renders as a plain,
// static link (mobile fallback, where nothing is scroll-animated).
function FeatureOutputCard({
	animated = true,
	card,
}: {
	animated?: boolean;
	card: WorkflowCard;
}) {
	return (
		<a
			aria-label={card.link}
			className="group flex min-w-0 flex-col gap-3"
			href={card.href}
			{...(animated ? { "data-feature-card": "" } : {})}
		>
			<div className="flex h-[15px] select-none items-center justify-between font-protokoll text-[10px] leading-none text-velion-j-text/42">
				<span className="flex items-center gap-1">
					<span className="size-[5px] rounded-full bg-velion-coral/70" />
					{card.kicker}
				</span>
				<span>Velion-resultat</span>
			</div>

			<div className="relative aspect-[3/4] overflow-hidden border border-velion-j-text/8 bg-white/55 shadow-[0_10px_28px_rgba(23,23,23,0.035)]">
				<Image
					alt=""
					className="select-none object-cover opacity-[0.76] saturate-[0.72] transition-transform duration-700 group-hover:scale-[1.035]"
					draggable={false}
					fill
					sizes="(max-width: 899px) 92vw, 22vw"
					src={card.image}
				/>

				<div className="absolute inset-0 bg-[linear-gradient(180deg,rgba(248,248,247,0.08),rgba(248,248,247,0.72))]" />

				<div className="absolute bottom-4 left-4 right-4">
					<h3 className="m-0 font-arbeit text-[clamp(1.45rem,2vw,2.2rem)] font-light leading-[1.04] tracking-[-0.055em] text-velion-j-text">
						{card.title}
					</h3>
				</div>
			</div>

			<p className="m-0 min-h-[4.2em] font-protokoll text-[clamp(0.92rem,0.92vw,1.02rem)] font-light leading-[1.38] text-velion-text-muted/90">
				{card.text}
			</p>

			<ArrowButtonLabel className="text-velion-j-text/62">
				{card.link}
			</ArrowButtonLabel>
		</a>
	);
}

// The 4-card grid, revealed last in the pinned sequence (the "send" action's
// payoff). Width caps at 1440px on wide screens and at a height-derived
// floor (`214svh - 706px`, min 700px) on short ones, so cards never blow
// past a comfortable size in either dimension. Desktop-only.
function FeatureOutputStage() {
	return (
		<div
			className="absolute bottom-[clamp(30px,7vh,45px)] left-1/2 z-40 grid w-[min(90vw,1440px,max(700px,calc(214svh_-_706px)))] -translate-x-1/2 grid-cols-4 gap-[clamp(12px,1.25vw,22px)] opacity-0 max-[899px]:hidden"
			data-feature-output-stage=""
		>
			{workflowCards.map((card) => (
				<FeatureOutputCard card={card} key={card.title} />
			))}
		</div>
	);
}

// Below 900px there's no pin, no scrub, no GSAP at all — just this plain,
// normally-scrolling stack: heading, source card, composer, then the 4
// result cards. Renders in parallel with the desktop markup below and is
// toggled purely by the `max-[899px]:` / default Tailwind breakpoints.
function FeatureMobileFallback() {
	return (
		<div className="hidden max-[899px]:grid max-[899px]:gap-10 max-[899px]:pt-10">
			<div className="text-center">
				<p className="mb-4 font-protokoll text-[0.7rem] font-medium uppercase leading-none tracking-[0.3em] text-velion-j-text/38">
					Forankret handling
				</p>

				<h2 className="m-0 font-arbeit text-[clamp(3rem,14vw,5rem)] font-light leading-[0.95] tracking-[-0.07em] text-velion-j-text">
					Kundearbeid, klart til godkjenning
				</h2>

				<p className="mx-auto mt-5 max-w-[34rem] font-protokoll text-[1rem] font-light leading-[1.48] text-velion-text-muted">
					Velion samler kilder, lager forslag og viser hva som må
					godkjennes før noe skjer ute hos kunden.
				</p>
			</div>

			<div className="mx-auto grid w-full max-w-[760px] gap-8">
				<div className="relative mx-auto aspect-[3/4] w-[min(72vw,270px)] border border-velion-j-text/10 bg-[#f8f8f7] shadow-[0_10px_28px_rgba(23,23,23,0.035)]">
					<Image
						alt="Kundekontekst visualisert som et signal før Velion lager forslag."
						className="object-cover opacity-70 saturate-[0.72]"
						fill
						sizes="72vw"
						src="/velion-vibe/human-haze.png"
					/>
				</div>

				<VelionComposerPreview prompt={promptText} />
			</div>

			<div className="grid gap-8">
				{workflowCards.map((card) => (
					<FeatureOutputCard
						animated={false}
						card={card}
						key={card.title}
					/>
				))}
			</div>
		</div>
	);
}

export function FeatureCardsSection() {
	const sectionRef = useRef<HTMLElement>(null);

	useLayoutEffect(() => {
		const section = sectionRef.current;

		if (!section) {
			return;
		}

		const reduceMotion = window.matchMedia(
			"(prefers-reduced-motion: reduce)",
		).matches;

		// Grab every element the two timelines below animate, by data-attribute.
		const pin = section.querySelector<HTMLElement>("[data-feature-pin]");
		const featureIntro = section.querySelector<HTMLElement>(
			"[data-feature-intro]",
		);
		const featureBg =
			section.querySelector<HTMLElement>("[data-feature-bg]");
		const featureCopy = section.querySelector<HTMLElement>(
			"[data-feature-copy]",
		);
		const source = section.querySelector<HTMLElement>(
			"[data-feature-source]",
		);
		const prompt = section.querySelector<HTMLElement>(
			"[data-feature-prompt]",
		);
		const outputStage = section.querySelector<HTMLElement>(
			"[data-feature-output-stage]",
		);
		const processing = section.querySelector<HTMLElement>(
			"[data-feature-processing]",
		);
		const sendArrow = section.querySelector<HTMLElement>(
			"[data-feature-send-arrow]",
		);
		const sendButton = section.querySelector<HTMLElement>(
			"[data-feature-send-button]",
		);
		const sendRing = section.querySelector<HTMLElement>(
			"[data-feature-send-ring]",
		);
		const cards = Array.from(
			section.querySelectorAll<HTMLElement>("[data-feature-card]"),
		);
		const chars = Array.from(
			section.querySelectorAll<HTMLElement>("[data-feature-char]"),
		);

		// Below 900px width, FeatureMobileFallback renders instead — bail out
		// of the whole pin/scrub setup rather than fight the mobile layout.
		// Also bails if any target is missing (e.g. markup changed elsewhere).
		if (
			window.innerWidth < 900 ||
			!pin ||
			!featureIntro ||
			!featureBg ||
			!featureCopy ||
			!source ||
			!prompt ||
			!outputStage ||
			!processing ||
			!sendArrow ||
			!sendButton ||
			!sendRing ||
			cards.length === 0
		) {
			return;
		}

		// Custom fonts loading after the initial layout pass can shift text
		// height, which shifts where "top top" / "top 44%" etc. actually land.
		// Re-measure once they're in.
		document.fonts?.ready.then(() => ScrollTrigger.refresh());

		const featureCopyItems = Array.from(featureCopy.children).filter(
			(child): child is HTMLElement => child instanceof HTMLElement,
		);

		// prefers-reduced-motion: skip both timelines entirely and just show
		// the finished state (cards visible, nothing pinned).
		if (reduceMotion) {
			const context = gsap.context(() => {
				gsap.set([featureIntro, featureBg, featureCopy, outputStage], {
					autoAlpha: 1,
				});
				gsap.set(featureCopyItems, { autoAlpha: 1, y: 0 });
				gsap.set(outputStage, { pointerEvents: "auto" });
				gsap.set(cards, {
					autoAlpha: 1,
					filter: "none",
					rotationX: 0,
					scale: 1,
					xPercent: 0,
					y: 0,
				});
				gsap.set([source, prompt, processing, sendRing], {
					autoAlpha: 0,
				});
			}, section);

			return () => context.revert();
		}

		const context = gsap.context(() => {
			// Starting ("hidden") state for every animated element, before either
			// timeline below has run. `force3D` promotes each to its own
			// compositor layer only while GSAP is actively transforming it —
			// deliberately not paired with a permanent `will-change` class.
			gsap.set(featureIntro, { autoAlpha: 0 });
			gsap.set(featureBg, { autoAlpha: 0 });
			gsap.set(featureCopy, {
				autoAlpha: 1,
				filter: "blur(6px)",
				y: 0,
				force3D: true,
			});
			gsap.set(featureCopyItems, {
				autoAlpha: 0,
				y: 32,
				force3D: true,
			});

			gsap.set(source, {
				autoAlpha: 0,
				y: 6,
				force3D: true,
			});

			gsap.set(prompt, {
				autoAlpha: 0,
				scale: 0.94,
				y: 30,
				force3D: true,
			});

			gsap.set(chars, { autoAlpha: 0 });

			gsap.set(outputStage, {
				autoAlpha: 0,
				perspective: 1400,
				pointerEvents: "none",
				transformStyle: "preserve-3d",
			});

			gsap.set(sendButton, {
				backgroundColor: "#111111",
				scale: 1,
				transformOrigin: "50% 50%",
				force3D: true,
			});

			gsap.set(sendArrow, {
				y: 0,
				force3D: true,
			});

			gsap.set(sendRing, {
				autoAlpha: 0,
				scale: 0.86,
				transformOrigin: "50% 50%",
			});

			gsap.set(processing, {
				autoAlpha: 0,
				scaleX: 0,
				transformOrigin: "0% 50%",
			});

			gsap.set(cards, {
				autoAlpha: 0,
				backfaceVisibility: "hidden",
				rotationX: -42,
				scale: 1.08,
				transformOrigin: "50% 0%",
				transformStyle: "preserve-3d",
				xPercent: (index) => [142, 48, -48, -142][index] ?? 0,
				y: -14,
				force3D: true,
			});

			// Headline reveal ("Tenking, satt i arbeid"). Scroll-scrubbed (not a
			// fixed-duration play-once animation) so it stays in sync with scroll
			// speed instead of drifting out of sync with the pin below it.
			// start "top 44%": fires ~60px after the hero's own fog effect starts,
			// so the two read as one continuous handoff rather than racing.
			// end "top 2%": fully revealed just before the pin engages at
			// "top top" — the reveal finishes right as the card sequence takes over.
			const introTimeline = gsap.timeline({
				defaults: { ease: "power2.out" },
				scrollTrigger: {
					trigger: section,
					start: "top 44%",
					end: "top 2%",
					scrub: 0.25,
					invalidateOnRefresh: true,
					id: "velion-feature-intro-fade",
				},
			});

			introTimeline
				.to(featureIntro, { autoAlpha: 1, duration: 0.52 }, 0)
				.to(featureBg, { autoAlpha: 1, duration: 0.52 }, 0)
				.to(
					featureCopy,
					{
						filter: "blur(0px)",
						duration: 0.62,
					},
					0,
				)
				.to(
					featureCopyItems,
					{
						autoAlpha: 1,
						y: 0,
						duration: 0.78,
						ease: "power3.out",
						stagger: 0.09,
					},
					0.06,
				);

			// The main pinned sequence: source card -> prompt types -> send ->
			// cards reveal. `end` is a fixed pixel distance (1.71x viewport
			// height) tuned so the timeline's positions below map to a
			// gapless scroll with no dead stretches and no wasted space —
			// changing that multiplier changes the felt speed of every beat,
			// so if you retune it, rescale the position values together with it.
			const timeline = gsap.timeline({
				defaults: { ease: "none" },
				scrollTrigger: {
					trigger: section,
					start: "top top",
					end: () => `+=${Math.round(window.innerHeight * 1.71)}`,
					pin,
					scrub: 0.45,
					anticipatePin: 1,
					invalidateOnRefresh: true,
					refreshPriority: 3,
					id: "velion-feature-cards-scroll",
				},
			});

			timeline
				// Beat 1 (0 -> 0.26): the source "signal" card fades in immediately
				// as the pin engages (no lead-in gap), then settles/shrinks to make
				// room for the prompt composer arriving right behind it.
				.to(
					source,
					{
						autoAlpha: 1,
						y: 0,
						duration: 0.12,
					},
					0,
				)
				.to(source, { y: -18, scale: 0.965, duration: 0.14 }, 0.12)
				// Beat 2 (0.14 -> 0.38): the prompt composer appears and its text
				// types itself out character by character (`chars`, staggered).
				.to(
					prompt,
					{
						autoAlpha: 1,
						y: 0,
						scale: 1,
						duration: 0.18,
					},
					0.14,
				)
				.to(
					chars,
					{
						autoAlpha: 1,
						duration: 0.18,
						stagger: 0.002,
					},
					0.2,
				)
				// Beat 3 (0.44 -> 0.56): the send button presses, a ring pulses out,
				// and the processing bar fills — then everything resets to idle.
				.to(
					sendButton,
					{
						backgroundColor: "#ee7a50",
						scale: 0.88,
						duration: 0.06,
					},
					0.44,
				)
				.to(sendArrow, { y: -3, duration: 0.06 }, 0.44)
				.to(
					sendRing,
					{
						autoAlpha: 1,
						scale: 1.12,
						duration: 0.08,
					},
					0.45,
				)
				.to(
					processing,
					{
						autoAlpha: 1,
						scaleX: 1,
						duration: 0.14,
					},
					0.49,
				)
				.to(
					sendButton,
					{
						backgroundColor: "#111111",
						scale: 1,
						duration: 0.1,
					},
					0.54,
				)
				.to(sendArrow, { y: 0, duration: 0.1 }, 0.54)
				.to(
					sendRing,
					{
						autoAlpha: 0,
						scale: 1.45,
						duration: 0.12,
					},
					0.55,
				)
				// Beat 4 (0.56 -> 0.74): source and prompt shrink/fade out while the
				// 4-card grid flips up into place (blur was removed here on purpose —
				// scrubbed filter animations are expensive; the 3D flip alone reads
				// as depth without the repaint cost).
				.to(
					prompt,
					{
						autoAlpha: 0.18,
						y: 76,
						scale: 0.92,
						duration: 0.08,
					},
					0.56,
				)
				.to(
					source,
					{
						autoAlpha: 0.42,
						scale: 0.76,
						y: 4,
						duration: 0.08,
					},
					0.56,
				)
				.to(outputStage, { autoAlpha: 1, duration: 0.06 }, 0.58)
				.set(outputStage, { pointerEvents: "auto" }, 0.58)
				.to(
					cards,
					{
						autoAlpha: 0.78,
						stagger: 0.035,
						duration: 0.04,
					},
					0.58,
				)
				.to(
					cards,
					{
						scale: 1,
						xPercent: 0,
						y: 0,
						stagger: 0.04,
						duration: 0.24,
					},
					0.59,
				)
				.to(
					cards,
					{
						rotationX: 0,
						stagger: 0.04,
						duration: 0.3,
					},
					0.59,
				)
				.to(cards, { autoAlpha: 1, duration: 0.05 }, 0.74)
				.to(source, { autoAlpha: 0, duration: 0.06 }, 0.65)
				.to(prompt, { autoAlpha: 0, duration: 0.06 }, 0.65)
				// Small trailing hold so the pin doesn't release the instant the
				// cards finish settling — a brief rest after the motion, not dead
				// time before it (see the beat-1 comment above for why that
				// distinction matters at the start vs. the end of the sequence).
				.to({}, { duration: 0.07 });
		}, section);

		return () => context.revert();
	}, []);

	return (
		<section
			aria-labelledby="feature-cards-title"
			className="relative isolate overflow-hidden bg-background text-velion-j-text"
			id="feature-cards"
			ref={sectionRef}
		>
			{/* Anchor for the navbar's "Produkt" link; not a visible element. */}
			<span
				aria-hidden="true"
				className="absolute left-0 top-0 h-px w-px"
				id="produkt"
			/>

			{/* The pinned stage: exactly one viewport tall, everything inside is
			    absolutely positioned and cross-faded by the timelines above. */}
			<div
				className="relative min-h-svh overflow-hidden max-[899px]:min-h-0"
				data-feature-pin=""
			>
				<div className="relative min-h-svh overflow-hidden max-[899px]:grid max-[899px]:min-h-0 max-[899px]:gap-12 max-[899px]:px-[var(--velion-page-pad)] max-[899px]:py-24">
					{/* Decorative background grid lines (desktop only). */}
					<div
						aria-hidden="true"
						className="absolute inset-0 bg-[linear-gradient(90deg,rgba(23,23,23,0.055)_1px,transparent_1px),linear-gradient(180deg,rgba(23,23,23,0.048)_1px,transparent_1px)] bg-[length:calc(100%/4)_calc(100%/3),calc(100%/4)_calc(100%/3)] max-[899px]:bg-[length:92px_92px]"
					/>

					{/* Soft radial/linear wash over the grid lines. */}
					<div
						aria-hidden="true"
						className="absolute inset-0 bg-[radial-gradient(circle_at_64%_40%,rgba(238,122,80,0.045),transparent_30%),linear-gradient(180deg,rgba(248,248,247,0),rgba(248,248,247,0.72))]"
					/>

					<FeatureIntro />

					{/* Vertical rule marking the pinned stage's left/right edges. */}
					<div
						aria-hidden="true"
						className="pointer-events-none absolute inset-x-4 bottom-0 top-0 border-x border-velion-j-text/6 max-[899px]:hidden"
					/>

					<FeatureSourceCard />
					<FeaturePromptComposer />
					<FeatureOutputStage />

					{/* Four decorative corner/edge dots marking the stage's thirds. */}
					<div
						aria-hidden="true"
						className="pointer-events-none absolute inset-0 max-[899px]:hidden"
					>
						<span className="absolute left-4 top-[calc(100%/3)] size-[7px] -translate-y-1/2 rounded-full bg-velion-j-text/18" />
						<span className="absolute bottom-[calc(100%/3)] left-4 size-[7px] translate-y-1/2 rounded-full bg-velion-j-text/18" />
						<span className="absolute right-4 top-[calc(100%/3)] size-[7px] -translate-y-1/2 rounded-full bg-velion-j-text/18" />
						<span className="absolute bottom-[calc(100%/3)] right-4 size-[7px] translate-y-1/2 rounded-full bg-velion-j-text/18" />
					</div>

					<FeatureMobileFallback />
				</div>
			</div>

			{/* Real (non-decorative) heading for screen readers/SEO — the visual
			    headline above is presentational and lives inside FeatureIntro. */}
			<h2 className="sr-only" id="feature-cards-title">
				Kundearbeid klart til godkjenning
			</h2>
		</section>
	);
}

export default FeatureCardsSection;
