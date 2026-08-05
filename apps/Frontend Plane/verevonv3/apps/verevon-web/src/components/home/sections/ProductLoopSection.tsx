"use client";

import Image from "next/image";
import { useLayoutEffect, useRef, useState } from "react";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import type { VerevonComposerMode } from "@/components/ui/VerevonComposerPreview";
import { FeatureComposerCycle } from "./FeatureComposerCycle";
import { getProductLoopComposerCopy } from "./product-loop-composer-copy";
import {
	ProductLoopMediaContent,
	type ProductLoopMedia,
} from "./ProductLoopProductDemos";
import {
	getProductLoopTimelineProgress,
	PRODUCT_LOOP_ENTRANCE_SCROLL_MARGIN,
	PRODUCT_LOOP_ENTRANCE_TIMELINE_DURATION,
	PRODUCT_LOOP_ENTRANCE_VIEWPORT_RATIO,
} from "./product-loop-progress";

gsap.registerPlugin(ScrollTrigger);

type LoopStage = {
	body?: string;
	kind: "composer" | "media";
	media?: ProductLoopMedia;
	state: string;
	step: string;
	title?: string;
};

const loopStages: LoopStage[] = [
	{
		step: "01",
		state: "Inngang",
		kind: "media",
		media: {
			kind: "image",
			src: "/human-haze.png",
			alt: "Mennesker i et lyst, abstrakt arbeidsrom — kunnskap og mennesker i samme flyt.",
			objectPosition: "center 48%",
		},
	},
	{
		step: "02",
		state: "Arbeid",
		title: "Verevon gjør spørsmålet om til arbeidet.",
		body: "En oppgave blir til en arbeidsflyt med riktig kontekst, foreslåtte steg og et grunnlag teamet kan følge.",
		kind: "media",
		media: {
			kind: "image",
			src: "/man-talking-and-delegating.jpg",
			alt: "En person som forklarer et arbeid i et varmt, lyst arbeidsrom.",
			objectPosition: "center 42%",
		},
	},
	{
		step: "03",
		state: "Kontroll",
		title: "Risikable handlinger stopper hos dere.",
		body: "Se kilde, policy, konsekvens og neste steg i samme flyt. Dere godkjenner før noe sendes, endres eller publiseres.",
		kind: "media",
		media: {
			kind: "image",
			src: "/hand-with-strings-of-text.jpg",
			alt: "Hender som arbeider med tekst og informasjon i et lyst rom.",
			objectPosition: "center 48%",
		},
	},
	{
		step: "04",
		state: "Composer",
		kind: "composer",
	},
	{
		step: "05",
		state: "Plattform",
		kind: "media",
		media: {
			kind: "image",
			src: "/verevon-product-shots/dashboard-live-overview.png",
			alt: "Verevon-oversikten med søk, arbeidsflater og innganger til teamets daglige arbeid.",
			objectPosition: "center 42%",
		},
	},
];

const copyStages = loopStages.slice(1, 3);

function LoopComposerContent() {
	const [activeComposerMode, setActiveComposerMode] =
		useState<VerevonComposerMode>("chat");
	const activeComposerCopy = getProductLoopComposerCopy(activeComposerMode);

	return (
		<div className="relative h-full w-full overflow-visible bg-transparent text-verevon-j-text">
			<div
				className="absolute inset-0 opacity-70"
				data-product-loop-composer-background=""
			>
				<Image
					alt=""
					className="object-cover"
					fill
					sizes="90vw"
					src="/soft-orb.png"
				/>
			</div>
			<div
				className="absolute inset-0 bg-[radial-gradient(circle_at_50%_42%,rgba(255,255,255,0.82),transparent_44%),linear-gradient(180deg,rgba(250,249,246,0.16),rgba(250,249,246,0.72))]"
				data-product-loop-composer-overlay=""
			/>
			<div className="relative flex h-full w-full flex-col items-center justify-center overflow-visible px-[clamp(16px,2vw,28px)] py-[clamp(18px,2.4vh,30px)] text-center">
				<div className="shrink-0" data-product-loop-composer-copy="">
					<p
						className="verevon-eyebrow text-verevon-coral"
						data-product-loop-composer-label=""
					>
						{activeComposerCopy.label}
					</p>
					<h2 className="mt-4 min-h-[1.8em] max-w-[14ch] font-arbeit text-[clamp(2.8rem,5vw,6.2rem)] font-light leading-[0.9] tracking-[-0.075em] text-verevon-j-text text-balance">
						<span
							className="block motion-safe:animate-[fade-in_300ms_ease-out]"
							data-product-loop-composer-title=""
							key={activeComposerMode}
						>
							{activeComposerCopy.title}
						</span>
					</h2>
				</div>
				<div className="mt-[clamp(24px,4vh,48px)] w-full max-w-[820px] shrink-0">
					<FeatureComposerCycle
						id="product-loop-composer"
						onModeChange={setActiveComposerMode}
					/>
				</div>
				<p
					className="mt-[clamp(22px,4vh,44px)] min-h-[3em] max-w-[42rem] shrink-0 font-protokoll text-[clamp(0.96rem,1.04vw,1.16rem)] font-light leading-[1.48] text-verevon-text-muted"
					data-product-loop-composer-copy=""
				>
					<span
						className="block motion-safe:animate-[fade-in_300ms_ease-out]"
						data-product-loop-composer-body=""
						key={activeComposerMode}
					>
						{activeComposerCopy.body}
					</span>
				</p>
			</div>
		</div>
	);
}

