"use client";

import Image from "next/image";
import { useLayoutEffect, useRef } from "react";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { VelionComposerPreview } from "@/components/ui/VelionComposerPreview";

gsap.registerPlugin(ScrollTrigger);

type MediaLayer = {
	alt: string;
	kind: "image" | "video";
	poster?: string;
	src: string;
};

type WorkflowCard = {
	href: string;
	image: string;
	kicker: string;
	link: string;
	title: string;
	text: string;
};

const mediaLayers: MediaLayer[] = [
	{
		kind: "video",
		src: "/velion-product-shots/velion-dashboard-typing.mp4",
		poster: "/velion-product-shots/dashboard-expanded-prompt.png",
		alt: "Velion workspace dashboard in use.",
	},
	{
		kind: "image",
		src: "/velion-product-shots/chat-agent-steps.png",
		alt: "Velion agent steps and approval trace.",
	},
	{
		kind: "image",
		src: "/velion-vibe/human-haze.png",
		alt: "Customer context signal silhouettes.",
	},
];

const copyPanels = [
	{
		body: "Velion turns company knowledge into governed action, with every source, answer, and approval kept inside one operating surface.",
		cta: undefined,
		align: "left",
	},
	{
		body: "Human approval gates, EU processing, and traceable memory work together so customer work can move faster without losing control.",
		cta: "Trust center",
		align: "left",
	},
	{
		body: "Trusted automation scales when every workflow carries its evidence, ownership, and audit trail with it.",
		cta: undefined,
		align: "center",
	},
];

const promptText =
	"Lag en løsning for dagens uløste kundesamtaler: svarutkast, kilder, prioritet og godkjenning.";

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
		text: "Kobler til nettsider, dokumenter og integrasjoner, og bygger en arbeidsminne mennesker kan inspisere.",
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

const featureSourceFramePlacement =
	"left-1/2 top-[clamp(390px,40vh,300px)] aspect-[3/4] w-[min(28vh,260px)] -translate-x-1/2";

function MediaLayerView({
	index,
	layer,
}: {
	index: number;
	layer: MediaLayer;
}) {
	return (
		<div
			aria-hidden="true"
			className="absolute inset-0 overflow-hidden"
			data-trust-layer={index}
			style={{
				clipPath: index === 0 ? "inset(0% 0% 0%)" : "inset(100% 0% 0%)",
				opacity: index === 0 ? 1 : 0,
			}}
		>
			{layer.kind === "video" ? (
				<video
					autoPlay
					className="h-full w-full object-cover"
					loop
					muted
					playsInline
					poster={layer.poster}
					preload="metadata"
				>
					<source src={layer.src} type="video/mp4" />
				</video>
			) : (
				<Image
					alt={layer.alt}
					className={[
						"object-cover",
						index === 2 ? "opacity-70 saturate-[0.72]" : "",
					].join(" ")}
					fill
					sizes="(max-width: 900px) 92vw, 74vw"
					src={layer.src}
				/>
			)}
		</div>
	);
}

function CopyPanel({
	index,
	panel,
}: {
	index: number;
	panel: (typeof copyPanels)[number];
}) {
	return (
		<div
			className={[
				"absolute left-0 top-0 max-w-[680px] text-velion-j-text/68",
				panel.align === "center" ? "text-center" : "text-left",
			].join(" ")}
			data-trust-copy-panel={index}
			style={{
				opacity: index === 0 ? 1 : 0,
				pointerEvents: index === 0 ? "auto" : "none",
				visibility: index === 0 ? "visible" : "hidden",
			}}
		>
			<p className="m-0 font-protokoll text-[clamp(1rem,1.18vw,1.35rem)] font-light leading-[1.38] text-pretty">
				{panel.body}
			</p>

			{panel.cta ? (
				<a
					className="mt-9 inline-flex h-11 items-center border border-velion-j-text/10 bg-velion-j-text/[0.035] px-5 font-protokoll text-[0.82rem] font-light leading-none text-velion-j-text/64 transition-colors hover:bg-velion-j-text/[0.075] hover:text-velion-j-text"
					href="/trust"
				>
					{panel.cta}
				</a>
			) : null}
		</div>
	);
}

