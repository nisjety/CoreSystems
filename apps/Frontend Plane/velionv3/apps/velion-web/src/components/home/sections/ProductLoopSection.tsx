"use client";

import { useLayoutEffect, useRef } from "react";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { ArrowButton } from "@/components/ui/ArrowButton";
import {
	ProductLoopMediaContent,
	type ProductLoopMedia,
} from "./ProductLoopProductDemos";

gsap.registerPlugin(ScrollTrigger);

type LoopStage = ProductLoopMedia & {
	body: string;
	expands?: boolean;
	state: string;
	step: string;
	title: string;
};

const loopIntro: ProductLoopMedia = {
	kind: "image",
	src: "/velion-product-shots/dashboard-live-overview.png",
	alt: "Velion-oversikten med arbeidsflater, søk og innganger til teamets daglige arbeid.",
	objectPosition: "center 42%",
};

const loopStages: LoopStage[] = [
	{
		step: "01",
		state: "Spør",
		title: "Start der arbeidet allerede begynner.",
		body: "Skriv hva du trenger i Velion. Instruksen blir satt inn i virksomhetens kontekst før noe arbeid starter.",
		kind: "image",
		src: "/velion-product-shots/dashboard-live-prompt.png",
		alt: "Velion-oversikten med en norsk instruksjon klar i arbeidsfeltet.",
		objectPosition: "center 38%",
	},
	{
		step: "02",
		state: "Utkast",
		title: "Velion gjør spørsmålet om til arbeid.",
		body: "Utkastet bygges fra samtalen, retningslinjene og kildene deres. Teamet ser både svaret og grunnlaget bak det.",
		kind: "image",
		src: "/velion-product-shots/chat-draft-sources-focus.jpg",
		alt: "Velion-chat med svarutkast og synlige arbeidsdetaljer.",
		objectPosition: "center 42%",
	},
	{
		step: "03",
		state: "Godkjenn",
		title: "Risikable handlinger stopper hos dere.",
		body: "Se beløp, policy, kilde og konsekvens i samme visning. Godkjenn, revider eller gjør handlingen manuelt.",
		kind: "approval",
		alt: "Velion-innboksen med kundesamtale, kildebasert utkast og en refusjon som krever menneskelig godkjenning.",
	},
	{
		step: "04",
		state: "Revider",
		title: "Korriger én gang. Forbedre neste runde.",
		body: "Oppdater kilden eller regelen bak svaret. Endringen blir sporbar, kan rulles tilbake og brukes i neste arbeidsflyt.",
		expands: true,
		kind: "knowledge",
		alt: "Velions kunnskapsflate med en revidert leveringspolicy, versjonshistorikk og synkroniserte kilder.",
	},
];