function LoopMediaLayer({
	index,
	stage,
}: {
	index: number;
	stage: LoopStage;
}) {
	return (
		<div
			aria-hidden="true"
			className="absolute inset-0 overflow-hidden"
			data-product-loop-layer={index}
			style={{
				clipPath: index === 0 ? "inset(0% 0% 0%)" : "inset(0% 0% 100%)",
				opacity: index === 0 ? 1 : 0,
			}}
		>
			{stage.kind === "composer" ? (
				<LoopComposerContent />
			) : stage.media ? (
				<ProductLoopMediaContent media={stage.media} priority={index === 0} />
			) : null}
			{stage.kind === "media" ? (
				<div className="absolute inset-0 bg-[linear-gradient(180deg,rgba(248,248,247,0.02),rgba(248,248,247,0.16)),radial-gradient(circle_at_74%_22%,rgba(238,122,80,0.08),transparent_26%)]" />
			) : null}
		</div>
	);
}

function LoopCopyPanel({ index, stage }: { index: number; stage: LoopStage }) {
	return (
		<div
			className="absolute left-0 top-0 max-w-[640px] text-left"
			data-product-loop-copy-panel={index}
			style={{
				opacity: index === 0 ? 1 : 0,
				pointerEvents: index === 0 ? "auto" : "none",
				visibility: index === 0 ? "visible" : "hidden",
			}}
		>
			<p className="verevon-eyebrow mb-5 text-[color-mix(in_srgb,var(--verevon-a-earth)_78%,var(--verevon-j-text))]">
				{stage.step} / {stage.state}
			</p>
			<h2 className="m-0 max-w-[10.5ch] font-arbeit text-[clamp(3rem,5.4vw,7.4rem)] font-light leading-[0.9] tracking-[-0.07em] text-verevon-j-text text-balance">
				{stage.title}
			</h2>
			<p className="mt-[clamp(22px,2.4vw,36px)] max-w-[520px] font-protokoll text-[clamp(1rem,1.02vw,1.18rem)] font-light leading-[1.5] text-verevon-text-muted text-pretty">
				{stage.body}
			</p>
		</div>
	);
}

function LoopFrameChrome() {
	return (
		<div
			aria-hidden="true"
			className="pointer-events-none absolute inset-0 z-20 text-verevon-j-text/14"
			data-product-loop-frame-chrome=""
		>
			<span className="absolute inset-x-0 top-0 h-px bg-current" />
			<span className="absolute inset-x-0 bottom-0 h-px bg-current" />
			<span className="absolute inset-y-0 left-0 w-px bg-current" />
			<span className="absolute inset-y-0 right-0 w-px bg-current" />
			<span className="absolute left-4 top-4 size-[6px] border border-current bg-background/60" />
			<span className="absolute right-4 top-4 size-[6px] border border-current bg-background/60" />
			<span className="absolute bottom-4 left-4 size-[6px] border border-current bg-background/60" />
			<span className="absolute bottom-4 right-4 size-[6px] border border-current bg-background/60" />
		</div>
	);
}

