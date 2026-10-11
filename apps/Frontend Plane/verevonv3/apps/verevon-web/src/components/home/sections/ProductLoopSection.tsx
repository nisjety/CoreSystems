"use client";

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import dynamic from "next/dynamic";
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

const ProductDemo = dynamic(() => import("../product-demo/product-demo").then((module) => module.ProductDemo), { ssr: false });

// Only mount the demo layout that can actually be seen. Keep its state while scrolling.
function ResponsiveDemo({ step, desktop = false }: { step: 1 | 2 | 3; desktop?: boolean }) {
	const root = useRef<HTMLDivElement>(null);
	const [enabled, setEnabled] = useState(false);
	useEffect(() => {
		const host = root.current?.closest<HTMLElement>(
			desktop ? "[data-product-loop-frame]" : "[data-product-loop-mobile-fallback]",
		);
		// Mount only the layout visible at this viewport width.
		let near = typeof IntersectionObserver === "undefined";
		const update = () => setEnabled(
			Boolean(host && getComputedStyle(host).display !== "none" && (near || !desktop)),
		);
		const observer = typeof IntersectionObserver === "undefined" ? null : new IntersectionObserver(([entry]) => {
			if (entry.isIntersecting) { near = true; update(); observer?.disconnect(); }
		}, { rootMargin: "1000px" });
		const section = root.current?.closest("[data-product-loop]");
		if (section) observer?.observe(section);
		const resizeObserver = host && typeof ResizeObserver !== "undefined"
			? new ResizeObserver(update)
			: null;
		if (host) resizeObserver?.observe(host);
		update();
		window.addEventListener("resize", update);
		return () => {
			observer?.disconnect();
			resizeObserver?.disconnect();
			window.removeEventListener("resize", update);
		};
	}, [desktop]);
	return <div ref={root} className="h-full w-full">{enabled && <ProductDemo embeddedStep={step} />}</div>;
}

type LoopStage = {
	body?: string;
	kind: "composer" | "media" | "demo";
	demoStep?: 1 | 2 | 3;
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
			src: "/verevon-mood/shared-surface-amber.jpg",
			alt: "To personer holder den samme opplyste flaten mellom seg.",
			objectPosition: "center 50%",
		},
	},
	{ step: "02", state: "Plan", kind: "media",
		title: "Fra spørsmål til plan. Med kildene synlige.",
		body: "Verevon leser avtaler, ordre og e-post, viser hvor hvert funn kommer fra og lager et planutkast du godkjenner.",
		media: {
			kind: "video",
			src: "/product-scenes/02-plan.mp4",
			webm: "/product-scenes/02-plan.webm",
			poster: "/product-scenes/02-plan-poster.webp",
			alt: "Verevon svarer på status for Nordlys-leveransen. Hvert funn har sin kilde, og et planutkast venter på godkjenning.",
		} },
	{ step: "03", state: "Undersøk", kind: "media",
		title: "Finn årsaken. Se beviset. Godkjenn fiksen.",
		body: "Verevon undersøker parallelt, viser hva den fant i loggen og skjemaet, og foreslår en endring du kan teste før du sier ja.",
		media: {
			kind: "video",
			src: "/product-scenes/03-undersok.mp4",
			webm: "/product-scenes/03-undersok.webm",
			poster: "/product-scenes/03-undersok-poster.webp",
			alt: "Verevon undersøker eksporten av ordre 4821 med tre parallelle oppgaver. Loggen viser at customer_id mangler, og endringen kjøres som simulert test.",
		} },
    { step: "04", state: "Følg opp", kind: "demo", demoStep: 3,
        title: "Hold arbeidet i gang.",
        body: "Se status, gjennomgå kundeutkastet og velg hvordan arbeidet skal følges opp." },
    { step: "05", state: "Composer", kind: "composer" },
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
						{activeComposerCopy.label.replace("04 /", "05 /")}
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
					className="mt-[clamp(22px,4vh,44px)] min-h-[3em] max-w-[42rem] shrink-0 font-geist text-[clamp(0.96rem,1.04vw,1.16rem)] font-light leading-[1.48] text-verevon-text-muted"
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
			data-product-loop-layer={index}
			style={{
				clipPath: index === 0 ? "inset(0% 0% 0%)" : "inset(0% 0% 100%)",
				opacity: index === 0 ? 1 : 0,
                visibility: index === 0 ? "visible" : "hidden",
			}}
		>
			{stage.kind === "composer" ? (
				<LoopComposerContent />
			) : stage.kind === "demo" ? (
				<LoopDemoPreview step={stage.demoStep!} />
            ) : stage.media ? (
				<ProductLoopMediaContent media={stage.media} priority={index === 0} />
			) : null}
			{stage.kind === "media" && stage.media?.kind !== "video" ? (
				<div className="absolute inset-0 bg-[linear-gradient(180deg,rgba(248,248,247,0.02),rgba(248,248,247,0.16)),radial-gradient(circle_at_74%_22%,rgba(238,122,80,0.08),transparent_26%)]" />
			) : null}
		</div>
	);
}