function ArrowGlyph({ className = "" }: { className?: string }) {
	return (
		<svg
			aria-hidden="true"
			className={["h-[10px] fill-current lg:h-[11px]", className]
				.filter(Boolean)
				.join(" ")}
			viewBox="0 0 22.35 7.16"
			xmlns="http://www.w3.org/2000/svg"
		>
			<path
				d="m18.77 0 3.58 3.58c-.76 0-1.52-.29-2.1-.87l-2.1-2.1.62-.61zm-.61 6.54 2.1-2.1c.58-.58 1.34-.87 2.1-.87l-3.58 3.58-.62-.61zm.28-2.53v-.87H0V4h18.44z"
				strokeWidth="0.5px"
			/>
		</svg>
	);
}

function CardArrowLabel({ children }: { children: string }) {
	return (
		<span className="relative inline-flex w-fit items-center overflow-hidden px-px py-1 font-protokoll text-[clamp(0.94rem,0.95vw,1.08rem)] font-light leading-none text-velion-j-text/62 opacity-80 transition-all duration-500 group-hover:opacity-100">
			<span className="flex translate-x-[-43px] items-center gap-3 transition-transform duration-500 group-hover:translate-x-0 sm:translate-x-[-37px] sm:gap-1">
				<ArrowGlyph />
				<span className="whitespace-nowrap">{children}</span>
				<ArrowGlyph className="absolute left-full translate-x-3 sm:translate-x-1" />
			</span>
		</span>
	);
}

function FeaturePromptComposer() {
	return (
		<div
			className="absolute bottom-[clamp(132px,17vh,182px)] left-1/2 z-40 w-[min(91vw,760px)] origin-bottom -translate-x-1/2 opacity-0 will-change-transform max-[899px]:hidden"
			data-feature-prompt=""
		>
			<VelionComposerPreview animateCharacters prompt={promptText} />
		</div>
	);
}

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
			className="group flex min-w-0 flex-col gap-3 will-change-[transform,opacity,filter]"
			href={card.href}
			{...(animated ? { "data-feature-card": "" } : {})}
		>
			<div className="flex h-[15px] select-none items-center justify-between font-protokoll text-[10px] leading-none text-velion-j-text/42">
				<span className="flex items-center gap-1">
					<span className="size-[5px] rounded-full bg-velion-coral/70" />
					{card.kicker}
				</span>
				<span>Velion output</span>
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

			<CardArrowLabel>{card.link}</CardArrowLabel>
		</a>
	);
}

function FeatureOutputStage() {
	return (
		<div
			className="absolute bottom-[clamp(85px,14vh,115px)] left-1/2 z-40 grid w-[min(90vw,1180px,max(860px,calc(214svh_-_706px)))] -translate-x-1/2 grid-cols-4 gap-[clamp(12px,1.25vw,22px)] opacity-0 max-[899px]:hidden"
			data-feature-output-stage=""
		>
			{workflowCards.map((card) => (
				<FeatureOutputCard card={card} key={card.title} />
			))}
		</div>
	);
}

