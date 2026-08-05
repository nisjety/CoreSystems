"use client";

import Image from "next/image";
import { useLayoutEffect, useRef } from "react";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import {
	FeatureFilmGroup,
} from "./FeatureCardFilms";
import { FeatureComposerCycle } from "./FeatureComposerCycle";
import {
	FeatureWorkflowCard,
	workflowCards,
} from "./FeatureWorkflowCards";

gsap.registerPlugin(ScrollTrigger);

// Full-viewport headline overlay ("Én plattform. Flere arbeidsflater."). Revealed by
// introTimeline just before the pin engages, then sits behind everything
// else for the rest of the pinned sequence. Desktop-only (hidden <1141px).
function FeatureIntro() {
	return (
		<div
			aria-hidden="true"
			className="pointer-events-none absolute inset-0 z-10 opacity-0 max-[1140px]:hidden"
			data-feature-intro=""
		>
			{/* Soft background wash behind the headline, faded in alongside it. */}
			<div className="absolute inset-0 opacity-0" data-feature-bg="">
				<div className="absolute inset-0 bg-background/92" />
				<div className="absolute inset-0 bg-[radial-gradient(circle_at_50%_34%,rgba(238,122,80,0.06),transparent_26%),radial-gradient(circle_at_12%_18%,rgba(79,125,243,0.08),transparent_24%),linear-gradient(180deg,rgba(23,23,23,0.018),transparent_44%)]" />
			</div>

			{/* Eyebrow + heading + subtext. Children are staggered in by introTimeline. */}
			<div
				className="absolute left-1/2 top-[clamp(150px,16vh,196px)] w-[min(760px,calc(100%-48px))] -translate-x-1/2 text-center"
				data-feature-copy=""
			>
				<p className="mb-4 font-protokoll text-[0.72rem] font-medium uppercase leading-none tracking-[0.34em] text-verevon-j-text/38">
					Verevon-arbeidsflyten
				</p>

				<h2 className="m-0 font-arbeit text-[clamp(2.75rem,4vw,5.35rem)] font-light leading-[0.94] tracking-[-0.068em] text-verevon-j-text">
					Én plattform. Flere arbeidsflater.
				</h2>

				<p className="mx-auto mt-4 max-w-[650px] font-protokoll text-[clamp(0.95rem,0.95vw,1.08rem)] font-light leading-[1.42] text-verevon-text-muted">
					Verevon samler agenten, kunnskapen, systemene og kontrollen rundt arbeidet dere allerede gjør.
				</p>

				<div aria-label="Chat, Crawl og Søk koblet til kunnskap, agenter og kontroll." className="mx-auto mt-6 flex max-w-[680px] flex-wrap items-center justify-center gap-x-3 gap-y-2 font-protokoll text-[0.62rem] font-medium uppercase tracking-[0.17em] text-verevon-j-text/48">
					<span>Start her</span>
					<span className="text-verevon-j-text/72">Chat · Crawl · Søk</span>
					<span aria-hidden="true" className="text-verevon-coral/70">→</span>
					<span>Derfra videre</span>
					<span className="text-verevon-j-text/72">Kunnskap · agenter · kontroll</span>
				</div>
			</div>
		</div>
	);
}

// The small task card is intentionally generic: it represents any incoming
// piece of work before Verevon turns it into a grounded proposal.
function FeatureSourceCard() {
	const corners = [
		"left-[-3.5px] top-[-3.5px]",
		"right-[-3.5px] top-[-3.5px]",
		"bottom-[-3.5px] left-[-3.5px]",
		"bottom-[-3.5px] right-[-3.5px]",
	];

	return (
		<div
			className="invisible absolute left-1/2 top-[calc(clamp(150px,20vh,216px)_+_80px)] z-30 aspect-[3/4] w-[min(24vh,240px)] opacity-0 max-[1140px]:hidden"
			data-feature-source=""
		>
			<div className="absolute inset-x-0 top-[-20px] flex h-[15px] select-none items-center justify-between font-protokoll text-[10px] leading-none text-verevon-j-text/38">
				<span className="flex items-center gap-1">
					<span className="size-[5px] rounded-full bg-verevon-coral/70" />
					oppgave
				</span>
				<span>arbeidskontekst</span>
			</div>

			<div className="relative h-full w-full overflow-hidden border border-verevon-j-text/10 bg-[#f8f8f7] shadow-[0_10px_28px_rgba(23,23,23,0.035)]">
				<Image
					alt="Kundekontekst visualisert som et signal før Verevon lager forslag."
					className="object-cover opacity-70 saturate-[0.72]"
					fill
					sizes="260px"
					src="/human-haze.png"
				/>
			</div>

			{/* Decorative corner brackets, purely visual. */}
			{corners.map((position) => (
				<span
					aria-hidden="true"
					className={[
						"absolute size-[6px] border border-verevon-j-text/14 bg-background",
						position,
					].join(" ")}
					key={position}
				/>
			))}
		</div>
	);
}

