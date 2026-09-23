"use client";

import { useLayoutEffect, useRef, useState } from "react";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import type { ProductRecording } from "@/lib/product-recording-contract";
import { ProductRecordingPlayer } from "./ProductRecordingPlayer";
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
			src: "/verevon-mood/shared-surface-amber.jpg",
			alt: "To personer holder den samme opplyste flaten mellom seg.",
			objectPosition: "center 50%",
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
			src: "/verevon-mood/operator-calm-warm.jpg",
			alt: "En person arbeider rolig gjennom en oppgave i et lyst, dempet rom.",
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
			kind: "video",
			src: "/verevon-product-shots/product-showcase.mp4",
			poster: "/verevon-product-shots/product-showcase-poster.jpg",
			alt: "Verevons faktiske norske composer med et eksempelspørsmål om beslutninger, kilder og neste steg.",
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
		<div className="relative h-full w-full text-verevon-j-text">
			<div className="relative flex h-full w-full flex-col items-center justify-center overflow-visible px-[clamp(16px,2vw,28px)] py-[clamp(18px,2.4vh,30px)] text-center">
				<div className="shrink-0" data-product-loop-composer-copy="">
					<p
						className="verevon-eyebrow text-verevon-coral"
						data-product-loop-composer-label=""
					>
						{activeComposerCopy.label}
					</p>
					<h2 className="verevon-home-heading mt-4 min-h-[1.8em] max-w-[14ch] text-verevon-j-text text-balance">
						<span
							className="block animate-[fade-in_300ms_ease-out]"
							data-product-loop-composer-title=""
							key={activeComposerMode}
						>
							{activeComposerCopy.title}
						</span>
					</h2>
				</div>
				<div className="mt-[clamp(24px,4vh,48px)] w-full max-w-[656px] shrink-0">
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
						className="block animate-[fade-in_300ms_ease-out]"
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
			className="absolute left-0 top-0 max-w-[512px] text-left"
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
			<h2 className="verevon-home-heading m-0 max-w-[10.5ch] text-verevon-j-text text-balance">
				{stage.title}
			</h2>
			<p className="mt-[clamp(22px,2.4vw,36px)] max-w-[416px] font-protokoll text-[clamp(1rem,1.02vw,1.18rem)] font-light leading-[1.5] text-verevon-text-muted text-pretty">
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
			className="hidden max-[899px]:grid max-[899px]:gap-12"
			data-product-loop-mobile-fallback=""
		>
			<div className="grid gap-6 border-t border-verevon-j-text/10 pt-6">
				<h2 className="verevon-home-heading max-w-[11ch] text-verevon-j-text">
					Fra kunnskap til handling
				</h2>
				<div className="relative aspect-[1.12] overflow-hidden rounded-[22px] bg-[#f4f3f0]">
					<ProductLoopMediaContent
						media={loopStages[0].media as ProductLoopMedia}
						priority
					/>
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

