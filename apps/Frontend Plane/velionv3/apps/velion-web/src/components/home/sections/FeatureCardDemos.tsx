"use client";

import Image from "next/image";
import {
	cubicBezier,
	motion,
	useAnimationFrame,
	useMotionValue,
	useReducedMotion,
	useTransform,
} from "framer-motion";
import type { MotionValue } from "framer-motion";
import { Check, Clock3, RotateCcw, Search, ShieldCheck } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { VelionMark } from "./VelionMark";

export type FeatureDemoKind = "build" | "connect" | "understand" | "delegate";

type BrandName =
	| "bring"
	| "gmail"
	| "notion"
	| "onedrive"
	| "outlook"
	| "sharepoint"
	| "slack";

const brands: Record<BrandName, { label: string; src: string }> = {
	bring: { label: "Bring", src: "/brand-logos/bring.svg" },
	gmail: { label: "Gmail", src: "/brand-logos/gmail.svg" },
	notion: { label: "Notion", src: "/brand-logos/notion.svg" },
	onedrive: { label: "OneDrive", src: "/brand-logos/onedrive.svg" },
	outlook: { label: "Outlook", src: "/brand-logos/outlook.svg" },
	sharepoint: { label: "SharePoint", src: "/brand-logos/sharepoint.svg" },
	slack: { label: "Slack", src: "/brand-logos/slack.svg" },
};

/*
 * Loop architecture (rebuilt after expert research — see the PR notes):
 *
 * ONE CLOCK PER CARD. Every animated group derives its opacity/y from a
 * single progress MotionValue (0→1 over LOOP_MS) via useTransform. This is
 * the Motion-docs-blessed pattern for perpetual loops: with independent
 * `repeat: Infinity` animations, opacity runs on the compositor while y
 * runs on the rAF loop (two engines, permanent phase skew), and any React
 * re-render restarts one sibling and desyncs the scene forever. One clock
 * makes sync and an exact wrap true by construction.
 *
 * TWO BEATS + PERSISTENT ANCHOR. Each demo keeps its input element (task
 * card / channel row / search bar / header) permanently visible — the card
 * never reads as empty. Below it, beat A (the working detail) and beat B
 * (the result) alternate. B's entrance overlaps A's exit by ~200ms — a
 * Material fade-through crossfade, never a blank stage — and the loop ends
 * on a 0.8s rest that hides the wrap seam (first === last value on every
 * track, so the restart is invisible).
 *
 * SINE, SUB-SECOND FADES. All fades use easeInOutSine — zero velocity at
 * BOTH ends of every segment, so there are no kicks at keyframe
 * boundaries (the previous easeOutExpo restarted at 6x average velocity on
 * every segment). Enter 600ms, exit 400ms, hold:transition ≈ 5:1.
 *
 * NO ANIMATED BLUR. backdrop-filter lives only on the static DemoSurface;
 * animated panels use flat translucent fills (animating a backdrop-blur
 * node forces a full re-blur every frame, and accelerated opacity is
 * documented to break backdrop-filter rendering — motion issue #1837).
 */
const LOOP_MS = 8000;
const sine = cubicBezier(0.37, 0, 0.63, 1);
const lin = (t: number) => t;

type BeatTrack = {
	opEase: Array<(t: number) => number>;
	opStops: number[];
	opValues: number[];
	yEase: Array<(t: number) => number>;
	yStops: number[];
	yValues: number[];
};

// Fractions of the 8s loop: A enters 0–0.6s, holds, exits 3.2–3.6s; B
// enters 3.4–4.0s (200ms crossfade with A's exit), holds, exits 6.8–7.2s;
// 7.2–8.0s is the rest beat. Drift is enter-only (+8px → 0), never
// reversed on exit — exits are opacity-only so nothing bounces back.
const TRACKS: Record<"setup" | "setupLate" | "result", BeatTrack> = {
	setup: {
		opStops: [0, 0.075, 0.4, 0.45, 1],
		opValues: [0, 1, 1, 0, 0],
		opEase: [sine, lin, sine, lin],
		yStops: [0, 0.075, 1],
		yValues: [8, 0, 0],
		yEase: [sine, lin],
	},
	setupLate: {
		opStops: [0, 0.02, 0.095, 0.4, 0.45, 1],
		opValues: [0, 0, 1, 1, 0, 0],
		opEase: [lin, sine, lin, sine, lin],
		yStops: [0, 0.02, 0.095, 1],
		yValues: [8, 8, 0, 0],
		yEase: [lin, sine, lin],
	},
	result: {
		opStops: [0, 0.425, 0.5, 0.85, 0.9, 1],
		opValues: [0, 0, 1, 1, 0, 0],
		opEase: [lin, sine, lin, sine, lin],
		yStops: [0, 0.425, 0.5, 1],
		yValues: [8, 8, 0, 0],
		yEase: [lin, sine, lin],
	},
};