// The composer overlay cycles through Verevon's three real entry points while
// the scroll scene is visible: Chat, Crawl and Søk.
function FeaturePromptComposer() {
	return (
		<div
			className="invisible absolute left-1/2 top-[calc(clamp(150px,20vh,216px)_+_min(32vh,320px)_+_80px)] z-40 w-[min(91vw,880px)] origin-top -translate-x-1/2 opacity-0 max-[1140px]:hidden"
			data-feature-prompt=""
		>
			<FeatureComposerCycle id="feature-desktop-composer" />
		</div>
	);
}

// The 4-card grid, revealed last in the pinned sequence (the "send" action's
// payoff). Width caps at 1440px on wide screens and at a height-derived
// floor (`214svh - 706px`, min 700px) on short ones, so cards never blow
// past a comfortable size in either dimension. Desktop-only.
function FeatureOutputStage() {
	return (
		<div
			className="invisible absolute left-1/2 top-[calc(clamp(112px,16vh,156px)_+_24px)] z-40 grid w-[min(90vw,1440px,max(700px,calc(214svh_-_706px)))] -translate-x-1/2 grid-cols-4 gap-x-[clamp(12px,1.25vw,22px)] gap-y-[clamp(16px,2vh,24px)] opacity-0 max-[1140px]:hidden"
			data-feature-output-stage=""
		>
			<div
				className="col-span-4 mx-auto -translate-y-2 max-w-[760px] text-center"
				data-feature-output-copy=""
			>
				<h2 className="m-0 font-arbeit text-[clamp(2rem,3vw,3.5rem)] font-light leading-[0.98] tracking-[-0.06em] text-verevon-j-text">
					Én plattform. Flere arbeidsflater.
				</h2>
				<p className="mx-auto mt-3 max-w-[620px] font-protokoll text-[clamp(0.85rem,0.9vw,1rem)] font-light leading-[1.4] text-verevon-text-muted">
					Fra kilder og kunnskap til agenter, arbeidsflater og kontroll — samlet i Verevon.
				</p>
			</div>

			<FeatureFilmGroup
				controlClassName="right-0 top-0"
				requireStageActivation
			>
				{workflowCards.map((card, index) => (
					<FeatureWorkflowCard
						card={card}
						index={index}
						key={card.title}
					/>
				))}
			</FeatureFilmGroup>
		</div>
	);
}