export function ProductLoopSection({ recordings = [] }: { recordings?: ProductRecording[] }) {
	const sectionRef = useRef<HTMLElement>(null);

	useLayoutEffect(() => {
		const section = sectionRef.current;

		if (!section || recordings.length > 0) {
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
					const circle = section.querySelector<HTMLElement>(
						"[data-product-loop-circle]",
					);
					const connector = section.querySelector<HTMLElement>(
						"[data-product-loop-connector]",
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
					const composerCopy = Array.from(
						section.querySelectorAll<HTMLElement>(
							"[data-product-loop-composer-copy]",
						),
					);
					if (
						!viewport ||
						!frame ||
						!circle ||
						!connector ||
						!chrome ||
						!header ||
						!copyFloat ||
						frameMarkers.length !== 4 ||
						copyMarkers.length !== copyStages.length ||
						layers.length !== loopStages.length ||
						panels.length !== copyStages.length ||
						composerCopy.length !== 2
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
					const entranceStartFrame = () => {
						const viewportRect = viewport.getBoundingClientRect();
						const markerRect = placeFrame(0);

						return {
							...markerRect,
							left: (viewportRect.width - markerRect.width) / 2,
						};
					};
					const dynamicEntranceStartFrame = () => ({
						height: () => entranceStartFrame().height,
						left: () => entranceStartFrame().left,
						top: () => entranceStartFrame().top,
						width: () => entranceStartFrame().width,
					});
					const contentFrame = () => {
						const rect = viewport.getBoundingClientRect();
						const inset = Math.max(Math.min(rect.width * 0.077, 100), (rect.width - 1512) / 2);
						const width = rect.width - inset * 2;
						const height = Math.min(rect.height - 160, width * 0.625);
						return { width, height, left: inset, top: Math.max(96, (rect.height - height) / 2) };
					};
					const dynamicFullFrame = () => ({
						height: () => contentFrame().height,
						left: () => contentFrame().left,
						top: () => contentFrame().top,
						width: () => contentFrame().width,
					});
					const dynamicCopyPosition = (index: number) => ({
						height: () => placeCopy(index).height,
						left: () => placeCopy(index).left,
						top: () => placeCopy(index).top,
						width: () => placeCopy(index).width,
					});
					const entranceFrame = contentFrame;
					const dynamicEntranceFrame = () => ({
						height: () => entranceFrame().height,
						left: () => entranceFrame().left,
						top: () => entranceFrame().top,
						width: () => entranceFrame().width,
					});
					// A single ring that grows and rotates behind the frame as the
					// timeline moves through each stage — the section's own "loop"
					// motif (lightweight.info-style morphing circle), independent of
					// SignalPathLayer. Sized as a ratio of the viewport's own height
					// (kept square) and offset as a ratio of viewport width/height, so
					// it stays responsive the same way dynamicFramePosition does.
					const dynamicCircleGeometry = (
						sizeRatio: number,
						xRatio: number,
						yRatio: number,
					) => ({
						height: () => viewport.getBoundingClientRect().height * sizeRatio,
						width: () => viewport.getBoundingClientRect().height * sizeRatio,
						x: () => viewport.getBoundingClientRect().width * xRatio,
						y: () => viewport.getBoundingClientRect().height * yRatio,
					});

					let timeline: gsap.core.Timeline | null = null;

					const animationContext = gsap.context(() => {
						gsap.set(frame, {
							...entranceStartFrame(),
							autoAlpha: 1,
							bottom: "auto",
							right: "auto",
							x: 0,
							y: 0,
						});
						gsap.set(header, { autoAlpha: 1, y: 0 });
						gsap.set(chrome, { autoAlpha: 0 });
						gsap.set(circle, {
							autoAlpha: 0.5,
							force3D: true,
							rotation: -20,
							xPercent: -50,
							yPercent: -50,
							...dynamicCircleGeometry(0.3, 0, 0.15),
						});
						gsap.set(connector, { autoAlpha: 1 });
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
						gsap.set(composerCopy, { autoAlpha: 1, y: 0 });

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
									// Clear the copy before it crosses the moving image.
									.to(copyFloat, { autoAlpha: 0, pointerEvents: "none", duration: 0.1 }, at - 0.1)
									.to(
										copyFloat,
										{
											...dynamicCopyPosition(copyIndex),
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
									)
									.to(copyFloat, { autoAlpha: 1, pointerEvents: "auto", duration: 0.16 }, at + 0.52);
							}
						};

						// The entrance progress is driven before the sticky boundary, so
						// this expansion is already underway as the section enters view.
						loopTimeline
							.fromTo(
							frame,
							{
								...dynamicEntranceStartFrame(),
									bottom: "auto",
									right: "auto",
									x: 0,
									y: 0,
								},
								{
									...dynamicEntranceFrame(),
									duration: PRODUCT_LOOP_ENTRANCE_TIMELINE_DURATION,
									ease: "none",
									immediateRender: true,
								},
								0,
							)
							.to(
								circle,
								{
									...dynamicCircleGeometry(0.62, 0.14, -0.08),
									duration: PRODUCT_LOOP_ENTRANCE_TIMELINE_DURATION,
									ease: "power3.out",
									rotation: 0,
								},
								0,
							)
							.to(
								connector,
								{ autoAlpha: 0, duration: 0.3, ease: "power1.in" },
								0,
							)
							.to(
								header,
								{ autoAlpha: 0, y: -18, duration: 0.28, ease: "power2.in" },
								0.5,
							)
							.to(chrome, { autoAlpha: 1, duration: 0.22 }, 0.52);

						transition(0, 1, 1, 0, 0.92);
						loopTimeline.to(
							circle,
							{
								...dynamicCircleGeometry(0.78, -0.16, 0.1),
								duration: 0.52,
								rotation: 45,
							},
							0.92,
						);
						transition(1, 2, 2, 1, 1.82);
						loopTimeline.to(
							circle,
							{
								...dynamicCircleGeometry(0.9, 0.18, -0.12),
								duration: 0.52,
								rotation: 90,
							},
							1.82,
						);

						loopTimeline.to(
							circle,
							{
								...dynamicCircleGeometry(1.05, 0, 0.05),
								duration: 0.58,
								rotation: 135,
							},
							2.72,
						);

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
							// Let the composer stand on the page itself, with its copy
							// and controls above the loop ring until the platform crossfade.
							.to(
								circle,
								{
									...dynamicCircleGeometry(1.3, 0, 0),
									autoAlpha: 0,
									duration: 0.6,
									rotation: 180,
								},
								3.72,
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
	}, [recordings.length]);

	if (recordings.length > 0) {
		return <section id="produkt" ref={sectionRef} aria-labelledby="product-recordings-title" className="bg-background px-[var(--verevon-page-pad)] py-24 text-verevon-j-text">
			<div className="mx-auto mb-10 max-w-6xl"><h2 id="product-recordings-title" className="verevon-home-heading max-w-[14ch]">Fra kunnskap til handling.</h2></div>
			<ProductRecordingPlayer recordings={recordings} />
		</section>;
	}

	// The first 0.78 timeline units now play before the sticky boundary. Reducing
	// the pinned height by the same proportion preserves the approved pacing of
	// every downstream card, composer, and dashboard transition.
	return (
		<section
			aria-labelledby="product-loop-title"
			className="relative isolate z-[5] -mt-[clamp(48px,6vh,88px)] h-[396svh] overflow-visible bg-background text-verevon-j-text max-[899px]:mt-0 max-[899px]:h-auto max-[899px]:overflow-hidden"
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

			<div className="sticky top-0 min-h-svh overflow-hidden max-[899px]:relative max-[899px]:min-h-0" data-product-loop-pin="">
				<div
					className="relative min-h-svh overflow-hidden max-[899px]:grid max-[899px]:min-h-0 max-[899px]:gap-12 max-[899px]:px-[var(--verevon-page-pad)] max-[899px]:py-24"
					data-product-loop-viewport=""
				>
					<div aria-hidden="true" className="absolute inset-0 bg-[radial-gradient(circle_at_78%_28%,rgba(238,122,80,0.08),transparent_27%),radial-gradient(circle_at_14%_74%,rgba(41,64,74,0.07),transparent_28%),linear-gradient(180deg,rgba(248,248,247,0),rgba(248,248,247,0.78))]" />

					<div
						className="absolute left-[var(--verevon-edge)] top-[clamp(112px,15vh,176px)] z-40 max-w-[min(46.8vw,630px)] text-left max-[899px]:hidden"
						data-product-loop-header=""
					>
						<h2 className="verevon-home-heading max-w-[10ch] text-verevon-j-text text-balance">
							Fra kunnskap til handling
						</h2>
					</div>

					{/* Continues the centered handoff from ProblemSection — fades out
					    as the entrance plays, right as the loop circle takes over. */}
						<div
							aria-hidden="true"
							className="pointer-events-none absolute left-1/2 top-0 z-10 h-[clamp(48px,9vh,140px)] w-px -translate-x-1/2 bg-[linear-gradient(180deg,transparent,color-mix(in_srgb,var(--verevon-j-text)_28%,transparent)_60%,transparent)] max-[899px]:hidden"
							data-product-loop-connector=""
						/>

					<div aria-hidden="true" className="absolute inset-0 max-[899px]:hidden">
						<div className="absolute left-[22%] top-[24%] h-[clamp(260px,34vh,390px)] w-[calc((100%-2*var(--verevon-edge))*0.48)]" data-product-loop-frame-marker="" />
						<div className="absolute right-[var(--verevon-edge)] top-[clamp(142px,17vh,206px)] h-[clamp(344px,47vh,560px)] w-[calc((100%-2*var(--verevon-edge))*0.53)]" data-product-loop-frame-marker="" />
						<div className="absolute left-[var(--verevon-edge)] top-[clamp(222px,34vh,350px)] h-[clamp(260px,34vh,390px)] w-[clamp(520px,43vw,760px)]" data-product-loop-frame-marker="" />
						<div
							className="absolute left-1/2 top-1/2 h-[clamp(560px,64vh,680px)] w-[min(calc(100%-2*var(--verevon-edge)),1040px)] -translate-x-1/2 -translate-y-1/2"
							data-product-loop-frame-marker=""
						/>

						<div className="absolute left-[var(--verevon-edge)] top-[clamp(214px,27vh,310px)] h-[360px] w-[calc((100%-2*var(--verevon-edge))*0.41)]" data-product-loop-copy-marker="" />
						<div className="absolute right-[var(--verevon-edge)] top-[clamp(236px,34vh,374px)] h-[320px] w-[calc((100%-2*var(--verevon-edge))*0.43)]" data-product-loop-copy-marker="" />
					</div>

					{/* Loop motif: one ring behind the frame, growing/rotating through
					    each stage (see the circle tweens in the timeline above). */}
					<div
						aria-hidden="true"
						className="pointer-events-none absolute left-1/2 top-1/2 z-10 aspect-square rounded-full border-2 border-verevon-j-text/25 max-[899px]:hidden"
						data-product-loop-circle=""
					/>

					<div
						className="absolute left-0 top-0 z-20 h-full w-full overflow-hidden rounded-[24px] border border-verevon-j-text/8 bg-[#f8f8f7] shadow-[0_28px_96px_rgba(23,23,23,0.1)] max-[899px]:hidden"
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

					<div className="absolute z-30 max-[899px]:hidden" data-product-loop-copy="">
						{copyStages.map((stage, index) => (
							<LoopCopyPanel index={index} key={stage.step} stage={stage} />
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