function ProductLoopMobileFallback() {
	const [activeComposerMode, setActiveComposerMode] =
		useState<VerevonComposerMode>("chat");
	const activeComposerCopy = getProductLoopComposerCopy(activeComposerMode);

	return (
		<div
			className="hidden motion-reduce:grid motion-reduce:gap-12 max-[899px]:grid max-[899px]:gap-12"
			data-product-loop-mobile-fallback=""
		>
			<div className="grid gap-6 border-t border-verevon-j-text/10 pt-6">
				<p className="verevon-eyebrow text-verevon-coral">03 / Produkt</p>
				<h2 className="max-w-[11ch] font-arbeit text-[clamp(2.8rem,13vw,5rem)] font-light leading-[0.9] tracking-[-0.07em] text-verevon-j-text">
					Fra kunnskap til handling
				</h2>
				<div className="relative aspect-[1.12] overflow-hidden rounded-[22px] bg-[#f4f3f0]">
					<ProductLoopMediaContent media={loopStages[0].media as ProductLoopMedia} />
				</div>
			</div>

			{copyStages.map((stage) => (
				<article className="grid gap-5 border-t border-verevon-j-text/10 pt-6" key={stage.step}>
					<p className="verevon-eyebrow text-verevon-coral">
						{stage.step} / {stage.state}
					</p>
					<h3 className="m-0 max-w-[11ch] font-arbeit text-[clamp(2.5rem,12vw,4.6rem)] font-light leading-[0.94] tracking-[-0.065em] text-verevon-j-text text-balance">
						{stage.title}
					</h3>
					<p className="max-w-[34rem] font-protokoll text-[1rem] font-light leading-[1.48] text-verevon-text-muted">
						{stage.body}
					</p>
					<div className="relative aspect-[1.18] overflow-hidden rounded-[20px] bg-[#f4f3f0]">
						<ProductLoopMediaContent media={stage.media as ProductLoopMedia} />
					</div>
					{stage.step === "02" ? <FeatureComposerCycle id="product-mobile-composer" /> : null}
				</article>
			))}

			<div className="grid gap-6 border-t border-verevon-j-text/10 pt-6">
				<p className="verevon-eyebrow text-verevon-coral">
					{activeComposerCopy.label}
				</p>
				<h3 className="max-w-[14ch] font-arbeit text-[clamp(2.5rem,11vw,4.4rem)] font-light leading-[0.92] tracking-[-0.065em] text-verevon-j-text">
					{activeComposerCopy.title}
				</h3>
				<FeatureComposerCycle
					id="product-mobile-composer-control"
					onModeChange={setActiveComposerMode}
				/>
				<p className="max-w-[34rem] font-protokoll text-[1rem] font-light leading-[1.48] text-verevon-text-muted">
					{activeComposerCopy.body}
				</p>
			</div>

			<div className="grid gap-6 border-t border-verevon-j-text/10 pt-6">
				<p className="verevon-eyebrow text-verevon-coral">05 / Plattform</p>
				<p className="max-w-[20ch] font-arbeit text-[clamp(2.4rem,11vw,4.4rem)] font-light leading-[0.92] tracking-[-0.065em] text-verevon-j-text">
					Én oversikt over arbeidet som skjer.
				</p>
				<div className="relative aspect-[1.18] overflow-hidden rounded-[20px] bg-[#f8f8f7]">
					<ProductLoopMediaContent media={loopStages[4].media as ProductLoopMedia} />
				</div>
			</div>
		</div>
	);
}

