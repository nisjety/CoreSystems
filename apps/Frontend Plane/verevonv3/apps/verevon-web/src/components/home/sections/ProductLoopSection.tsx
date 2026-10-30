"use client";

import { useLayoutEffect, useRef, useState } from "react";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import type { ProductRecording } from "@/lib/product-recording-contract";
import { ProductRecordingPlayer } from "./ProductRecordingPlayer";
import type { VerevonComposerMode } from "@/components/ui/VerevonComposerPreview";
import { FeatureComposerCycle } from "./FeatureComposerCycle";
import { LiveDemoMobileCard, LoopLiveDemo } from "./ProductLoopLiveDemo";
import {
	PRODUCT_LOOP_LIVE_DEMO,
	type ProductLoopLiveDemo,
} from "./product-loop-live-demo";
import { getProductLoopComposerCopy } from "./product-loop-composer-copy";
import {
	ProductLoopMediaContent,
	type ProductLoopMedia,
} from "./ProductLoopProductDemos";
import {
	getProductLoopContentFrame,
	getProductLoopScrollYForTime,
	getProductLoopTimelineProgress,
	PRODUCT_LOOP_ENTRANCE_SCROLL_MARGIN,
	PRODUCT_LOOP_ENTRANCE_TIMELINE_DURATION,
	PRODUCT_LOOP_ENTRANCE_VIEWPORT_RATIO,
	PRODUCT_LOOP_SETTLE_EVENT,
	PRODUCT_LOOP_STAGE04_ACTIVE_END,
	PRODUCT_LOOP_STAGE04_EXIT,
	PRODUCT_LOOP_STAGE04_SETTLE_START,
} from "./product-loop-progress";

gsap.registerPlugin(ScrollTrigger);

type LoopStage = {
	body?: string;
	kind: "composer" | "media" | "live";
	live?: ProductLoopLiveDemo;
	media?: ProductLoopMedia;
	state: string;
	step: string;
	title?: string;
};

export const loopStages: LoopStage[] = [
	{
		step: "01",
		state: "Inngang",
		kind: "media",
		media: {
			kind: "image",
			src: "/verevon-product-shots/verevon-dashboard-curtain.jpg",
    alt: "Verevon-dashbord med søk i kunnskapsbasen, arbeidsflater og innganger til kunnskap og agenter, vist på en skjerm foran varme gardiner.",
			objectPosition: "center 6%",
		},
	},
	{
		step: "02",
		state: "Forstå",
		kind: "media",
		title: "Se hva som berøres.",
		body: "Når noe skjer i virksomheten, samler Verevon det som er relevant fra systemer, kunnskap og historikk. Dere ser sammenhengen før dere tar stilling.",
		media: {
			kind: "video",
			src: "/product-scenes/02-plan.mp4",
			webm: "/product-scenes/02-plan.webm",
			poster: "/product-scenes/02-plan-poster.webp",
			alt: "Et signal fra et fagsystem blir satt i sammenheng med relevant kunnskap og historikk i Verevon.",
		},
	},
	{
		step: "03",
		state: "Vurder",
		kind: "media",
		title: "Grunnlaget samlet. Valget deres.",
		body: "Når noe avviker, viser Verevon hva som har skjedd, hva som berøres og hvilke valg som finnes. Arbeidet går videre når dere har tatt stilling.",
		media: {
			kind: "video",
			src: "/product-scenes/03-undersok.mp4",
			webm: "/product-scenes/03-undersok.webm",
			poster: "/product-scenes/03-undersok-poster.webp",
			alt: "Verevon viser årsaken til et avvik, kildene bak det og alternativer for videre behandling.",
		},
	},
	{
		step: "04",
		state: "Utforsk",
		kind: "live",
		title: "Se arbeidsflaten i praksis.",
		body: "Utforsk den felles arbeidsflaten med fiktive data. Still spørsmål, se sammenhengen og følg hva som ligger bak hvert forslag.",
		live: PRODUCT_LOOP_LIVE_DEMO,
	},
	{ step: "05", state: "Composer", kind: "composer" },
];