// One animated wrapper per beat group — leaves inside stay static, so each
// card animates 2–3 composited nodes instead of 12 (layer-count guidance
// from web.dev; fewer live tracks, fewer per-frame style writes).
function Beat({
	children,
	className,
	progress,
	staticFrame,
	track,
}: {
	children: React.ReactNode;
	className?: string;
	progress: MotionValue<number>;
	staticFrame: boolean;
	track: keyof typeof TRACKS;
}) {
	const t = TRACKS[track];
	const opacity = useTransform(progress, t.opStops, t.opValues, {
		ease: t.opEase,
	});
	const y = useTransform(progress, t.yStops, t.yValues, { ease: t.yEase });

	return (
		<motion.div
			className={className}
			style={staticFrame ? undefined : { opacity, y }}
		>
			{children}
		</motion.div>
	);
}

// The per-card clock. Advances only while the card is active (stage
// revealed / in view), so offscreen loops cost nothing and resume in
// place. phaseMs offsets each card so the four never transition at once.
function useVignetteProgress(active: boolean, phaseMs: number) {
	const elapsed = useRef(phaseMs);
	const progress = useMotionValue((phaseMs % LOOP_MS) / LOOP_MS);

	useAnimationFrame((_, delta) => {
		if (!active) {
			return;
		}

		elapsed.current += delta;
		progress.set((elapsed.current % LOOP_MS) / LOOP_MS);
	});

	return progress;
}

function BrandLogo({ name, size = 24 }: { name: BrandName; size?: number }) {
	const brand = brands[name];

	return (
		<span
			className="grid shrink-0 place-items-center rounded-[9px] border border-black/[0.055] bg-white shadow-[0_3px_10px_rgba(20,20,20,0.06)]"
			style={{ height: size + 10, width: size + 10 }}
		>
			<Image
				alt=""
				aria-hidden="true"
				height={size}
				src={brand.src}
				width={size}
			/>
		</span>
	);
}

function BrandChip({
	detail,
	name,
}: {
	detail?: string;
	name: BrandName;
}) {
	return (
		<div className="flex min-w-0 items-center gap-2 rounded-[12px] border border-black/[0.065] bg-white/92 px-2 py-1.5 shadow-[0_7px_18px_rgba(30,27,24,0.07)]">
			<BrandLogo name={name} size={18} />
			<span className="min-w-0 font-protokoll">
				<span className="block truncate text-[9px] font-medium leading-tight text-[#20201f]">
					{brands[name].label}
				</span>
				{detail ? (
					<span className="mt-0.5 block truncate text-[8px] leading-tight text-[#20201f]/70">
						{detail}
					</span>
				) : null}
			</span>
		</div>
	);
}

function BrandTile({ name }: { name: BrandName }) {
	return (
		<div className="grid min-w-0 place-items-center rounded-[12px] border border-black/[0.065] bg-white/92 px-1.5 py-2 shadow-[0_7px_18px_rgba(30,27,24,0.07)]">
			<BrandLogo name={name} size={20} />
			<span className="mt-1.5 max-w-full truncate font-protokoll text-[8px] font-medium leading-none text-[#20201f]/76">
				{brands[name].label}
			</span>
		</div>
	);
}

// The ONLY backdrop-filter surface per card, and it never animates —
// animated children above it are flat fills.
function DemoSurface({
	children,
	label,
}: {
	children: React.ReactNode;
	label: string;
}) {
	return (
		<div className="relative overflow-hidden rounded-[20px] border border-white/80 bg-white/[0.82] p-3 shadow-[0_14px_34px_rgba(35,31,27,0.09),inset_0_1px_0_rgba(255,255,255,0.9)] backdrop-blur-2xl">
			{/* Coral status dot, not emerald: on this site the pulsing coral dot
			    is the "live, waiting on a human" marker (see ApprovalQueueCard) —
			    the brand accent belongs on the chrome, not just inside one demo. */}
			<div className="mb-2.5 flex items-center justify-between border-b border-[#171717]/[0.075] pb-2 font-protokoll text-[8px] font-medium uppercase tracking-[0.14em] text-[#171717]/64">
				<span className="flex items-center gap-1.5">
					<span className="size-1.5 rounded-full bg-velion-coral motion-safe:animate-pulse" />
					{label}
				</span>
				<span className="flex items-center gap-1 text-[#171717]/68">
					Velion <VelionMark className="size-3" />
				</span>
			</div>
			{children}
		</div>
	);
}