function frameChrome() {
	const positions = ["left", "right", "top", "bottom"] as const;
	const corners = [
		"left-[-3px] top-[-3px]",
		"right-[-3px] top-[-3px]",
		"bottom-[-3px] left-[-3px]",
		"bottom-[-3px] right-[-3px]",
	];

	return (
		<>
			<span
				aria-hidden="true"
				className="pointer-events-none absolute inset-0 z-20 text-velion-j-text/12"
			>
				<svg
					className="h-full w-full"
					preserveAspectRatio="none"
					viewBox="0 0 100 100"
				>
					<path
						d="M0 100 L100 0"
						fill="none"
						stroke="currentColor"
						strokeWidth="0.55"
						vectorEffect="non-scaling-stroke"
					/>
				</svg>
			</span>

			{positions.map((position) => (
				<span
					aria-hidden="true"
					className={[
						"pointer-events-none absolute z-30 bg-velion-j-text/16",
						position === "left" || position === "right"
							? "top-0 h-full w-px"
							: "left-0 h-px w-full",
						position === "left" ? "left-0" : "",
						position === "right" ? "right-0" : "",
						position === "top" ? "top-0" : "",
						position === "bottom" ? "bottom-0" : "",
					].join(" ")}
					key={position}
				/>
			))}

			{corners.map((corner) => (
				<span
					aria-hidden="true"
					className={`pointer-events-none absolute z-40 size-[6px] rounded-full bg-velion-j-text/24 ${corner}`}
					key={corner}
				/>
			))}
		</>
	);
}