const LIVE_LAYER_INDEX = loopStages.findIndex((stage) => stage.kind === "live");

const copyStages = loopStages.slice(1, 3);

function LoopComposerContent() {
	const [activeComposerMode, setActiveComposerMode] =
		useState<VerevonComposerMode>("chat");
	const activeComposerCopy = getProductLoopComposerCopy(activeComposerMode);

	return (
		<div className="relative h-full w-full text-verevon-j-text">
			<div className="relative flex h-full w-full flex-col items-center justify-center overflow-visible px-[clamp(16px,2vw,28px)] py-[clamp(18px,2.4vh,30px)] text-center">
				<div className="shrink-0" data-product-loop-composer-copy="">
					<h2 className="verevon-home-heading min-h-[1.8em] max-w-[14ch] text-verevon-j-text text-balance">
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
					className="mt-[clamp(22px,4vh,44px)] min-h-[3em] max-w-[42rem] shrink-0 font-manrope text-[clamp(1rem,1.05vw,1.125rem)] font-light leading-[1.5] tracking-[-0.015em] text-verevon-text-muted"
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
			className="absolute inset-0 overflow-hidden"
			data-active={index === 0}
			data-product-loop-layer={index}
			style={{
				clipPath: index === 0 ? "inset(0% 0% 0%)" : "inset(0% 0% 100%)",
				opacity: index === 0 ? 1 : 0,
                visibility: index === 0 ? "visible" : "hidden",
			}}
		>
			{stage.kind === "composer" ? (
				<LoopComposerContent />
			) : stage.kind === "live" && stage.live ? (
				<LoopLiveDemo live={stage.live} />
			) : stage.media ? (
				<ProductLoopMediaContent media={stage.media} />
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
			<h2 className="verevon-home-heading m-0 max-w-[10.5ch] pt-7 text-verevon-j-text text-balance">
				{stage.title}
			</h2>
			<p className="mt-[clamp(22px,2.4vw,36px)] max-w-[34ch] font-manrope text-[clamp(1rem,1.05vw,1.125rem)] font-light leading-[1.5] tracking-[-0.015em] text-verevon-text-muted text-pretty">
				{stage.body}
			</p>
		</div>
	);
}

const FRAME_TICKS_HIDDEN_OVER_FILM =
	"[[data-product-loop-frame]:has([data-product-loop-layer][aria-hidden=false]_video)_&]:opacity-0 [[data-product-loop-frame]:has([data-product-loop-layer][aria-hidden=false]_[data-product-loop-live])_&]:opacity-0";

function LoopFrameChrome() {
	return (
		<div
			aria-hidden="true"
			className="pointer-events-none absolute inset-0 z-20 text-verevon-j-text/14"
			data-product-loop-frame-chrome=""
		>
			<div className={`absolute inset-0 transition-opacity duration-300 ease-out ${FRAME_TICKS_HIDDEN_OVER_FILM}`} data-product-loop-frame-ticks="">
				<span className="absolute left-4 top-4 size-[6px] border border-current bg-background/60" />
				<span className="absolute right-4 top-4 size-[6px] border border-current bg-background/60" />
				<span className="absolute bottom-4 left-4 size-[6px] border border-current bg-background/60" />
				<span className="absolute bottom-4 right-4 size-[6px] border border-current bg-background/60" />
			</div>
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
					Felles grunnlag. Styrt handling.
				</h2>
				<div className="relative aspect-[1.12] overflow-hidden rounded-[22px] bg-[#f4f3f0]">
					<ProductLoopMediaContent
						media={loopStages[0].media as ProductLoopMedia}
					/>
				</div>
			</div>

			{loopStages.slice(1, 4).map((stage) => (
				<article className="grid gap-5 border-t border-verevon-j-text/10 pt-6" key={stage.step}>
					<h3 className="m-0 max-w-[11ch] font-manrope text-[clamp(1.875rem,7vw,2.75rem)] font-light leading-[0.96] tracking-[-0.045em] text-verevon-j-text text-balance">
						{stage.title}
					</h3>
					<p className="max-w-[34rem] font-manrope text-[1rem] font-light leading-[1.5] tracking-[-0.015em] text-verevon-text-muted">
						{stage.body}
					</p>
					{stage.kind === "live" && stage.live ? (
						<LiveDemoMobileCard live={stage.live} />
					) : stage.media ? (
						<div className="relative aspect-[16/10] overflow-hidden rounded-[20px] bg-[#f4f3f0]">
							<ProductLoopMediaContent media={stage.media} />
						</div>
					) : null}
				</article>
			))}

			<div className="grid gap-6 border-t border-verevon-j-text/10 pt-6">
				<h3 className="max-w-[14ch] font-manrope text-[clamp(1.875rem,7vw,2.75rem)] font-light leading-[0.96] tracking-[-0.045em] text-verevon-j-text">
					{activeComposerCopy.title}
				</h3>
				<FeatureComposerCycle
					id="product-mobile-composer-control"
					onModeChange={setActiveComposerMode}
				/>
				<p className="max-w-[34rem] font-manrope text-[1rem] font-light leading-[1.5] tracking-[-0.015em] text-verevon-text-muted">
					{activeComposerCopy.body}
				</p>
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
				"(min-width: 900px)",
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
					const monitorImage = layers[0]?.querySelector<HTMLImageElement>("img");
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

					let viewportRect = viewport.getBoundingClientRect();
                    const markerBoxes = new Map<HTMLElement, {height: number; left: number; top: number; width: number}>();
                    const measureMarker = (marker: HTMLElement) => {
						const markerRect = marker.getBoundingClientRect();

						return {
							height: markerRect.height,
							left: markerRect.left - viewportRect.left,
							top: markerRect.top - viewportRect.top,
							width: markerRect.width,
						};
					};

                    const measureGeometry = () => {
                        viewportRect = viewport.getBoundingClientRect();
                        [...frameMarkers, ...copyMarkers].forEach(marker => markerBoxes.set(marker, measureMarker(marker)));
                    };
                    measureGeometry();
                    const boxFromMarker = (marker: HTMLElement) => markerBoxes.get(marker)!;

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
						const markerRect = placeFrame(0);
						const width = Math.min(markerRect.width, markerRect.height * 2);

						return {
							...markerRect,
							left: (viewportRect.width - width) / 2,
							width,
						};
					};
					const dynamicEntranceStartFrame = () => ({
						height: () => entranceStartFrame().height,
						left: () => entranceStartFrame().left,
						top: () => entranceStartFrame().top,
						width: () => entranceStartFrame().width,
					});
					const contentFrame = () => getProductLoopContentFrame(viewportRect);
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
						height: () => viewportRect.height * sizeRatio,
						width: () => viewportRect.height * sizeRatio,
						x: () => viewportRect.width * xRatio,
						y: () => viewportRect.height * yRatio,
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
							...dynamicCircleGeometry(0.28, 0, 0.1),
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
						if (monitorImage) {
							gsap.set(monitorImage, { scale: 1, transformOrigin: "50% 24%" });
						}
						gsap.set(panels, {
							autoAlpha: 0,
							y: 16,
							pointerEvents: "none",
						});
						gsap.set(composerCopy, { autoAlpha: 1, y: 0 });

						let previousActive = -1;
						const liveLayer = layers[LIVE_LAYER_INDEX] ?? null;
						let previousSettled: boolean | null = null;
						const loopTimeline = gsap.timeline({
							defaults: { ease: "none" },
							paused: true,
                            onUpdate() {
                                const time = this.time();
                                const settled = time >= PRODUCT_LOOP_STAGE04_SETTLE_START && time < PRODUCT_LOOP_STAGE04_EXIT;
                                if (settled !== previousSettled) {
                                    previousSettled = settled;
                                    if (liveLayer) liveLayer.dataset.productLoopSettled = String(settled);
                                }
                                const active = time < 1.16 ? 0 : time < 2.06 ? 1 : time < 2.96 ? 2 : time < PRODUCT_LOOP_STAGE04_ACTIVE_END ? 3 : 4;
                                if (active === previousActive) return;
                                previousActive = active;
                                layers.forEach((layer, index) => {
                                    layer.inert = index !== active;
                                    layer.setAttribute("aria-hidden", String(index !== active));
						layer.dataset.active = String(index === active);
                                });
                            },
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
									{ ...(frameIndex === 4 ? dynamicFullFrame() : dynamicFramePosition(frameIndex)), duration: 0.52 },
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
								...dynamicCircleGeometry(0.58, 0.08, -0.02),
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
								{ autoAlpha: 0, y: -18, duration: 0.22, ease: "power2.in" },
								0.28,
							)
							.to(chrome, { autoAlpha: 1, duration: 0.22 }, 0.52);

						// Push into the photograph's actual screen before the HTML demo
						// takes over. The image moves inside the existing frame, so the
						// approved section geometry and opening crop remain unchanged.
						if (monitorImage && (!window.matchMedia("(prefers-reduced-motion: reduce)").matches || document.documentElement.dataset.motion === "full")) {
							loopTimeline.to(monitorImage, { scale: 1.42, duration: 0.72, ease: "power2.inOut" }, 0.5);
						}

						transition(0, 1, 1, 0, 0.92);
						loopTimeline.to(
							circle,
							{
								...dynamicCircleGeometry(0.68, -0.08, 0.05),
								duration: 0.52,
								rotation: 45,
							},
							0.92,
						);
						transition(1, 2, 2, 1, 1.82);
						loopTimeline.to(
							circle,
							{
								...dynamicCircleGeometry(0.78, 0.1, -0.04),
								duration: 0.52,
								rotation: 90,
							},
							1.82,
						);

						loopTimeline.to(
							circle,
							{
								...dynamicCircleGeometry(0.86, 0, 0),
								duration: 0.58,
								rotation: 135,
							},
							2.72,
						);

                        transition(2, 3, 4, null, 2.72);
                        transition(3, 4, 3, null, PRODUCT_LOOP_STAGE04_EXIT);
                        loopTimeline
                            .to(chrome, { autoAlpha: 0, duration: 0.18 }, PRODUCT_LOOP_STAGE04_EXIT + 0.16)
							.to(circle, { ...dynamicCircleGeometry(0.86, 0, 0), rotation: 180, duration: 0.6 }, PRODUCT_LOOP_STAGE04_EXIT)
                            .to({}, { duration: 0.22 }, PRODUCT_LOOP_STAGE04_EXIT + 0.6);

					}, section);

					let disposed = false;
					let cachedTop = sectionTop();
					let cachedRange = sectionScrollRange();
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
							pinnedScrollRange: cachedRange,
							scrollY: window.scrollY,
							sectionTop: cachedTop,
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

					const settleStage = (event: Event) => {
						if (!timeline) return;
						const reducedMotion = Boolean((event as CustomEvent<{ reducedMotion?: boolean }>).detail?.reducedMotion);
						window.scrollTo({
							behavior: reducedMotion ? "auto" : "smooth",
							top: getProductLoopScrollYForTime({
								entranceScrollDistance: window.innerHeight * PRODUCT_LOOP_ENTRANCE_VIEWPORT_RATIO,
								entranceTimelineDuration: PRODUCT_LOOP_ENTRANCE_TIMELINE_DURATION,
								pinnedScrollRange: cachedRange,
								sectionTop: cachedTop,
								time: (PRODUCT_LOOP_STAGE04_SETTLE_START + PRODUCT_LOOP_STAGE04_EXIT) / 2,
								timelineDuration: timeline.duration(),
							}),
						});
					};
					window.addEventListener(PRODUCT_LOOP_SETTLE_EVENT, settleStage);

					const queueRefresh = () => {
                            if (disposed) return;
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
									ScrollTrigger.refresh();
									updateProgress(true);
								}, 80);
							});
						};

						const scrollDriver = ScrollTrigger.create({
							trigger: section,
							start: () => `top ${window.innerHeight * PRODUCT_LOOP_ENTRANCE_VIEWPORT_RATIO}px`,
                            end: "bottom bottom",
                            onRefreshInit: () => {
                                progressTween?.kill();
                                // Rewind before invalidating so new tween starts never capture a later card.
                                timeline?.progress(0);
                                measureGeometry();
                                cachedTop = sectionTop();
                                cachedRange = sectionScrollRange();
                                timeline?.invalidate();
                            },
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

						queueRefresh();
						document.fonts?.ready.then(queueRefresh);
						window.addEventListener("load", queueRefresh, { once: true });

					window.addEventListener("resize", queueRefresh);
					updateProgress(true);

					return () => {
						disposed = true;
						if (refreshFrame !== null) {
							window.cancelAnimationFrame(refreshFrame);
						}
						if (refreshTimeout !== null) {
							window.clearTimeout(refreshTimeout);
						}

						progressTween?.kill();
						scrollDriver.kill();

						window.removeEventListener("resize", queueRefresh);
						window.removeEventListener(PRODUCT_LOOP_SETTLE_EVENT, settleStage);
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
			<div className="mx-auto mb-10 max-w-6xl"><h2 id="product-recordings-title" className="verevon-home-heading max-w-[14ch]">Felles grunnlag. Styrt handling.</h2></div>
			<ProductRecordingPlayer recordings={recordings} />
		</section>;
	}

	// The first 0.78 timeline units now play before the sticky boundary. Reducing
	// the pinned height by the same proportion preserves the approved pacing of
	// every downstream card, composer, and dashboard transition.
	return (
		<section
			aria-labelledby="product-loop-title"
			className="relative isolate z-[5] -mt-[clamp(48px,6vh,88px)] h-[436svh] overflow-visible bg-background text-verevon-j-text max-[899px]:mt-0 max-[899px]:h-auto"
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

			<div className="sticky top-0 min-h-svh overflow-visible max-[899px]:relative max-[899px]:min-h-0" data-product-loop-pin="">
				<div
					className="relative min-h-svh overflow-visible max-[899px]:grid max-[899px]:min-h-0 max-[899px]:gap-12 max-[899px]:px-[var(--verevon-page-pad)] max-[899px]:py-24"
					data-product-loop-viewport=""
				>
					<div aria-hidden="true" className="absolute -inset-y-[14vh] inset-x-0 bg-[radial-gradient(circle_at_78%_28%,rgba(238,122,80,0.08),transparent_27%),radial-gradient(circle_at_14%_74%,rgba(41,64,74,0.07),transparent_28%),linear-gradient(180deg,rgba(248,248,247,0),rgba(248,248,247,0.78))]" />

					<div
						className="absolute left-[var(--verevon-edge)] top-[clamp(112px,15vh,176px)] z-40 max-w-[min(46.8vw,630px)] text-left max-[899px]:hidden"
						data-product-loop-header=""
					>
						<h2 className="verevon-home-heading max-w-[10ch] text-verevon-j-text text-balance">
							Felles grunnlag. Styrt handling.
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
						<div className="absolute left-[22%] top-[29%] h-[clamp(260px,34vh,390px)] w-[calc((100%-2*var(--verevon-edge))*0.48)]" data-product-loop-frame-marker="" />
						<div className="absolute right-[var(--verevon-edge)] top-[clamp(142px,17vh,206px)] aspect-[16/10] w-[calc((100%-2*var(--verevon-edge))*0.53)]" data-product-loop-frame-marker="" />
						<div className="absolute left-[var(--verevon-edge)] top-[clamp(222px,34vh,350px)] aspect-[16/10] w-[clamp(520px,43vw,760px)]" data-product-loop-frame-marker="" />
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
						className="absolute left-0 top-0 z-20 h-full w-full overflow-hidden max-[899px]:hidden"
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
				Fra felles grunnlag til styrt handling
			</h2>
		</section>
	);
}

export default ProductLoopSection;