type DemoProps = {
	progress: MotionValue<number>;
	staticFrame: boolean;
};

function BuildDemo({ progress, staticFrame }: DemoProps) {
	const sourceNames: BrandName[] = ["notion", "sharepoint", "gmail"];

	return (
		<DemoSurface label="Bygg en agent">
			<div className="relative h-[202px]">
				{/* Anchor — the task never leaves; the card is never empty. */}
				<div className="rounded-[13px] border border-[#171717]/[0.075] bg-white/90 px-3 py-2.5 shadow-[0_8px_18px_rgba(30,27,24,0.06)]">
					<p className="m-0 font-protokoll text-[8px] uppercase tracking-[0.12em] text-[#171717]/68">
						Oppgave
					</p>
					<p className="m-0 mt-1.5 font-protokoll text-[10px] font-medium leading-[1.35] text-[#171717]/82">
						Svar på returspørsmål i vår tone
					</p>
				</div>

				<Beat
					className="absolute inset-x-0 top-[66px] grid grid-cols-3 gap-1.5"
					progress={progress}
					staticFrame={staticFrame}
					track="setup"
				>
					{sourceNames.map((name) => (
						<BrandTile key={name} name={name} />
					))}
				</Beat>

				<Beat
					className="absolute inset-x-1 bottom-0 rounded-[16px] bg-[#191919] p-3 text-white shadow-[0_12px_24px_rgba(23,23,23,0.18)]"
					progress={progress}
					staticFrame={staticFrame}
					track="result"
				>
					<div className="flex items-center gap-2.5">
						<span className="grid size-9 place-items-center rounded-full bg-velion-coral">
							<VelionMark className="size-5 text-white" />
						</span>
						<span className="min-w-0 font-protokoll">
							<span className="block text-[11px] font-medium">Returagent</span>
							<span className="mt-0.5 block text-[8px] text-white/72">
								3 kilder · 2 regler · norsk tone
							</span>
						</span>
					</div>
					<div className="mt-2.5 flex items-center justify-between border-t border-white/10 pt-2 font-protokoll text-[8px]">
						<span className="flex items-center gap-1.5 text-emerald-300">
							<Check className="size-3" /> Klar til forhåndsvisning
						</span>
						<span className="text-white/70">01 / 01</span>
					</div>
				</Beat>
			</div>
		</DemoSurface>
	);
}

function ConnectDemo({ progress, staticFrame }: DemoProps) {
	const channels: BrandName[] = ["outlook", "gmail", "slack"];

	return (
		<DemoSurface label="Koble arbeidet">
			<div className="relative h-[202px]">
				{/* Anchor — the connected systems stay on. */}
				<div className="grid grid-cols-3 gap-1.5">
					{channels.map((name) => (
						<BrandTile key={name} name={name} />
					))}
				</div>

				<Beat
					className="absolute inset-x-3 top-[65px] rounded-[14px] border border-white/75 bg-white/92 p-2.5 shadow-[0_12px_24px_rgba(30,27,24,0.09)]"
					progress={progress}
					staticFrame={staticFrame}
					track="setup"
				>
					<div className="flex items-center justify-between font-protokoll">
						<span className="text-[9px] font-medium text-[#171717]/80">
							Hvor er pakken min?
						</span>
						<span className="text-[8px] font-medium text-velion-coral">#52481</span>
					</div>
				</Beat>

				<Beat
					className="absolute inset-x-0 bottom-0 rounded-[16px] border border-[#171717]/[0.075] bg-[#fffefa] p-3 shadow-[0_12px_26px_rgba(30,27,24,0.10)]"
					progress={progress}
					staticFrame={staticFrame}
					track="result"
				>
					<div className="flex items-center gap-2.5">
						<BrandLogo name="bring" size={28} />
						<div className="min-w-0 flex-1 font-protokoll">
							<div className="flex items-center justify-between gap-2">
								<span className="truncate text-[10px] font-medium text-[#171717]/82">
									Pakke underveis
								</span>
								<span className="rounded-full bg-emerald-50 px-2 py-1 text-[7px] text-emerald-700">
									Live
								</span>
							</div>
							<p className="m-0 mt-1 text-[8px] text-[#171717]/68">
								Oslo terminal · forventet i morgen
							</p>
						</div>
					</div>
					<div className="mt-2 flex items-center gap-1.5 border-t border-[#171717]/[0.07] pt-2 font-protokoll text-[8px] text-[#171717]/66">
						<span>Outlook</span><span>·</span><span>Bring</span><span>·</span><span>Kundehistorikk</span>
					</div>
				</Beat>
			</div>
		</DemoSurface>
	);
}