// Below 1141px there's no pin, no scrub, no GSAP at all — just this plain,
// normally-scrolling stack: heading, source card, looping composer, then the 4
// result cards. Renders in parallel with the desktop markup below and is
// toggled purely by the `max-[1140px]:` / default Tailwind breakpoints.
function FeatureMobileFallback() {
	return (
		<div
			className="hidden max-[1140px]:grid max-[1140px]:gap-10 max-[1140px]:pt-10"
			data-feature-fallback=""
		>
			<div className="text-center">
				<p className="mb-4 font-protokoll text-[0.7rem] font-medium uppercase leading-none tracking-[0.3em] text-verevon-j-text/38">
					Verevon-arbeidsflyten
				</p>

				<h2 className="m-0 font-arbeit text-[clamp(3rem,14vw,5rem)] font-light leading-[0.95] tracking-[-0.07em] text-verevon-j-text">
					Én plattform. Flere arbeidsflater.
				</h2>

				<p className="mx-auto mt-5 max-w-[34rem] font-protokoll text-[1rem] font-light leading-[1.48] text-verevon-text-muted">
					Verevon samler agenten, kunnskapen, systemene og kontrollen rundt arbeidet dere allerede gjør.
				</p>

				<div aria-label="Chat, Crawl og Søk koblet til kunnskap, agenter og kontroll." className="mx-auto mt-5 flex max-w-[34rem] flex-wrap items-center justify-center gap-x-2 gap-y-2 font-protokoll text-[0.62rem] font-medium uppercase tracking-[0.15em] text-verevon-j-text/48">
					<span>Chat · Crawl · Søk</span>
					<span aria-hidden="true" className="text-verevon-coral/70">→</span>
					<span>Kunnskap · agenter · kontroll</span>
				</div>
			</div>

			<div className="mx-auto grid w-full max-w-[760px] gap-8">
				<div className="relative mx-auto aspect-[3/4] w-[min(72vw,270px)] border border-verevon-j-text/10 bg-[#f8f8f7] shadow-[0_10px_28px_rgba(23,23,23,0.035)]">
					<Image
						alt="En oppgave visualisert som kontekst før Verevon lager et forslag."
						className="object-cover opacity-70 saturate-[0.72]"
						fill
						sizes="72vw"
						src="/human-haze.png"
					/>
				</div>

				<FeatureComposerCycle id="feature-fallback-composer" />
			</div>

			<div className="relative grid gap-8 sm:grid-cols-2 sm:gap-x-6">
				<div className="col-span-full text-center sm:col-span-2">
					<h2 className="m-0 font-arbeit text-[clamp(2rem,7vw,3.5rem)] font-light leading-[0.98] tracking-[-0.06em] text-verevon-j-text">
						Én plattform. Flere arbeidsflater.
					</h2>
					<p className="mx-auto mt-3 max-w-[34rem] font-protokoll text-[0.95rem] font-light leading-[1.45] text-verevon-text-muted">
						Fra kilder og kunnskap til agenter, arbeidsflater og kontroll — samlet i Verevon.
					</p>
				</div>

				<FeatureFilmGroup controlClassName="right-0 top-0">
					{workflowCards.map((card, index) => (
						<FeatureWorkflowCard
							animated={false}
							card={card}
							index={index}
							key={card.title}
						/>
					))}
				</FeatureFilmGroup>
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

		const media = gsap.matchMedia();

		media.add(
			{
				isDesktop: "(min-width: 1141px)",
				reduceMotion: "(prefers-reduced-motion: reduce)",
			},
			(mediaContext) => {
				const reduceMotion = Boolean(mediaContext.conditions?.reduceMotion);

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

		// Below 1141px, FeatureMobileFallback renders instead — bail out
		// of the whole pin/scrub setup rather than fight the mobile layout.
		// Also bails if any target is missing (e.g. markup changed elsewhere).
		if (
			!mediaContext.conditions?.isDesktop ||
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
		let refreshCancelled = false;
		document.fonts?.ready.then(() => {
			if (!refreshCancelled) {
				ScrollTrigger.refresh();
			}
		});

		const featureCopyItems = Array.from(featureCopy.children).filter(
			(child): child is HTMLElement => child instanceof HTMLElement,
		);

		// prefers-reduced-motion: skip both timelines entirely and just show
		// the finished state (cards visible, nothing pinned).
		if (reduceMotion) {
			const context = gsap.context(() => {
					gsap.set([featureIntro, featureBg, featureCopy, source, prompt], {
						autoAlpha: 0,
					});
					gsap.set(featureCopyItems, { autoAlpha: 0, y: 0 });
					gsap.set(outputStage, { autoAlpha: 1 });
				gsap.set(outputStage, { pointerEvents: "auto" });
				gsap.set(cards, {
					autoAlpha: 1,
					filter: "none",
					rotationX: 0,
					scale: 1,
					xPercent: 0,
					y: 0,
				});
					gsap.set([processing, sendRing], {
					autoAlpha: 0,
				});
			}, section);

			return () => {
				refreshCancelled = true;
				context.revert();
			};
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
				y: 0,
			});
			gsap.set(featureCopyItems, {
				autoAlpha: 0,
				y: 32,
				force3D: true,
			});

			gsap.set(source, {
				autoAlpha: 0,
				xPercent: -50,
				yPercent: 0,
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
				attr: { "data-feature-output-active": "false" },
				perspective: 1400,
				pointerEvents: "none",
				transformStyle: "preserve-3d",
			});

			gsap.set(sendButton, {
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
				rotationX: -24,
				scale: 1.04,
				transformOrigin: "50% 0%",
				transformStyle: "preserve-3d",
				xPercent: (index) => [72, 24, -24, -72][index] ?? 0,
				y: -8,
				force3D: true,
			});

			// Headline reveal ("Én plattform. Flere arbeidsflater."). Scroll-scrubbed (not a
			// fixed-duration play-once animation) so it stays in sync with scroll
			// speed instead of drifting out of sync with the pin below it.
			// Keep the pre-pin reveal short: it gives the heading enough time to
			// establish the section without creating a separate scroll chapter
			// before the interaction begins.
			const introTimeline = gsap.timeline({
				defaults: { ease: "power2.out" },
					scrollTrigger: {
					trigger: section,
					start: "top 66%",
					end: "top top",
					scrub: true,
					preventOverlaps: "feature-cards",
					invalidateOnRefresh: true,
					id: "verevon-feature-intro-fade",
				},
			});

			introTimeline
				.to(featureIntro, { autoAlpha: 1, duration: 0.52 }, 0)
				.to(featureBg, { autoAlpha: 1, duration: 0.52 }, 0)
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

			// The pin length follows the actual timeline. The typing beat gets the
			// scroll distance it needs before sending can begin; the final 0.08u
			// is a deliberate settling hold after the cards resolve.
			const typingStart = 0.44;
			const typingEnd =
				typingStart + 0.2 + Math.max(0, chars.length - 1) * 0.002;
			// Keep the handoff from completed text to confirmation tight so the
			// scene does not leave the user scrolling through an empty beat.
			const sendStart = typingEnd + 0.04;
			const cardsStart = sendStart + 0.2;
			const cardsSettledAt = cardsStart + 0.52;
			const sceneDuration = cardsSettledAt + 0.08;
			const pinScrollViewports = Math.min(
				2.1,
				Math.max(1.65, sceneDuration * 1.1),
			);

			const timeline = gsap.timeline({
				defaults: { ease: "none" },
				scrollTrigger: {
					trigger: section,
					start: "top top",
					end: () => `+=${Math.round(window.innerHeight * pinScrollViewports)}`,
					pin,
					scrub: true,
					anticipatePin: 1,
					preventOverlaps: "feature-cards",
					invalidateOnRefresh: true,
					id: "verevon-feature-cards-scroll",
					onUpdate: (trigger) => {
						// Keep the vignette clocks active at the completed pin state.
						// IntersectionObserver still pauses them once the cards leave
						// the viewport, so this does not create offscreen animation work.
						const active = trigger.progress >= cardsStart / sceneDuration;
						const nextValue = active ? "true" : "false";

						if (outputStage.dataset.featureOutputActive !== nextValue) {
							outputStage.dataset.featureOutputActive = nextValue;
						}
					},
				},
			});

			timeline
				// Beat 1 starts on the first pinned frame: the section statement
				// clears before the task settles into the same visual field.
				.to(
					featureCopy,
					{
						autoAlpha: 0,
						y: -28,
						duration: 0.16,
					},
					0,
				)
				.to(
					source,
					{
						autoAlpha: 1,
						y: 0,
						duration: 0.14,
					},
					0.12,
				)
				.to(source, { y: -18, scale: 0.965, duration: 0.14 }, 0.26)
				// Beat 2: the composer appears, then finishes typing before the send
				// beat can begin. `typingEnd` is derived from the real character count.
				.to(
					prompt,
					{
						autoAlpha: 1,
						y: 0,
						scale: 1,
						duration: 0.18,
					},
					0.3,
				)
				.to(
					chars,
					{
						autoAlpha: 1,
						duration: 0.2,
						stagger: 0.002,
					},
					typingStart,
				)
				// Beat 3: a short, discrete confirmation (no scrubbed paint effects).
				.set(sendButton, { backgroundColor: "#ee7a50" }, sendStart)
				.to(
					sendButton,
					{
						scale: 0.88,
						duration: 0.06,
					},
					sendStart,
				)
				.to(sendArrow, { y: -3, duration: 0.06 }, sendStart)
				.to(
					sendRing,
					{
						autoAlpha: 1,
						scale: 1.12,
						duration: 0.08,
					},
					sendStart + 0.01,
				)
				.to(
					processing,
					{
						autoAlpha: 1,
						scaleX: 1,
						duration: 0.14,
					},
					sendStart + 0.05,
				)
				.set(sendButton, { backgroundColor: "#111111" }, sendStart + 0.14)
				.to(
					sendButton,
					{
						scale: 1,
						duration: 0.1,
					},
					sendStart + 0.14,
				)
				.to(sendArrow, { y: 0, duration: 0.1 }, sendStart + 0.14)
				.to(
					sendRing,
					{
						autoAlpha: 0,
						scale: 1.45,
						duration: 0.12,
					},
					sendStart + 0.15,
				)
				// Beat 4: the setup falls away while the four system surfaces settle in.
				.to(
					prompt,
					{
						autoAlpha: 0.18,
						y: 76,
						scale: 0.92,
						duration: 0.08,
					},
					cardsStart,
				)
				.to(
					source,
					{
						autoAlpha: 0.42,
						scale: 0.76,
						y: 4,
						duration: 0.08,
					},
					cardsStart,
				)
				.to(outputStage, { autoAlpha: 1, duration: 0.06 }, cardsStart + 0.02)
				.set(outputStage, { pointerEvents: "auto" }, cardsStart + 0.02)
				.to(
					cards,
					{
						autoAlpha: 0.78,
						stagger: 0.05,
						duration: 0.06,
					},
					cardsStart + 0.02,
				)
				.to(
					cards,
					{
						scale: 1,
						xPercent: 0,
						y: 0,
						stagger: 0.05,
						duration: 0.26,
					},
					cardsStart + 0.04,
				)
				.to(
					cards,
					{
						rotationX: 0,
						stagger: 0.05,
						duration: 0.26,
					},
					cardsStart + 0.04,
				)
				.to(cards, { autoAlpha: 1, duration: 0.05 }, cardsStart + 0.47)
				.to(source, { autoAlpha: 0, duration: 0.08 }, cardsStart + 0.12)
				.to(prompt, { autoAlpha: 0, duration: 0.08 }, cardsStart + 0.12)
				.to(outputStage, { autoAlpha: 1, duration: 0.08 }, cardsSettledAt);
		}, section);

			return () => {
				refreshCancelled = true;
				context.revert();
			};
			},
		);

		return () => media.revert();
	}, []);

	return (
		<section
			aria-labelledby="feature-cards-title"
			className="relative isolate overflow-hidden bg-background text-verevon-j-text"
			id="feature-cards"
			ref={sectionRef}
		>
			{/* The pinned stage: exactly one viewport tall, everything inside is
			    absolutely positioned and cross-faded by the timelines above. */}
			<div
				className="relative min-h-svh overflow-hidden max-[1140px]:min-h-0"
				data-feature-pin=""
			>
				<div className="relative min-h-svh overflow-hidden max-[1140px]:grid max-[1140px]:min-h-0 max-[1140px]:gap-12 max-[1140px]:px-[var(--verevon-page-pad)] max-[1140px]:py-24">
					{/* Decorative background grid lines (desktop only). */}
					<div
						aria-hidden="true"
						className="absolute inset-0 bg-[linear-gradient(90deg,rgba(23,23,23,0.055)_1px,transparent_1px),linear-gradient(180deg,rgba(23,23,23,0.048)_1px,transparent_1px)] bg-[length:calc(100%/4)_calc(100%/3),calc(100%/4)_calc(100%/3)] max-[1140px]:bg-[length:92px_92px]"
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
						className="pointer-events-none absolute inset-x-4 bottom-0 top-0 border-x border-verevon-j-text/6 max-[1140px]:hidden"
					/>

					<FeatureSourceCard />
					<FeaturePromptComposer />
					<FeatureOutputStage />

					{/* Four decorative corner/edge dots marking the stage's thirds. */}
					<div
						aria-hidden="true"
						className="pointer-events-none absolute inset-0 max-[1140px]:hidden"
					>
						<span className="absolute left-4 top-[calc(100%/3)] size-[7px] -translate-y-1/2 rounded-full bg-verevon-j-text/18" />
						<span className="absolute bottom-[calc(100%/3)] left-4 size-[7px] translate-y-1/2 rounded-full bg-verevon-j-text/18" />
						<span className="absolute right-4 top-[calc(100%/3)] size-[7px] -translate-y-1/2 rounded-full bg-verevon-j-text/18" />
						<span className="absolute bottom-[calc(100%/3)] right-4 size-[7px] translate-y-1/2 rounded-full bg-verevon-j-text/18" />
					</div>

					<FeatureMobileFallback />
				</div>
			</div>

			{/* Real (non-decorative) heading for screen readers/SEO — the visual
			    headline above is presentational and lives inside FeatureIntro. */}
			<h2 className="sr-only" id="feature-cards-title">
				Én plattform. Flere arbeidsflater.
			</h2>
		</section>
	);
}

export default FeatureCardsSection;