export function ProductLoopSection() {
	const sectionRef = useRef<HTMLElement>(null);

	useLayoutEffect(() => {
		const section = sectionRef.current;

		if (!section) {
			return;
		}

		const matchMedia = gsap.matchMedia();
		const context = gsap.context(() => {
			matchMedia.add(
				"(min-width: 900px) and (prefers-reduced-motion: no-preference)",
				() => {
					const viewport = section.querySelector<HTMLElement>(
						"[data-product-loop-viewport]",
					);
					const frame = section.querySelector<HTMLElement>(
						"[data-product-loop-frame]",
					);
					const chrome = section.querySelector<HTMLElement>(
						"[data-product-loop-frame-chrome]",
					);
					const header = section.querySelector<HTMLElement>(
						"[data-product-loop-header]",
					);
					const copyFloat = section.querySelector<HTMLElement>(
						"[data-product-loop-copy]",
					);
					const frameMarkers = Array.from(
						section.querySelectorAll<HTMLElement>(
							"[data-product-loop-frame-marker]",
						),
					);
					const copyMarkers = Array.from(
						section.querySelectorAll<HTMLElement>(
							"[data-product-loop-copy-marker]",
						),
					);
					const layers = Array.from(
						section.querySelectorAll<HTMLElement>(
							"[data-product-loop-layer]",
						),
					);
					const panels = Array.from(
						section.querySelectorAll<HTMLElement>(
							"[data-product-loop-copy-panel]",
						),
					);
					const stateCards = Array.from(
						section.querySelectorAll<HTMLElement>(
							"[data-product-loop-state]",
						),
					);
					const stateAccents = Array.from(
						section.querySelectorAll<HTMLElement>(
							"[data-product-loop-state-accent]",
						),
					);
					const composerCopy = Array.from(
						section.querySelectorAll<HTMLElement>(
							"[data-product-loop-composer-copy]",
						),
					);
					const composerBackground = section.querySelector<HTMLElement>(
						"[data-product-loop-composer-background]",
					);
					const composerOverlay = section.querySelector<HTMLElement>(
						"[data-product-loop-composer-overlay]",
					);

					if (
						!viewport ||
						!frame ||
						!chrome ||
						!header ||
						!copyFloat ||
						frameMarkers.length !== 4 ||
						copyMarkers.length !== copyStages.length ||
						layers.length !== loopStages.length ||
						panels.length !== copyStages.length ||
						stateCards.length !== loopStages.length ||
						stateAccents.length !== loopStages.length ||
						composerCopy.length !== 2 ||
						!composerBackground ||
						!composerOverlay
					) {
						return;
					}

					const boxFromMarker = (marker: HTMLElement) => {
						const viewportRect = viewport.getBoundingClientRect();
						const markerRect = marker.getBoundingClientRect();

						return {
							height: markerRect.height,
							left: markerRect.left - viewportRect.left,
							top: markerRect.top - viewportRect.top,
							width: markerRect.width,
						};
					};

					const sectionTop = () =>
						section.getBoundingClientRect().top + window.scrollY;
					const sectionScrollRange = () =>
						Math.max(
							window.innerHeight,
							section.offsetHeight - window.innerHeight,
						);
					const placeFrame = (index: number) =>
						boxFromMarker(frameMarkers[index]);
					const placeCopy = (index: number) => boxFromMarker(copyMarkers[index]);
					const dynamicFramePosition = (index: number) => ({
						height: () => placeFrame(index).height,
						left: () => placeFrame(index).left,
						top: () => placeFrame(index).top,
						width: () => placeFrame(index).width,
					});
					const dynamicFullFrame = () => ({
						height: () => viewport.getBoundingClientRect().height,
						left: () => 0,
						top: () => 0,
						width: () => viewport.getBoundingClientRect().width,
					});
					const dynamicCopyPosition = (index: number) => ({
						height: () => placeCopy(index).height,
						left: () => placeCopy(index).left,
						top: () => placeCopy(index).top,
						width: () => placeCopy(index).width,
					});
					const entranceFrame = () => {
						const viewportRect = viewport.getBoundingClientRect();

						return {
							height: viewportRect.height * 0.9,
							left: viewportRect.width * 0.05,
							top: viewportRect.height * 0.05,
							width: viewportRect.width * 0.9,
						};
					};
					const dynamicEntranceFrame = () => ({
						height: () => entranceFrame().height,
						left: () => entranceFrame().left,
						top: () => entranceFrame().top,
						width: () => entranceFrame().width,
					});

					let timeline: gsap.core.Timeline | null = null;

					const animationContext = gsap.context(() => {
						gsap.set(frame, {
							...placeFrame(0),
							autoAlpha: 1,
							bottom: "auto",
							right: "auto",
							x: 0,
							y: 0,
						});
						gsap.set(header, { autoAlpha: 1, y: 0 });
						gsap.set(chrome, { autoAlpha: 0 });
						gsap.set(copyFloat, {
							...placeCopy(0),
							autoAlpha: 0,
							pointerEvents: "none",
							y: 28,
						});
						gsap.set(layers, {
							autoAlpha: 0,
							clipPath: "inset(0% 0% 100%)",
							force3D: true,
						});
						gsap.set(layers[0], {
							autoAlpha: 1,
							clipPath: "inset(0% 0% 0%)",
						});
						gsap.set(panels, {
							autoAlpha: 0,
							y: 16,
							pointerEvents: "none",
						});
						gsap.set(stateCards, {
							autoAlpha: (index) => (index === 0 ? 1 : 0.42),
							y: (index) => (index === 0 ? 0 : 8),
							force3D: true,
						});
						gsap.set(stateAccents, {
							scaleX: (index) => (index === 0 ? 1 : 0.12),
							transformOrigin: "0% 50%",
						});
					gsap.set(composerCopy, { autoAlpha: 1, y: 0 });
					gsap.set([composerBackground, composerOverlay], { autoAlpha: 1 });

						const loopTimeline = gsap.timeline({
							defaults: { ease: "none" },
							paused: true,
						});
						timeline = loopTimeline;

						const transition = (
							fromLayer: number,
							toLayer: number,
							frameIndex: number,
							copyIndex: number | null,
							toStage: number,
							at: number,
						) => {
							loopTimeline
								.to(
									frame,
									{ ...dynamicFramePosition(frameIndex), duration: 0.52 },
									at,
								)
								.to(
									layers[fromLayer],
									{
										autoAlpha: 0,
										clipPath: "inset(100% 0% 0%)",
										duration: 0.3,
									},
									at + 0.1,
								)
								.to(
									layers[toLayer],
									{
										autoAlpha: 1,
										clipPath: "inset(0% 0% 0%)",
										duration: 0.38,
									},
									at + 0.16,
									)
								.to(
									stateCards,
									{
										autoAlpha: (index) =>
											index === toStage ? 1 : 0.42,
										y: (index) => (index === toStage ? 0 : 8),
										duration: 0.28,
									},
									at + 0.18,
									)
								.to(
									stateAccents,
									{
										scaleX: (index) =>
											index === toStage ? 1 : 0.12,
										duration: 0.28,
									},
									at + 0.18,
									);

							if (copyIndex === null) {
								loopTimeline.to(
									copyFloat,
									{
										autoAlpha: 0,
										pointerEvents: "none",
										y: -24,
										duration: 0.24,
									},
									at + 0.06,
								);
							} else {
								const nextPanel = panels[copyIndex];
								loopTimeline
									.to(
										copyFloat,
										{
											...dynamicCopyPosition(copyIndex),
											autoAlpha: 1,
											pointerEvents: "auto",
											y: 0,
											duration: 0.52,
										},
										at,
									)
									.to(
										panels,
										{
											autoAlpha: (index) =>
												index === copyIndex ? 1 : 0,
											y: (index) => (index === copyIndex ? 0 : -14),
											pointerEvents: (index) =>
												index === copyIndex ? "auto" : "none",
											duration: 0.28,
										},
										at + 0.28,
									)
									.to(
										nextPanel,
										{ y: 0, duration: 0.28 },
										at + 0.28,
									);
							}
						};

						// The entrance progress is driven before the sticky boundary, so
						// this expansion is already underway as the section enters view.
						loopTimeline
							.fromTo(
								frame,
								{
									...dynamicFramePosition(0),
									bottom: "auto",
									right: "auto",
									x: 0,
									y: 0,
								},
								{
									...dynamicEntranceFrame(),
									duration: PRODUCT_LOOP_ENTRANCE_TIMELINE_DURATION,
									ease: "power3.out",
									immediateRender: true,
								},
								0,
							)
							.to(
								header,
								{ autoAlpha: 0, y: -18, duration: 0.28, ease: "power2.in" },
								0.5,
							)
							.to(chrome, { autoAlpha: 1, duration: 0.22 }, 0.52)
							.to(
								stateCards,
								{
									autoAlpha: (index) => (index === 0 ? 1 : 0.42),
									duration: 0.22,
								},
								0.56,
							)
							.to(
								stateAccents,
								{
									scaleX: (index) => (index === 0 ? 1 : 0.12),
									duration: 0.22,
								},
								0.56,
							);

						transition(0, 1, 1, 0, 1, 0.92);
						transition(1, 2, 2, 1, 2, 1.82);

						loopTimeline
							.to(
								frame,
								{ ...dynamicFramePosition(3), duration: 0.58 },
								2.72,
							)
							.to(
								layers[2],
								{
									autoAlpha: 0,
									clipPath: "inset(100% 0% 0%)",
									duration: 0.3,
								},
								2.84,
							)
							.to(
								layers[3],
								{
									autoAlpha: 1,
									clipPath: "inset(0% 0% 0%)",
									duration: 0.42,
								},
								2.9,
							)
							.to(
								copyFloat,
								{
									autoAlpha: 0,
									pointerEvents: "none",
									y: -24,
									duration: 0.24,
								},
								2.78,
							)
							.to(chrome, { autoAlpha: 0, duration: 0.18 }, 2.88)
							.to(
								stateCards,
								{
									autoAlpha: (index) => (index === 3 ? 1 : 0.42),
									y: (index) => (index === 3 ? 0 : 8),
									duration: 0.24,
								},
								2.94,
							)
							.to(
								stateAccents,
								{
									scaleX: (index) => (index === 3 ? 1 : 0.12),
									duration: 0.24,
								},
									2.94,
							)
							.to(
								frame,
								{
									backgroundColor: "transparent",
									borderColor: "transparent",
									borderRadius: "0px",
									boxShadow: "none",
									duration: 0.34,
								},
								2.78,
							)
							.to(
								[composerBackground, composerOverlay],
								{ autoAlpha: 0, duration: 0.3 },
								2.82,
							)
							.to(
								composerCopy,
								{ autoAlpha: 0, y: -14, duration: 0.36, ease: "power2.in" },
								3.62,
							)
							.to(
								frame,
								{
									...dynamicFullFrame(),
									backgroundColor: "#f8f8f7",
									borderColor: "rgba(31,31,29,0.08)",
									borderRadius: "24px",
									boxShadow: "0 28px 96px rgba(23,23,23,0.1)",
									duration: 0.82,
									ease: "power3.out",
								},
								3.72,
							)
							.to(
								layers[3],
								{
									autoAlpha: 0,
									clipPath: "inset(100% 0% 0%)",
									duration: 0.32,
								},
								3.86,
							)
							.to(
								layers[4],
								{
									autoAlpha: 1,
									clipPath: "inset(0% 0% 0%)",
									duration: 0.46,
								},
								3.9,
							)
							.to(
								stateCards,
								{
									autoAlpha: (index) => (index === 4 ? 1 : 0.42),
									y: (index) => (index === 4 ? 0 : 8),
									duration: 0.28,
								},
								3.94,
							)
							.to(
								stateAccents,
								{
									scaleX: (index) => (index === 4 ? 1 : 0.12),
									duration: 0.28,
								},
								3.94,
							);
					}, section);

					let progressFrame: number | null = null;
					let progressTween: gsap.core.Tween | null = null;
					let refreshFrame: number | null = null;
					let refreshTimeout: number | null = null;

					const updateProgress = (immediate = false) => {
						if (!timeline) {
							return;
						}

						const progress = getProductLoopTimelineProgress({
							entranceScrollDistance:
								window.innerHeight * PRODUCT_LOOP_ENTRANCE_VIEWPORT_RATIO,
							entranceTimelineDuration:
								PRODUCT_LOOP_ENTRANCE_TIMELINE_DURATION,
							pinnedScrollRange: sectionScrollRange(),
							scrollY: window.scrollY,
							sectionTop: sectionTop(),
							timelineDuration: timeline.duration(),
						});

						if (immediate) {
							progressTween?.kill();
							progressTween = null;
							timeline.progress(progress);
							return;
						}

						progressTween = gsap.to(timeline, {
							duration: 0.46,
							ease: "power3.out",
							overwrite: true,
							progress,
						});
					};

					const queueProgress = () => {
						if (progressFrame !== null) {
							return;
						}

						progressFrame = window.requestAnimationFrame(() => {
							progressFrame = null;
							updateProgress();
						});
					};

						const queueRefresh = () => {
							if (refreshFrame !== null) {
								return;
							}

							refreshFrame = window.requestAnimationFrame(() => {
								refreshFrame = null;
								if (refreshTimeout !== null) {
									window.clearTimeout(refreshTimeout);
								}
								refreshTimeout = window.setTimeout(() => {
									refreshTimeout = null;
									timeline?.invalidate();
									ScrollTrigger.refresh();
									updateProgress(true);
								}, 80);
							});
						};

						const scrollDriver = ScrollTrigger.create({
							trigger: section,
							start: "top top",
							end: () => `+=${sectionScrollRange()}`,
							onRefresh: () => updateProgress(true),
							onUpdate: () => updateProgress(),
						});

						const refreshObserver =
							typeof ResizeObserver === "undefined"
								? null
								: new ResizeObserver(queueRefresh);
						const refreshTargets = [
							section,
							section.previousElementSibling,
							section.parentElement,
							document.body,
						].filter(
							(target): target is HTMLElement => target instanceof HTMLElement,
						);
						refreshTargets.forEach((target) => refreshObserver?.observe(target));

						const settleRefreshTimers = [0, 250, 750, 1500].map((delay) =>
							window.setTimeout(queueRefresh, delay),
						);
						document.fonts?.ready.then(queueRefresh);
						window.addEventListener("load", queueRefresh, { once: true });
					window.addEventListener("scroll", queueProgress, { passive: true });
					window.addEventListener("resize", queueRefresh);
					updateProgress(true);

					return () => {
						if (progressFrame !== null) {
							window.cancelAnimationFrame(progressFrame);
						}
						if (refreshFrame !== null) {
							window.cancelAnimationFrame(refreshFrame);
						}
						if (refreshTimeout !== null) {
							window.clearTimeout(refreshTimeout);
						}
						settleRefreshTimers.forEach((timer) => window.clearTimeout(timer));
						progressTween?.kill();
						scrollDriver.kill();
						window.removeEventListener("scroll", queueProgress);
						window.removeEventListener("resize", queueRefresh);
						window.removeEventListener("load", queueRefresh);
						refreshObserver?.disconnect();
						animationContext.revert();
					};
				},
			);
		}, section);

		return () => {
			matchMedia.revert();
			context.revert();
		};
	}, []);

	// The first 0.78 timeline units now play before the sticky boundary. Reducing
	// the pinned height by the same proportion preserves the approved pacing of
	// every downstream card, composer, and dashboard transition.
	return (
		<section
			aria-labelledby="product-loop-title"
			className="relative isolate z-[5] -mt-[clamp(48px,6vh,88px)] h-[396svh] overflow-visible border-t border-verevon-j-text/8 bg-background text-verevon-j-text motion-reduce:mt-0 motion-reduce:h-auto motion-reduce:overflow-hidden max-[899px]:mt-0 max-[899px]:h-auto max-[899px]:overflow-hidden"
			data-product-loop
			id="flyt"
			ref={sectionRef}
		>
			<span
				aria-hidden="true"
				className="absolute left-0 top-0 h-px w-px"
				id="produkt"
				style={{ scrollMarginTop: PRODUCT_LOOP_ENTRANCE_SCROLL_MARGIN }}
			/>

			<div className="sticky top-0 min-h-svh overflow-hidden motion-reduce:relative motion-reduce:min-h-0 max-[899px]:relative max-[899px]:min-h-0" data-product-loop-pin="">
				<div
					className="relative min-h-svh overflow-hidden motion-reduce:grid motion-reduce:min-h-0 motion-reduce:gap-12 motion-reduce:px-[var(--verevon-page-pad)] motion-reduce:py-24 max-[899px]:grid max-[899px]:min-h-0 max-[899px]:gap-12 max-[899px]:px-[var(--verevon-page-pad)] max-[899px]:py-24"
					data-product-loop-viewport=""
				>
					<div aria-hidden="true" className="absolute inset-0 bg-[linear-gradient(90deg,rgba(23,23,23,0.045)_1px,transparent_1px),linear-gradient(180deg,rgba(23,23,23,0.035)_1px,transparent_1px)] bg-[length:calc(100%/4)_calc(100%/3),calc(100%/4)_calc(100%/3)] max-[899px]:bg-[length:92px_92px]" />
					<div aria-hidden="true" className="absolute inset-0 bg-[radial-gradient(circle_at_78%_28%,rgba(238,122,80,0.08),transparent_27%),radial-gradient(circle_at_14%_74%,rgba(41,64,74,0.07),transparent_28%),linear-gradient(180deg,rgba(248,248,247,0),rgba(248,248,247,0.78))]" />

					<div
						className="absolute left-[clamp(28px,7vw,140px)] top-[clamp(112px,15vh,176px)] z-40 max-w-[min(52vw,700px)] text-left motion-reduce:hidden max-[899px]:hidden"
						data-product-loop-header=""
					>
						<p className="verevon-eyebrow text-verevon-coral">03 / Produkt</p>
						<h2 className="mt-4 max-w-[10ch] font-arbeit text-[clamp(2.8rem,5.4vw,7rem)] font-light leading-[0.88] tracking-[-0.075em] text-verevon-j-text text-balance">
							Fra kunnskap til handling
						</h2>
					</div>

					<div aria-hidden="true" className="pointer-events-none absolute inset-x-4 top-0 bottom-0 border-x border-verevon-j-text/6 max-[899px]:hidden" />

					<div aria-hidden="true" className="absolute inset-0 motion-reduce:hidden max-[899px]:hidden">
						<div className="absolute left-[22%] top-[24%] h-[clamp(260px,34vh,390px)] w-[clamp(520px,43vw,760px)]" data-product-loop-frame-marker="" />
						<div className="absolute right-[clamp(56px,6.6vw,128px)] top-[clamp(142px,17vh,206px)] h-[clamp(344px,47vh,560px)] w-[clamp(610px,58vw,1120px)]" data-product-loop-frame-marker="" />
						<div className="absolute left-[clamp(92px,9vw,176px)] top-[clamp(222px,34vh,350px)] h-[clamp(260px,34vh,390px)] w-[clamp(520px,43vw,760px)]" data-product-loop-frame-marker="" />
						<div
							className="absolute left-1/2 top-1/2 h-[clamp(560px,64vh,680px)] w-[min(92vw,1040px)] -translate-x-1/2 -translate-y-1/2"
							data-product-loop-frame-marker=""
						/>

						<div className="absolute left-[clamp(72px,7vw,142px)] top-[clamp(214px,27vh,310px)] h-[360px] w-[min(38vw,620px)]" data-product-loop-copy-marker="" />
						<div className="absolute right-[clamp(86px,8vw,168px)] top-[clamp(236px,34vh,374px)] h-[320px] w-[min(42vw,680px)]" data-product-loop-copy-marker="" />
					</div>

					<div
						className="absolute left-0 top-0 z-20 h-full w-full overflow-hidden rounded-[24px] border border-verevon-j-text/8 bg-[#f8f8f7] shadow-[0_28px_96px_rgba(23,23,23,0.1)] motion-reduce:hidden max-[899px]:hidden"
						data-product-loop-frame=""
					>
						<div className="relative h-full w-full">
							{loopStages.map((stage, index) => (
								<LoopMediaLayer
									index={index}
									key={`${stage.step}-${stage.state}`}
									stage={stage}
								/>
							))}
						</div>
						<LoopFrameChrome />
					</div>

					<div className="absolute z-30 motion-reduce:hidden max-[899px]:hidden" data-product-loop-copy="">
						{copyStages.map((stage, index) => (
							<LoopCopyPanel index={index} key={stage.step} stage={stage} />
						))}
					</div>

					<div
						aria-label="Verevon arbeidssløyfe, fra inngang til plattform"
						className="absolute bottom-[clamp(42px,6vh,70px)] left-1/2 z-30 grid w-[min(86vw,1040px)] -translate-x-1/2 grid-cols-5 gap-[clamp(8px,0.8vw,12px)] motion-reduce:hidden max-[899px]:hidden"
					>
						{loopStages.map((stage, index) => (
							<div
								className="relative overflow-hidden border border-verevon-j-text/10 bg-white/52 px-4 py-3 backdrop-blur-[14px]"
								data-product-loop-state=""
								key={stage.step}
								style={{ opacity: index === 0 ? 1 : 0.42 }}
							>
								<span aria-hidden="true" className="absolute inset-x-0 bottom-0 h-[3px] origin-left scale-x-[0.12] bg-verevon-coral/70" data-product-loop-state-accent="" style={{ transform: index === 0 ? "scaleX(1)" : "scaleX(0.12)" }} />
								<span className="block font-arbeit text-[0.7rem] font-normal uppercase leading-none tracking-[0.14em] text-verevon-j-text/42">{stage.step}</span>
								<span className="mt-2 block font-protokoll text-[clamp(0.82rem,0.88vw,1rem)] font-light leading-none text-verevon-j-text/76">{stage.state}</span>
							</div>
						))}
					</div>

					<ProductLoopMobileFallback />
				</div>
			</div>

			<h2 className="sr-only" id="product-loop-title">
				Produktflyten fra kunnskap til godkjent handling
			</h2>
		</section>
	);
}

export default ProductLoopSection;