function UnderstandDemo({ progress, staticFrame }: DemoProps) {
	const sources: Array<{ detail: string; name: BrandName }> = [
		{ detail: "kundedokumenter", name: "onedrive" },
		{ detail: "returpolicy", name: "notion" },
		{ detail: "produktmanual", name: "sharepoint" },
	];

	return (
		<DemoSurface label="Forstå kildene">
			<div className="relative h-[202px]">
				{/* Anchor — the question stays on. */}
				<div className="flex items-center gap-2 rounded-[13px] border border-[#171717]/[0.075] bg-white/90 px-3 py-2.5 shadow-[0_7px_17px_rgba(30,27,24,0.06)]">
					<Search className="size-3 text-[#171717]/64" />
					<span className="font-protokoll text-[9px] text-[#171717]/68">
						Hva har endret seg denne uken?
					</span>
				</div>

				<Beat
					className="absolute inset-x-0 top-[49px] space-y-1.5"
					progress={progress}
					staticFrame={staticFrame}
					track="setup"
				>
					{sources.map(({ detail, name }) => (
						<BrandChip detail={detail} key={name} name={name} />
					))}
				</Beat>

				<Beat
					className="absolute bottom-0 right-0 w-[88%] rounded-[15px] border border-velion-coral/35 bg-[#fffaf6] p-3 shadow-[0_12px_26px_rgba(46,34,28,0.10)]"
					progress={progress}
					staticFrame={staticFrame}
					track="result"
				>
					<div className="flex items-center justify-between font-protokoll text-[8px] uppercase tracking-[0.1em]">
						<span className="text-velion-coral-deep/85">Bekreftet innsikt</span>
						<span className="text-emerald-700">3 kilder</span>
					</div>
					<p className="m-0 mt-1.5 font-protokoll text-[9px] font-medium leading-[1.35] text-[#171717]/80">
						Ny leveringspolicy påvirker retursvaret.
					</p>
				</Beat>
			</div>
		</DemoSurface>
	);
}