function LoopDemoPreview({ step }: { step: 1 | 2 | 3 }) {
	const previewRef = useRef<HTMLDivElement>(null);
	const contentRef = useRef<HTMLDivElement>(null);
	useLayoutEffect(() => {
		const preview = previewRef.current;
		const content = contentRef.current;
		if (!preview || !content) return;
		const resize = (width: number, height: number) => {
			const scale = Math.min(1, width / 1100);
			if (!scale) return;
			const nextWidth = `${width / scale}px`;
			const nextHeight = `${height / scale}px`;
			if (content.style.width !== nextWidth) content.style.width = nextWidth;
			if (content.style.height !== nextHeight) content.style.height = nextHeight;
			content.style.transform = `scale(${scale})`;
		};
		const observer = new ResizeObserver(([entry]) => resize(entry.contentRect.width, entry.contentRect.height));
		observer.observe(preview);
		resize(preview.clientWidth, preview.clientHeight);
		return () => observer.disconnect();
	}, []);
	return <div ref={previewRef} className="h-full w-full overflow-hidden">
		<div ref={contentRef} className="h-full w-full origin-top-left">
			<ResponsiveDemo step={step} desktop />
		</div>
	</div>;
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
			<p className="mt-[clamp(22px,2.4vw,36px)] max-w-[416px] font-geist text-[clamp(1rem,1.02vw,1.18rem)] font-light leading-[1.5] text-verevon-text-muted text-pretty">
				{stage.body}
			</p>
		</div>
	);
}

// The corner ticks step aside while a film stage is the active layer: the films show real UI edge to
// edge, and a tick would sit on top of it (at 16px inset it lands on the thread title in 02).
const FRAME_TICKS_HIDDEN_OVER_FILM =
	"[[data-product-loop-frame]:has([data-product-loop-layer][aria-hidden=false]_video)_&]:opacity-0";

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
					Fra kunnskap til handling
				</h2>
				<div className="relative aspect-[1.12] overflow-hidden rounded-[22px] bg-[#f4f3f0]">
					<ProductLoopMediaContent
						media={loopStages[0].media as ProductLoopMedia}
						priority
					/>
				</div>
			</div>

			{loopStages.slice(1, 4).map((stage) => (
				<article className="grid gap-5 border-t border-verevon-j-text/10 pt-6" key={stage.step}>
					<p className="verevon-eyebrow text-verevon-coral">
						{stage.step} / {stage.state}
					</p>
					<h3 className="m-0 max-w-[11ch] font-manrope text-[clamp(2.5rem,12vw,4.6rem)] font-light leading-[0.94] tracking-[-0.065em] text-verevon-j-text text-balance">
						{stage.title}
					</h3>
					<p className="max-w-[34rem] font-geist text-[1rem] font-light leading-[1.48] text-verevon-text-muted">
						{stage.body}
					</p>
                    {stage.media ? (
                        <div className="relative aspect-[16/10] overflow-hidden rounded-[20px] bg-[#f8f8f7]">
                            <ProductLoopMediaContent media={stage.media} />
                        </div>
                    ) : (
                        <div className="relative overflow-hidden rounded-[20px]">
                            <ResponsiveDemo step={stage.demoStep!} />
                        </div>
                    )}
				</article>
			))}

			<div className="grid gap-6 border-t border-verevon-j-text/10 pt-6">
				<p className="verevon-eyebrow text-verevon-coral">
					{activeComposerCopy.label.replace("04 /", "05 /")}
				</p>
				<h3 className="max-w-[14ch] font-manrope text-[clamp(2.5rem,11vw,4.4rem)] font-light leading-[0.92] tracking-[-0.065em] text-verevon-j-text">
					{activeComposerCopy.title}
				</h3>
				<FeatureComposerCycle
					id="product-mobile-composer-control"
					onModeChange={setActiveComposerMode}
				/>
				<p className="max-w-[34rem] font-geist text-[1rem] font-light leading-[1.48] text-verevon-text-muted">
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
						const rect = viewportRect;
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

						let previousActive = -1;
						const loopTimeline = gsap.timeline({
							defaults: { ease: "none" },
							paused: true,
                            onUpdate() {
                                const time = this.time();
                                const active = time < 1.16 ? 0 : time < 2.06 ? 1 : time < 2.96 ? 2 : time < 3.96 ? 3 : 4;
                                if (active === previousActive) return;
                                previousActive = active;
                                layers.forEach((layer, index) => {
                                    layer.inert = index !== active;
                                    layer.setAttribute("aria-hidden", String(index !== active));
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

                        transition(2, 3, 4, null, 2.72);
                        transition(3, 4, 3, null, 3.72);
                        loopTimeline
                            .to(chrome, { autoAlpha: 0, duration: 0.18 }, 3.88)
                            .to(circle, { ...dynamicCircleGeometry(1.05, 0, 0.05), rotation: 180, duration: 0.6 }, 3.72)
                            .to(frame, { backgroundColor: "transparent", borderColor: "transparent", borderRadius: "0px", boxShadow: "none", duration: 0.6 }, 3.72)
                            .to({}, { duration: 0.22 }, 4.32);

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