function LoopMediaLayer({
	index,
	media,
}: {
	index: number;
	media: ProductLoopMedia;
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
			<ProductLoopMediaContent media={media} priority={index === 0} />
			{media.kind === "image" || media.kind === "video" ? (
				<div className="absolute inset-0 bg-[linear-gradient(180deg,rgba(248,248,247,0.02),rgba(248,248,247,0.2)),radial-gradient(circle_at_74%_22%,rgba(238,122,80,0.08),transparent_26%)]" />
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
			<p className="velion-eyebrow mb-5 text-[color-mix(in_srgb,var(--velion-a-earth)_78%,var(--velion-j-text))]">
				{stage.step} / {stage.state}
			</p>

			<h2 className="m-0 max-w-[10.5ch] font-arbeit text-[clamp(3rem,5.4vw,7.4rem)] font-light leading-[0.9] tracking-[-0.07em] text-velion-j-text text-balance">
				{stage.title}
			</h2>

			<p className="mt-[clamp(22px,2.4vw,36px)] max-w-[520px] font-protokoll text-[clamp(1rem,1.02vw,1.18rem)] font-light leading-[1.5] text-velion-text-muted text-pretty">
				{stage.body}
			</p>

			{index === 0 ? (
				<div className="mt-[clamp(30px,3.8vw,52px)]">
					<ArrowButton href="#plattform" variant="coral">
						Følg arbeidssløyfen
					</ArrowButton>
				</div>
			) : null}
		</div>
	);
}

function LoopFrameChrome() {
	return (
		<div
			aria-hidden="true"
			className="pointer-events-none absolute inset-0 z-20 text-velion-j-text/14"
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
	return (
		<div className="hidden max-[899px]:grid max-[899px]:gap-10">
			{loopStages.map((stage) => (
				<article
					className="grid gap-5 border-t border-velion-j-text/10 pt-5"
					key={stage.step}
				>
					<p className="velion-eyebrow text-velion-coral">
						{stage.step} / {stage.state}
					</p>
					<h3 className="m-0 font-arbeit text-[clamp(2.5rem,12vw,4.6rem)] font-light leading-[0.94] tracking-[-0.065em] text-velion-j-text text-balance">
						{stage.title}
					</h3>
					<p className="max-w-[34rem] font-protokoll text-[1rem] font-light leading-[1.48] text-velion-text-muted">
						{stage.body}
					</p>
					<div
						aria-hidden="true"
						className="relative aspect-[1.5] overflow-hidden rounded-[16px] border border-velion-j-text/8 bg-[#f8f8f7] shadow-[0_18px_56px_rgba(23,23,23,0.08)]"
					>
						<ProductLoopMediaContent media={stage} />
					</div>
				</article>
			))}
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

		const reduceMotion = window.matchMedia(
			"(prefers-reduced-motion: reduce)",
		).matches;
		const pin = section.querySelector<HTMLElement>(
			"[data-product-loop-pin]",
		);
		const viewport = section.querySelector<HTMLElement>(
			"[data-product-loop-viewport]",
		);
		const frame = section.querySelector<HTMLElement>(
			"[data-product-loop-frame]",
		);
		const chrome = section.querySelector<HTMLElement>(
			"[data-product-loop-frame-chrome]",
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
			section.querySelectorAll<HTMLElement>("[data-product-loop-layer]"),
		);
		const panels = Array.from(
			section.querySelectorAll<HTMLElement>(
				"[data-product-loop-copy-panel]",
			),
		);
		const stateCards = Array.from(
			section.querySelectorAll<HTMLElement>("[data-product-loop-state]"),
		);
		const stateAccents = Array.from(
			section.querySelectorAll<HTMLElement>(
				"[data-product-loop-state-accent]",
			),
		);

		if (
			reduceMotion ||
			window.innerWidth < 900 ||
			!pin ||
			!viewport ||
			!frame ||
			!chrome ||
			!copyFloat ||
			frameMarkers.length !== loopStages.length ||
			copyMarkers.length !== loopStages.length ||
			layers.length !== loopStages.length + 1 ||
			panels.length !== loopStages.length ||
			stateCards.length !== loopStages.length ||
			stateAccents.length !== loopStages.length
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

		const placeFrame = (index: number) =>
			boxFromMarker(frameMarkers[index]);
		const placeCopy = (index: number) => boxFromMarker(copyMarkers[index]);
		const sectionTop = () =>
			section.getBoundingClientRect().top + window.scrollY;
		const sectionScrollRange = () =>
			Math.max(
				window.innerHeight,
				section.offsetHeight - window.innerHeight,
			);
		const fullFrame = () => {
			const viewportRect = viewport.getBoundingClientRect();

			return {
				height: viewportRect.height,
				left: 0,
				top: 0,
				width: viewportRect.width,
			};
		};

		let timeline: gsap.core.Timeline | null = null;

		const context = gsap.context(() => {
			const expandingStageIndex = loopStages.findIndex(
				(stage) => stage.expands,
			);
			const introLayer = layers[0];
			const stageLayers = layers.slice(1);

			gsap.set(frame, {
				...fullFrame(),
				backgroundColor: "#f8f8f7",
				borderColor: "rgba(31,31,29,0)",
				borderRadius: "0px",
				boxShadow: "none",
				force3D: true,
				transformOrigin: "50% 50%",
			});
			gsap.set(chrome, { autoAlpha: 0 });
			gsap.set(copyFloat, {
				...placeCopy(0),
				autoAlpha: 0,
				pointerEvents: "none",
				y: 28,
			});
			gsap.set(introLayer, { autoAlpha: 1, clipPath: "inset(0% 0% 0%)" });
			gsap.set(stageLayers, {
				autoAlpha: 0,
				clipPath: "inset(0% 0% 100%)",
			});
			gsap.set(stageLayers, {
				force3D: true,
				scale: (index) => (index === expandingStageIndex ? 1.06 : 1),
				transformOrigin: "50% 50%",
			});
			gsap.set(panels, {
				autoAlpha: 0,
				y: 16,
				pointerEvents: "none",
			});
			gsap.set(stateCards, {
				autoAlpha: 0,
				y: 8,
				force3D: true,
			});
			gsap.set(stateAccents, {
				scaleX: 0.12,
				transformOrigin: "0% 50%",
			});

			const loopTimeline = gsap.timeline({
				defaults: { ease: "none" },
				paused: true,
			});
			timeline = loopTimeline;

			const activateStage = (stageIndex: number, at: number) => {
				loopTimeline
					.to(
						stateCards,
						{
							autoAlpha: (index) =>
								index === stageIndex ? 1 : 0.42,
							y: (index) => (index === stageIndex ? 0 : 8),
							duration: 0.22,
						},
						at,
					)
					.to(
						stateAccents,
						{
							scaleX: (index) =>
								index === stageIndex ? 1 : 0.12,
							duration: 0.22,
						},
						at,
					);
			};

			const transition = (from: number, to: number, at: number) => {
				loopTimeline
					.to(frame, { ...placeFrame(to), duration: 0.48 }, at)
					.to(copyFloat, { ...placeCopy(to), duration: 0.48 }, at)
					.to(
						stageLayers[from],
						{
							autoAlpha: 0,
							clipPath: "inset(100% 0% 0%)",
							duration: 0.3,
						},
						at + 0.08,
					)
					.to(
						stageLayers[to],
						{
							autoAlpha: 1,
							clipPath: "inset(0% 0% 0%)",
							duration: 0.38,
						},
						at + 0.14,
					)
					.to(
						panels[from],
						{
							autoAlpha: 0,
							y: -14,
							pointerEvents: "none",
							duration: 0.22,
						},
						at + 0.02,
					)
					.to(
						panels[to],
						{
							autoAlpha: 1,
							y: 0,
							pointerEvents: "auto",
							duration: 0.3,
						},
						at + 0.28,
					);

				activateStage(to, at + 0.16);
			};

			// The dashboard acts as the full-screen product prologue. It contracts into
			// stage 01, then every numbered stage receives its own readable beat before
			// the final knowledge view expands back to the full product frame.
			loopTimeline
				.to(
					frame,
					{
						...placeFrame(0),
						borderColor: "rgba(31,31,29,0.08)",
						borderRadius: "20px",
						boxShadow: "0 28px 96px rgba(23,23,23,0.09)",
						duration: 0.7,
					},
					0,
				)
				.to(
					introLayer,
					{
						autoAlpha: 0,
						clipPath: "inset(100% 0% 0%)",
						duration: 0.3,
					},
					0.24,
				)
				.to(
					stageLayers[0],
					{
						autoAlpha: 1,
						clipPath: "inset(0% 0% 0%)",
						duration: 0.38,
					},
					0.31,
				)
				.to(chrome, { autoAlpha: 1, duration: 0.22 }, 0.48)
				.to(
					copyFloat,
					{
						...placeCopy(0),
						autoAlpha: 1,
						pointerEvents: "auto",
						y: 0,
						duration: 0.34,
					},
					0.52,
				)
				.to(
					panels[0],
					{
						autoAlpha: 1,
						pointerEvents: "auto",
						y: 0,
						duration: 0.3,
					},
					0.56,
				)
				.to(
					stateCards,
					{
						autoAlpha: (index) => (index === 0 ? 1 : 0.42),
						y: (index) => (index === 0 ? 0 : 8),
						duration: 0.3,
					},
					0.56,
				)
				.to(
					stateAccents,
					{
						scaleX: (index) => (index === 0 ? 1 : 0.12),
						duration: 0.3,
					},
					0.56,
				);

			transition(0, 1, 1.02);
			transition(1, 2, 1.62);
			transition(2, 3, 2.22);

			if (expandingStageIndex >= 0) {
				loopTimeline
					.to(
						frame,
						{
							...fullFrame(),
							borderColor: "rgba(31,31,29,0)",
							borderRadius: "0px",
							boxShadow: "none",
							duration: 0.56,
						},
						2.82,
					)
					.to(
						stageLayers[expandingStageIndex],
						{
							scale: 1,
							duration: 0.56,
						},
						2.82,
					)
					.to(chrome, { autoAlpha: 0, duration: 0.18 }, 2.84)
					.to(
						copyFloat,
						{
							autoAlpha: 0,
							pointerEvents: "none",
							y: -26,
							duration: 0.18,
						},
						2.84,
					)
					.to(
						stateCards,
						{
							autoAlpha: 0,
							y: 12,
							duration: 0.18,
						},
						2.86,
					)
					.to(
						stateAccents,
						{
							scaleX: 0.12,
							duration: 0.18,
						},
						2.86,
					);
			}
		}, section);

		let progressFrame: number | null = null;
		let progressTween: gsap.core.Tween | null = null;
		let refreshFrame: number | null = null;
		let refreshTimeout: number | null = null;

		const updateProgress = (immediate = false) => {
			if (!timeline) {
				return;
			}

			const progress = gsap.utils.clamp(
				0,
				1,
				(window.scrollY - sectionTop()) / sectionScrollRange(),
			);

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
					ScrollTrigger.refresh();
					updateProgress(true);
				}, 80);
			});
		};

		const refreshObserver =
			typeof ResizeObserver === "undefined"
				? null
				: new ResizeObserver(queueRefresh);
		refreshObserver?.observe(section);

		if (section.previousElementSibling instanceof HTMLElement) {
			refreshObserver?.observe(section.previousElementSibling);
		}

		document.fonts?.ready.then(queueRefresh);

		if (document.readyState === "complete") {
			queueRefresh();
		} else {
			window.addEventListener("load", queueRefresh, { once: true });
		}

		updateProgress(true);
		window.addEventListener("scroll", queueProgress, { passive: true });
		window.addEventListener("resize", queueRefresh);

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

			progressTween?.kill();
			window.removeEventListener("scroll", queueProgress);
			window.removeEventListener("resize", queueRefresh);
			window.removeEventListener("load", queueRefresh);
			refreshObserver?.disconnect();
			context.revert();
		};
	}, []);

	// h-[377svh]: pin length derived from the timeline above, not chosen by eye.
	// sectionScrollRange() = offsetHeight - innerHeight, and updateProgress() maps
	// that whole range to the timeline's 0-1 progress, so vh-per-timeline-unit is
	// (heightMultiple - 1) / totalTimelineUnits. The timeline above totals 3.38
	// units, so 377 keeps ~0.82vh per unit — the same pace the beats had before
	// the dead zones were removed. Retiming the timeline without adjusting this
	// number makes every beat play slower or faster than intended (Law 3).
	return (
		<section
			aria-labelledby="product-loop-title"
			className="relative isolate z-[1] h-[377svh] overflow-visible border-t border-velion-j-text/8 bg-background text-velion-j-text max-[899px]:h-auto max-[899px]:overflow-hidden"
			data-product-loop
			id="flyt"
			ref={sectionRef}
		>
			{/* Anchor for the navbar's "Produkt" link; not a visible element. */}
			<span
				aria-hidden="true"
				className="absolute left-0 top-0 h-px w-px"
				id="produkt"
			/>

			<div
				className="sticky top-0 min-h-svh overflow-hidden max-[899px]:relative max-[899px]:min-h-0"
				data-product-loop-pin=""
			>
				<div
					className="relative min-h-svh overflow-hidden max-[899px]:grid max-[899px]:min-h-0 max-[899px]:gap-10 max-[899px]:px-[var(--velion-page-pad)] max-[899px]:py-24"
					data-product-loop-viewport=""
				>
					<div
						aria-hidden="true"
						className="absolute inset-0 bg-[linear-gradient(90deg,rgba(23,23,23,0.048)_1px,transparent_1px),linear-gradient(180deg,rgba(23,23,23,0.04)_1px,transparent_1px)] bg-[length:calc(100%/4)_calc(100%/3),calc(100%/4)_calc(100%/3)] max-[899px]:bg-[length:92px_92px]"
					/>
					<div
						aria-hidden="true"
						className="absolute inset-0 bg-[radial-gradient(circle_at_78%_28%,rgba(238,122,80,0.08),transparent_27%),radial-gradient(circle_at_14%_74%,rgba(41,64,74,0.07),transparent_28%),linear-gradient(180deg,rgba(248,248,247,0),rgba(248,248,247,0.78))]"
					/>

					<div
						aria-hidden="true"
						className="pointer-events-none absolute inset-x-4 top-0 bottom-0 border-x border-velion-j-text/6 max-[899px]:hidden"
					/>

					<div
						aria-hidden="true"
						className="absolute inset-0 max-[899px]:hidden"
					>
						<div
							className="absolute right-[clamp(56px,6.6vw,128px)] top-[clamp(142px,17vh,206px)] h-[clamp(344px,47vh,560px)] w-[clamp(610px,58vw,1120px)]"
							data-product-loop-frame-marker=""
						/>
						<div
							className="absolute right-[clamp(96px,10vw,190px)] top-[clamp(210px,30vh,310px)] h-[clamp(370px,45vh,500px)] w-[clamp(320px,27vw,470px)]"
							data-product-loop-frame-marker=""
						/>
						<div
							className="absolute left-[clamp(92px,9vw,176px)] top-[clamp(222px,34vh,350px)] h-[clamp(260px,34vh,390px)] w-[clamp(520px,43vw,760px)]"
							data-product-loop-frame-marker=""
						/>
						<div
							className="absolute left-[clamp(76px,7vw,150px)] top-[clamp(154px,19vh,230px)] h-[clamp(336px,45vh,520px)] w-[clamp(560px,50vw,940px)]"
							data-product-loop-frame-marker=""
						/>

						<div
							className="absolute left-[clamp(72px,7vw,142px)] top-[clamp(214px,27vh,310px)] h-[360px] w-[min(38vw,620px)]"
							data-product-loop-copy-marker=""
						/>
						<div
							className="absolute left-[clamp(90px,8vw,160px)] top-[clamp(238px,33vh,370px)] h-[320px] w-[min(42vw,680px)]"
							data-product-loop-copy-marker=""
						/>
						<div
							className="absolute right-[clamp(86px,8vw,168px)] top-[clamp(236px,34vh,374px)] h-[320px] w-[min(42vw,680px)]"
							data-product-loop-copy-marker=""
						/>
						<div
							className="absolute right-[clamp(92px,9vw,176px)] top-[clamp(244px,33vh,384px)] h-[320px] w-[min(40vw,650px)]"
							data-product-loop-copy-marker=""
						/>
					</div>

					<div
						className="absolute z-20 overflow-hidden rounded-[20px] border border-velion-j-text/8 bg-[#f8f8f7] shadow-[0_28px_96px_rgba(23,23,23,0.09)] max-[899px]:relative max-[899px]:left-auto max-[899px]:top-auto max-[899px]:aspect-[1.62] max-[899px]:h-auto max-[899px]:w-full"
						data-product-loop-frame=""
					>
						<div className="relative h-full w-full">
							<LoopMediaLayer index={0} media={loopIntro} />
							{loopStages.map((stage, index) => (
								<LoopMediaLayer
									index={index + 1}
									key={`${stage.step}-${stage.kind}`}
									media={stage}
								/>
							))}
						</div>
						<LoopFrameChrome />
					</div>

					<div
						className="absolute z-30 max-[899px]:hidden"
						data-product-loop-copy=""
					>
						{loopStages.map((stage, index) => (
							<LoopCopyPanel
								index={index}
								key={`${stage.step}-${stage.title}`}
								stage={stage}
							/>
						))}
					</div>

					<div
						aria-label="Velion arbeidssløyfe, fra signal til revidert handling"
						className="absolute bottom-[clamp(42px,6vh,70px)] left-1/2 z-30 grid w-[min(86vw,980px)] -translate-x-1/2 grid-cols-4 gap-[clamp(8px,0.8vw,12px)] max-[899px]:relative max-[899px]:bottom-auto max-[899px]:left-auto max-[899px]:z-auto max-[899px]:w-full max-[899px]:translate-x-0 max-[640px]:grid-cols-2"
					>
						{loopStages.map((stage, index) => (
							<div
								className="relative overflow-hidden border border-velion-j-text/10 bg-white/52 px-4 py-3 backdrop-blur-[14px]"
								data-product-loop-state=""
								key={stage.step}
								style={{
									opacity: index === 0 ? 1 : 0.42,
								}}
							>
								<span
									aria-hidden="true"
									className="absolute inset-x-0 bottom-0 h-[3px] origin-left scale-x-[0.12] bg-velion-coral/70"
									data-product-loop-state-accent=""
									style={{
										transform:
											index === 0
												? "scaleX(1)"
												: "scaleX(0.12)",
									}}
								/>
								<span className="block font-arbeit text-[0.76rem] font-normal uppercase leading-none tracking-[0.14em] text-velion-j-text/42">
									{stage.step}
								</span>
								<span className="mt-2 block font-protokoll text-[clamp(0.92rem,0.95vw,1.08rem)] font-light leading-none text-velion-j-text/76">
									{stage.state}
								</span>
							</div>
						))}
					</div>

					<ProductLoopMobileFallback />
				</div>
			</div>

			<h2 className="sr-only" id="product-loop-title">
				Arbeidssløyfen fra første signal til revidert handling
			</h2>
		</section>
	);
}

export default ProductLoopSection;