function DelegateDemo({ progress, staticFrame }: DemoProps) {
	const destinations: BrandName[] = ["outlook", "gmail", "slack"];

	return (
		<DemoSurface label="Deleger med kontroll">
			<div className="relative h-[202px]">
				{/* Anchor — proposed action + waiting chip stay on. */}
				<div className="flex items-center justify-between">
					<div>
						<p className="m-0 font-protokoll text-[8px] uppercase tracking-[0.11em] text-[#171717]/68">
							Foreslått handling
						</p>
						<p className="m-0 mt-1 font-arbeit text-[16px] leading-none tracking-[-0.04em] text-[#171717]">
							Refusjon · 490 kr
						</p>
					</div>
					{/* Waiting-on-a-human is THE coral beat site-wide (ApprovalQueue
					    uses a coral pulse for "Venter på godkjenning") — so this chip
					    carries the accent, not a neutral grey. */}
					<span className="flex items-center gap-1 rounded-full border border-velion-coral/30 bg-velion-coral/10 px-2 py-1 font-protokoll text-[8px] font-medium text-velion-coral-deep">
						<Clock3 className="size-2.5" /> {staticFrame ? "Godkjent" : "Venter"}
					</span>
				</div>

				<Beat
					className="mt-3 flex gap-1.5"
					progress={progress}
					staticFrame={staticFrame}
					track="setup"
				>
					{destinations.map((name) => (
						<BrandLogo key={name} name={name} size={20} />
					))}
					<span className="self-center font-protokoll text-[8px] text-[#171717]/68">
						utkast klart
					</span>
				</Beat>

				{/* The approval card shares the setup beat (fully out before the
				    receipt's crossfade completes) and is hidden in the static
				    frame — it overlaps the receipt in space, so they may only
				    meet during the brief low-opacity crossfade, never at rest. */}
				{staticFrame ? null : (
					<Beat
						className="absolute inset-x-0 top-[92px] rounded-[14px] border border-[#171717]/[0.075] bg-white/95 p-3 shadow-[0_12px_25px_rgba(30,27,24,0.09)]"
						progress={progress}
						staticFrame={staticFrame}
						track="setupLate"
					>
						<div className="flex items-center gap-2 font-protokoll text-[9px] text-[#171717]/76">
							<ShieldCheck className="size-3.5 text-emerald-600" /> Policy 04 matcher
						</div>
						<div className="mt-2 flex gap-1.5 font-protokoll text-[8px]">
							<span className="rounded-[8px] bg-velion-coral px-3 py-1.5 font-medium text-white">Godkjenn</span>
							<span className="rounded-[8px] border border-[#171717]/10 px-3 py-1.5 text-[#171717]/70">Revider</span>
						</div>
					</Beat>
				)}

				<Beat
					className="absolute inset-x-0 bottom-0 rounded-[15px] border border-emerald-200 bg-[#f7fff9] p-3 shadow-[0_12px_24px_rgba(30,55,37,0.10)]"
					progress={progress}
					staticFrame={staticFrame}
					track="result"
				>
					<div className="flex items-center justify-between font-protokoll">
						<span className="flex items-center gap-1.5 text-[9px] font-medium text-emerald-800">
							<Check className="size-3.5" /> Godkjent og sendt
						</span>
						<span className="text-[7px] text-emerald-700">10:41</span>
					</div>
					<div className="mt-2 flex items-center justify-between border-t border-emerald-900/10 pt-2 font-protokoll text-[8px] text-[#171717]/68">
						<span>Godkjent av Ida · revisjon lagret</span>
						<span className="flex items-center gap-1 rounded-[7px] border border-[#171717]/[0.08] bg-white px-2 py-1">
							<RotateCcw className="size-2.5" /> Rull tilbake
						</span>
					</div>
				</Beat>
			</div>
		</DemoSurface>
	);
}

// Phase offsets spread the four cards' beat changes 2s apart so the row
// always has one card mid-story instead of four synchronized blinks.
const DEMO_PHASE_MS: Record<FeatureDemoKind, number> = {
	build: 0,
	connect: 2000,
	understand: 4000,
	delegate: 6000,
};

export function FeatureCardDemo({ demo }: { demo: FeatureDemoKind }) {
	const reducedMotion = useReducedMotion() ?? false;
	const rootRef = useRef<HTMLDivElement>(null);
	const [isActive, setIsActive] = useState(false);

	useEffect(() => {
		const element = rootRef.current;
		if (!element) return undefined;

		const desktopStage = element.closest<HTMLElement>("[data-feature-output-stage]");
		if (desktopStage) {
			const updateFromStage = () => {
				const style = window.getComputedStyle(desktopStage);
				setIsActive(style.visibility !== "hidden" && Number.parseFloat(style.opacity) > 0.5);
			};
			const observer = new MutationObserver(updateFromStage);
			observer.observe(desktopStage, { attributeFilter: ["style"], attributes: true });
			updateFromStage();
			return () => observer.disconnect();
		}

		const observer = new IntersectionObserver(
			([entry]) => setIsActive(entry.isIntersecting && entry.intersectionRatio >= 0.25),
			{ threshold: [0, 0.25] },
		);
		observer.observe(element);
		return () => observer.disconnect();
	}, []);

	const staticFrame = reducedMotion || !isActive;
	const progress = useVignetteProgress(!staticFrame, DEMO_PHASE_MS[demo]);

	let content = <DelegateDemo progress={progress} staticFrame={staticFrame} />;
	if (demo === "build") content = <BuildDemo progress={progress} staticFrame={staticFrame} />;
	if (demo === "connect") content = <ConnectDemo progress={progress} staticFrame={staticFrame} />;
	if (demo === "understand") content = <UnderstandDemo progress={progress} staticFrame={staticFrame} />;

	return <div ref={rootRef}>{content}</div>;
}