function FeatureHandoffIntro() {
	return (
		<div
			aria-hidden="true"
			className="pointer-events-none absolute inset-0 z-10 opacity-0 max-[899px]:hidden"
			data-trust-feature-handoff=""
		>
			<div
				className="absolute inset-0 opacity-0"
				data-trust-feature-bg=""
			>
				<div className="absolute inset-0 bg-background/92" />
				<div className="absolute inset-0 bg-[radial-gradient(circle_at_50%_34%,rgba(238,122,80,0.06),transparent_26%),radial-gradient(circle_at_12%_18%,rgba(79,125,243,0.08),transparent_24%),linear-gradient(180deg,rgba(23,23,23,0.018),transparent_44%)]" />
			</div>

			<div
				className="absolute left-1/2 top-[clamp(150px,16.5vh,172px)] w-[min(760px,calc(100%-48px))] -translate-x-1/2 text-center will-change-[transform,opacity,filter]"
				data-trust-feature-copy=""
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

function FeatureSourceChrome() {
	const corners = [
		"left-[-3.5px] top-[-3.5px]",
		"right-[-3.5px] top-[-3.5px]",
		"bottom-[-3.5px] left-[-3.5px]",
		"bottom-[-3.5px] right-[-3.5px]",
	];

	return (
		<div
			aria-hidden="true"
			className={[
				"pointer-events-none absolute z-30 opacity-0 max-[899px]:hidden",
				featureSourceFramePlacement,
			].join(" ")}
			data-trust-feature-source-chrome=""
		>
			<div className="absolute inset-x-0 top-[-20px] flex h-[15px] select-none items-center justify-between font-protokoll text-[10px] leading-none text-velion-j-text/38">
				<span className="flex items-center gap-1">
					<span className="size-[5px] rounded-full bg-velion-coral/70" />
					signal
				</span>
				<span>customer context</span>
			</div>

			{corners.map((position) => (
				<span
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

function FeatureMobileFallback() {
	return (
		<div className="hidden max-[899px]:grid max-[899px]:gap-10 max-[899px]:pt-10">
			<div className="text-center">
				<p className="mb-4 font-protokoll text-[0.7rem] font-medium uppercase leading-none tracking-[0.3em] text-velion-j-text/38">
					Forankret handling
				</p>
				<h2 className="m-0 font-arbeit text-[clamp(3rem,14vw,5rem)] font-light leading-[0.95] tracking-[-0.07em] text-velion-j-text">
					Tenking, satt i arbeid
				</h2>
				<p className="mx-auto mt-5 max-w-[34rem] font-protokoll text-[1rem] font-light leading-[1.48] text-velion-text-muted">
					Ett kundesignal kan bli et svar, en rute, en policy-sjekk og
					en revidert handling — alt med kildene synlige.
				</p>
			</div>

			<div className="mx-auto grid w-full max-w-[760px] gap-8">
				<div className="relative mx-auto aspect-[3/4] w-[min(72vw,270px)] border border-velion-j-text/10 bg-[#f8f8f7] shadow-[0_10px_28px_rgba(23,23,23,0.035)]">
					<Image
						alt="Customer context signal silhouettes."
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

export function TrustScrollSection() {
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
			"[data-trust-scroll-pin]",
		);
		const viewport = section.querySelector<HTMLElement>(
			"[data-trust-scroll-viewport]",
		);
		const frame = section.querySelector<HTMLElement>("[data-trust-frame]");
		const chrome = section.querySelector<HTMLElement>(
			"[data-trust-frame-chrome]",
		);
		const copyFloat =
			section.querySelector<HTMLElement>("[data-trust-copy]");
		const markers = Array.from(
			section.querySelectorAll<HTMLElement>("[data-trust-frame-marker]"),
		);
		const copyMarkers = Array.from(
			section.querySelectorAll<HTMLElement>("[data-trust-copy-marker]"),
		);
		const layers = Array.from(
			section.querySelectorAll<HTMLElement>("[data-trust-layer]"),
		);
		const panels = Array.from(
			section.querySelectorAll<HTMLElement>("[data-trust-copy-panel]"),
		);
		const featureHandoff = section.querySelector<HTMLElement>(
			"[data-trust-feature-handoff]",
		);
		const featureBg = section.querySelector<HTMLElement>(
			"[data-trust-feature-bg]",
		);
		const featureCopy = section.querySelector<HTMLElement>(
			"[data-trust-feature-copy]",
		);
		const featureSourceChrome = section.querySelector<HTMLElement>(
			"[data-trust-feature-source-chrome]",
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

		if (
			reduceMotion ||
			window.innerWidth < 900 ||
			!pin ||
			!viewport ||
			!frame ||
			!chrome ||
			!copyFloat ||
			!featureHandoff ||
			!featureBg ||
			!featureCopy ||
			!featureSourceChrome ||
			!prompt ||
			!outputStage ||
			!processing ||
			!sendArrow ||
			!sendButton ||
			!sendRing ||
			cards.length === 0 ||
			markers.length !== 4 ||
			copyMarkers.length !== 3
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

		const placeFrame = (index: number) => boxFromMarker(markers[index]);
		const placeCopy = (index: number) => boxFromMarker(copyMarkers[index]);

		const context = gsap.context(() => {
			gsap.set(frame, {
				...placeFrame(0),
				backgroundColor: "#f8f8f7",
				force3D: true,
				transformOrigin: "50% 50%",
			});
			gsap.set(chrome, { autoAlpha: 1 });
			gsap.set(copyFloat, placeCopy(0));
			gsap.set(layers[0], { autoAlpha: 1, clipPath: "inset(0% 0% 0%)" });
			gsap.set(layers.slice(1), {
				autoAlpha: 0,
				clipPath: "inset(100% 0% 0%)",
			});
			gsap.set(panels[0], { autoAlpha: 1, y: 0, pointerEvents: "auto" });
			gsap.set(panels.slice(1), {
				autoAlpha: 0,
				y: 12,
				pointerEvents: "none",
			});
			gsap.set(featureHandoff, { autoAlpha: 0 });
			gsap.set(featureBg, { autoAlpha: 0 });
			gsap.set(featureCopy, {
				autoAlpha: 0,
				filter: "blur(10px)",
				y: 34,
				force3D: true,
			});
			gsap.set(featureSourceChrome, {
				autoAlpha: 0,
				y: 8,
				force3D: true,
			});
			gsap.set(prompt, {
				autoAlpha: 0,
				scale: 0.94,
				y: 42,
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
			gsap.set(sendArrow, { y: 0, force3D: true });
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
				filter: "blur(16px)",
				rotationX: -54,
				scale: 1.12,
				transformOrigin: "50% 0%",
				transformStyle: "preserve-3d",
				xPercent: (index) => [142, 48, -48, -142][index] ?? 0,
				y: -20,
				force3D: true,
			});

			const timeline = gsap.timeline({
				defaults: { ease: "none" },
				scrollTrigger: {
					trigger: section,
					start: "top top",
					end: () => `+=${Math.round(window.innerHeight * 6.75)}`,
					pin,
					scrub: 0.6,
					anticipatePin: 1,
					invalidateOnRefresh: true,
					refreshPriority: 3,
					id: "velion-trust-horizontal-scroll",
				},
			});

			timeline
				.to(frame, placeFrame(1), 0.26)
				.to(copyFloat, placeCopy(1), 0.26)
				.to(
					layers[0],
					{
						autoAlpha: 0,
						clipPath: "inset(0% 0% 100%)",
						duration: 0.28,
					},
					0.3,
				)
				.to(
					layers[1],
					{
						autoAlpha: 1,
						clipPath: "inset(0% 0% 0%)",
						duration: 0.34,
					},
					0.34,
				)
				.to(
					panels[0],
					{ autoAlpha: 0, y: -10, pointerEvents: "none" },
					0.22,
				)
				.to(
					panels[1],
					{ autoAlpha: 1, y: 0, pointerEvents: "auto" },
					0.45,
				)
				.to(frame, placeFrame(2), 1.28)
				.to(frame, { backgroundColor: "#f8f8f7", duration: 0.3 }, 1.28)
				.to(copyFloat, placeCopy(2), 1.28)
				.to(
					layers[1],
					{
						autoAlpha: 0,
						clipPath: "inset(0% 0% 100%)",
						duration: 0.28,
					},
					1.28,
				)
				.to(
					layers[2],
					{
						autoAlpha: 1,
						clipPath: "inset(0% 0% 0%)",
						duration: 0.38,
					},
					1.34,
				)
				.to(
					panels[1],
					{ autoAlpha: 0, y: -10, pointerEvents: "none" },
					1.16,
				)
				.to(
					panels[2],
					{ autoAlpha: 1, y: 0, pointerEvents: "auto" },
					1.48,
				)
				.to(
					panels[2],
					{ autoAlpha: 0, y: -12, pointerEvents: "none" },
					2.04,
				)
				.to(
					copyFloat,
					{ autoAlpha: 0, y: -12, pointerEvents: "none" },
					2.04,
				)
				.to(chrome, { autoAlpha: 0, duration: 0.28 }, 2.06)
				.to(featureHandoff, { autoAlpha: 1, duration: 0.01 }, 2.08)
				.to(featureBg, { autoAlpha: 1, duration: 0.62 }, 2.08)
				.to(
					frame,
					{
						...placeFrame(3),
						boxShadow: "0 10px 28px rgba(23,23,23,0.035)",
						duration: 0.68,
					},
					2.18,
				)
				.to(
					featureCopy,
					{
						autoAlpha: 1,
						filter: "blur(0px)",
						y: 0,
						duration: 0.58,
					},
					2.22,
				)
				.to(
					featureSourceChrome,
					{
						autoAlpha: 1,
						y: 0,
						duration: 0.32,
					},
					2.48,
				)
				.to(frame, { y: -18, scale: 0.965, duration: 0.16 }, 3.04)
				.to(
					featureSourceChrome,
					{ y: -18, scale: 0.965, duration: 0.16 },
					3.04,
				)
				.to(
					prompt,
					{ autoAlpha: 1, y: 0, scale: 1, duration: 0.2 },
					3.12,
				)
				.to(
					chars,
					{ autoAlpha: 1, duration: 0.22, stagger: 0.003 },
					3.2,
				)
				.to(
					sendButton,
					{
						backgroundColor: "#ee7a50",
						scale: 0.88,
						duration: 0.06,
					},
					3.48,
				)
				.to(sendArrow, { y: -3, duration: 0.06 }, 3.48)
				.to(
					sendRing,
					{
						autoAlpha: 1,
						scale: 1.12,
						duration: 0.08,
					},
					3.49,
				)
				.to(
					processing,
					{
						autoAlpha: 1,
						scaleX: 1,
						duration: 0.16,
					},
					3.52,
				)
				.to(
					sendButton,
					{
						backgroundColor: "#111111",
						scale: 1,
						duration: 0.1,
					},
					3.56,
				)
				.to(sendArrow, { y: 0, duration: 0.1 }, 3.56)
				.to(
					sendRing,
					{
						autoAlpha: 0,
						scale: 1.45,
						duration: 0.14,
					},
					3.58,
				)
				.to(
					prompt,
					{
						autoAlpha: 0.18,
						y: 76,
						scale: 0.92,
						duration: 0.08,
					},
					3.6,
				)
				.to(
					frame,
					{
						autoAlpha: 0.42,
						scale: 0.76,
						y: 4,
						duration: 0.08,
					},
					3.6,
				)
				.to(
					featureSourceChrome,
					{
						autoAlpha: 0.42,
						scale: 0.76,
						y: 4,
						duration: 0.08,
					},
					3.6,
				)
				.to(outputStage, { autoAlpha: 1, duration: 0.08 }, 3.62)
				.set(outputStage, { pointerEvents: "auto" }, 3.62)
				.to(
					cards,
					{
						autoAlpha: 0.78,
						stagger: 0.045,
						duration: 0.06,
					},
					3.62,
				)
				.to(
					cards,
					{
						filter: "blur(0px)",
						scale: 1,
						xPercent: 0,
						y: 0,
						stagger: 0.055,
						duration: 0.3,
					},
					3.64,
				)
				.to(
					cards,
					{
						rotationX: 0,
						stagger: 0.055,
						duration: 0.42,
					},
					3.64,
				)
				.to(cards, { autoAlpha: 1, duration: 0.06 }, 3.84)
				.to(frame, { autoAlpha: 0, duration: 0.07 }, 3.7)
				.to(featureSourceChrome, { autoAlpha: 0, duration: 0.07 }, 3.7)
				.to(prompt, { autoAlpha: 0, duration: 0.07 }, 3.7)
				.to({}, { duration: 0.08 });
		}, section);

		return () => context.revert();
	}, []);

	return (
		<section
			aria-labelledby="trust-scroll-title"
			className="relative isolate overflow-hidden bg-background text-velion-j-text"
			id="trust-scroll"
			ref={sectionRef}
		>
			<span
				aria-hidden="true"
				className="absolute left-0 top-0 h-px w-px"
				id="produkt"
			/>

			<div
				className="relative min-h-svh overflow-hidden max-[899px]:min-h-0"
				data-trust-scroll-pin=""
			>
				<div
					className="relative min-h-svh overflow-hidden max-[899px]:grid max-[899px]:min-h-0 max-[899px]:gap-12 max-[899px]:px-[var(--velion-page-pad)] max-[899px]:py-24"
					data-trust-scroll-viewport=""
				>
					<div
						aria-hidden="true"
						className="absolute inset-0 bg-[linear-gradient(90deg,rgba(23,23,23,0.055)_1px,transparent_1px),linear-gradient(180deg,rgba(23,23,23,0.048)_1px,transparent_1px)] bg-[length:calc(100%/4)_calc(100%/3),calc(100%/4)_calc(100%/3)] max-[899px]:bg-[length:92px_92px]"
					/>

					<div
						aria-hidden="true"
						className="absolute inset-0 bg-[radial-gradient(circle_at_64%_40%,rgba(238,122,80,0.045),transparent_30%),linear-gradient(180deg,rgba(248,248,247,0),rgba(248,248,247,0.72))]"
					/>

					<FeatureHandoffIntro />

					<div
						aria-hidden="true"
						className="pointer-events-none absolute inset-x-4 top-0 bottom-0 border-x border-velion-j-text/6 max-[899px]:hidden"
					/>

					<div
						aria-hidden="true"
						className="absolute inset-0 max-[899px]:hidden"
					>
						<div
							className="absolute left-[clamp(92px,9.8vw,176px)] top-[clamp(210px,38vh,390px)] h-[clamp(218px,28vh,338px)] w-[clamp(480px,41vw,720px)]"
							data-trust-frame-marker="0"
						/>
						<div
							className="absolute right-[clamp(112px,10vw,190px)] top-[clamp(232px,32vh,324px)] h-[clamp(368px,44vh,472px)] w-[clamp(300px,24vw,430px)]"
							data-trust-frame-marker="1"
						/>
						<div
							className="absolute left-1/2 top-[clamp(132px,16vh,190px)] aspect-[3/4] w-[clamp(340px,30vw,520px)] -translate-x-1/2"
							data-trust-frame-marker="2"
						/>
						<div
							className={[
								"absolute",
								featureSourceFramePlacement,
							].join(" ")}
							data-trust-frame-marker="3"
						/>

						<div
							className="absolute left-[calc(50%+34px)] top-[clamp(310px,45vh,470px)] h-[160px] w-[min(42vw,680px)]"
							data-trust-copy-marker="0"
						/>
						<div
							className="absolute left-[clamp(92px,9.8vw,176px)] top-[clamp(310px,43vh,430px)] h-[210px] w-[min(43vw,690px)]"
							data-trust-copy-marker="1"
						/>
						<div
							className="absolute left-1/2 top-[clamp(348px,45vh,460px)] h-[200px] w-[min(56vw,780px)] -translate-x-1/2"
							data-trust-copy-marker="2"
						/>
					</div>

					<div
						className="absolute z-20 overflow-hidden border border-velion-j-text/8 bg-[#f8f8f7] shadow-[0_18px_56px_rgba(23,23,23,0.065)] max-[899px]:relative max-[899px]:left-auto max-[899px]:top-auto max-[899px]:h-auto max-[899px]:w-full max-[899px]:aspect-[1.5]"
						data-trust-frame=""
					>
						<div className="relative h-full w-full">
							{mediaLayers.map((layer, index) => (
								<MediaLayerView
									index={index}
									key={`${layer.src}-${index}`}
									layer={layer}
								/>
							))}
						</div>
						<div
							aria-hidden="true"
							className="pointer-events-none absolute inset-0 z-20"
							data-trust-frame-chrome=""
						>
							{frameChrome()}
						</div>
					</div>

					<FeatureSourceChrome />
					<FeaturePromptComposer />
					<FeatureOutputStage />

					<div
						className="absolute z-30 max-[899px]:hidden"
						data-trust-copy=""
					>
						{copyPanels.map((panel, index) => (
							<CopyPanel
								index={index}
								key={`${panel.body}-${index}`}
								panel={panel}
							/>
						))}
					</div>

					<div
						aria-hidden="true"
						className="pointer-events-none absolute inset-0 max-[899px]:hidden"
					>
						<span className="absolute left-4 top-[calc(100%/3)] size-[7px] -translate-y-1/2 rounded-full bg-velion-j-text/18" />
						<span className="absolute bottom-[calc(100%/3)] left-4 size-[7px] translate-y-1/2 rounded-full bg-velion-j-text/18" />
						<span className="absolute right-4 top-[calc(100%/3)] size-[7px] -translate-y-1/2 rounded-full bg-velion-j-text/18" />
						<span className="absolute bottom-[calc(100%/3)] right-4 size-[7px] translate-y-1/2 rounded-full bg-velion-j-text/18" />
					</div>

					<div className="hidden max-[899px]:grid max-[899px]:gap-10">
						{copyPanels.map((panel, index) => (
							<div
								className="border-t border-velion-j-text/10 pt-5"
								key={panel.body}
							>
								<span className="mb-4 block font-arbeit text-[0.76rem] text-velion-coral">
									{String(index + 1).padStart(2, "0")}
								</span>
								<p className="m-0 font-protokoll text-[1rem] font-light leading-[1.48] text-velion-j-text/68">
									{panel.body}
								</p>
							</div>
						))}
					</div>

					<FeatureMobileFallback />
				</div>
			</div>

			<h2 className="sr-only" id="trust-scroll-title">
				Trusted AI operating layer
			</h2>
		</section>
	);
}

export default TrustScrollSection;
