"use client";

import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import {
	Activity,
	Check,
	Database,
	FileText,
	Inbox,
	Search,
	Shield,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";

const promptLines = [
	"Finn dagens uløste kundesaker.",
	"Sjekk virksomheten i Enhetsregisteret.",
	"Hent relevante kilder før svaret skrives.",
	"Se om noe viktig har endret seg.",
	"Lag et kort svarforslag for teamet.",
	"Marker hva som krever godkjenning.",
	"Stopp før noe sendes til kunden.",
	"Lagre kilder, region og godkjenning i sporet.",
];

type LoopSourceId =
	| "signal"
	| "brreg"
	| "knowledge"
	| "monitor"
	| "brief"
	| "approval"
	| "audit";

type LoopIcon = typeof Search;

type LoopSource = {
	detail: string;
	icon: LoopIcon;
	id: LoopSourceId;
	label: string;
	x: string;
	y: string;
};

type LoopStep = {
	accent: string;
	action: string;
	actionDetail: string;
	label: string;
	metric: string;
	prompt: string;
	result: string;
	resultDetail: string;
	sourceIds: LoopSourceId[];
	status: string;
	title: string;
};

const loopSources: LoopSource[] = [
	{
		id: "signal",
		label: "Kundesignal",
		detail: "Innboks",
		icon: Inbox,
		x: "8%",
		y: "18%",
	},
	{
		id: "brreg",
		label: "Enhetsregisteret",
		detail: "983 515 827",
		icon: Database,
		x: "6%",
		y: "58%",
	},
	{
		id: "knowledge",
		label: "Kunnskap",
		detail: "9 kilder",
		icon: Search,
		x: "71%",
		y: "11%",
	},
	{
		id: "monitor",
		label: "Endring",
		detail: "Endring funnet",
		icon: Activity,
		x: "78%",
		y: "48%",
	},
	{
		id: "brief",
		label: "Svarutkast",
		detail: "Teamklar",
		icon: FileText,
		x: "72%",
		y: "76%",
	},
	{
		id: "approval",
		label: "Godkjenning",
		detail: "Krever ja",
		icon: Shield,
		x: "63%",
		y: "78%",
	},
	{
		id: "audit",
		label: "Spor",
		detail: "EU/ZDR",
		icon: FileText,
		x: "22%",
		y: "80%",
	},
];

const loopSteps: LoopStep[] = [
	{
		label: "01 / Handling",
		title: "Finn sakene",
		prompt: promptLines[0],
		action: "conversation.lookup",
		actionDetail: "Verevon leser dagens kø og filtrerer saker uten avklart neste steg.",
		status: "kjører",
		metric: "3 åpne saker",
		result: "3 saker funnet",
		resultDetail: "1 sak har tydelig kundeintensjon og mangler et svarutkast.",
		accent: "#2f6f9f",
		sourceIds: ["signal"],
	},
	{
		label: "02 / Handling",
		title: "Verifiser selskapet",
		prompt: promptLines[1],
		action: "company_lookup.enhetsregisteret",
		actionDetail: "Organisasjonsnummer, navn og status kontrolleres før svaret bygges.",
		status: "oppslag",
		metric: "Org 983 515 827",
		result: "Virksomhet bekreftet",
		resultDetail: "AQUATIQ AS er aktiv, og saken kan knyttes til riktig kunde.",
		accent: "#397f73",
		sourceIds: ["brreg"],
	},
	{
		label: "03 / Handling",
		title: "Hent kildene",
		prompt: promptLines[2],
		action: "knowledge_search.graph",
		actionDetail: "Verevon søker i dokumenter, tidligere svar og policy før tekst skrives.",
		status: "søker",
		metric: "GraphRAG + søk",
		result: "9 kilder rangert",
		resultDetail: "Tre kilder blir brukt som belegg: avtale, policy og siste kundesvar.",
		accent: "#6f74b7",
		sourceIds: ["knowledge"],
	},
	{
		label: "04 / Handling",
		title: "Sjekk endringer",
		prompt: promptLines[3],
		action: "monitoring.diff",
		actionDetail: "Ny ekstern tekst sammenlignes med sist lagrede versjon av grunnlaget.",
		status: "sammenligner",
		metric: "2 avsnitt endret",
		result: "Ny frist oppdaget",
		resultDetail: "Leveransefristen er flyttet, og svaret må bruke oppdatert dato.",
		accent: "#497ea6",
		sourceIds: ["monitor"],
	},
	{
		label: "05 / Handling",
		title: "Skriv forslag",
		prompt: promptLines[4],
		action: "draft_reply.create",
		actionDetail: "Verevon lager et kort svar med kildebelegg, usikkerhet og neste steg.",
		status: "skriver",
		metric: "Lav risiko",
		result: "Svarutkast klart",
		resultDetail: "Teamet får en kort, kildebelagt tekst som kan justeres manuelt.",
		accent: "#c2764c",
		sourceIds: ["brief"],
	},
	{
		label: "06 / Handling",
		title: "Finn grensen",
		prompt: promptLines[5],
		action: "risk_policy.classify",
		actionDetail: "Verevon vurderer om handlingen kan utføres, eller bare foreslås.",
		status: "klassifiserer",
		metric: "Krever ja",
		result: "Publisering krever godkjenning",
		resultDetail: "Utkastet kan ikke sendes til kunden uten et menneskelig ja.",
		accent: "#a05c62",
		sourceIds: ["approval"],
	},
	{
		label: "07 / Handling",
		title: "Stopp før utsending",
		prompt: promptLines[6],
		action: "approval.request",
		actionDetail: "Godkjenning sendes til riktig team uten å utføre kundevendt handling.",
		status: "venter",
		metric: "Menneske i løkken",
		result: "Ligger i inbox",
		resultDetail: "Teamet kan godkjenne, avvise eller justere før kunden ser noe.",
		accent: "#bf6d45",
		sourceIds: ["approval"],
	},
	{
		label: "08 / Handling",
		title: "Lagre sporet",
		prompt: promptLines[7],
		action: "audit.tool_action",
		actionDetail: "Kilder, region, ZDR-status og godkjenning skrives til saken.",
		status: "logger",
		metric: "Sweden Central",
		result: "Revisjonsspor skrevet",
		resultDetail: "Kilde, region og menneskelig godkjenning er etterprøvbart senere.",
		accent: "#4b6f8f",
		sourceIds: ["audit"],
	},
];

const loopPaths: { d: string; id: LoopSourceId }[] = [
	{ id: "signal", d: "M175 185 C280 250 328 262 382 302" },
	{ id: "brreg", d: "M182 486 C270 446 320 424 382 389" },
	{ id: "knowledge", d: "M586 165 C506 236 462 276 418 315" },
	{ id: "monitor", d: "M620 428 C552 420 486 404 428 382" },
	{ id: "brief", d: "M566 600 C514 518 462 468 424 414" },
	{ id: "approval", d: "M526 612 C480 524 452 468 422 418" },
	{ id: "audit", d: "M244 622 C302 524 342 474 388 426" },
];

type PromptFeedState = {
	activeIndex: number;
	activeText: string;
	completed: string[];
	isTyping: boolean;
};

const initialPromptFeed: PromptFeedState = {
	activeIndex: 0,
	activeText: "",
	completed: [],
	isTyping: true,
};

function VerevonPromptLoop({ activeIndex }: { activeIndex: number }) {
	const shouldReduceMotion = useReducedMotion();
	const stepIndex = Math.min(
		Math.max(activeIndex, 0),
		loopSteps.length - 1,
	);
	const step = loopSteps[stepIndex];
	const activeSourceIds = new Set(step.sourceIds);

	return (
		<div
			aria-hidden="true"
			className="relative aspect-square w-full max-w-[760px] overflow-hidden bg-[#dfeaf4] text-verevon-j-text"
			data-promt-loop-surface
		>
			<motion.div
				className="absolute inset-0 bg-[linear-gradient(90deg,rgba(39,86,126,0.08)_1px,transparent_1px),linear-gradient(180deg,rgba(39,86,126,0.07)_1px,transparent_1px),linear-gradient(145deg,rgba(248,252,254,0.94)_0%,rgba(223,234,244,0.7)_44%,rgba(179,205,226,0.76)_100%)] bg-[length:52px_52px,52px_52px,100%_100%]"
				animate={
					shouldReduceMotion
						? undefined
						: { backgroundPosition: ["0px 0px", "52px 52px"] }
				}
				transition={{
					duration: 18,
					ease: "linear",
					repeat: Infinity,
				}}
			/>

			<motion.div
				className="absolute inset-[10%] border border-verevon-j-text/8"
				animate={
					shouldReduceMotion
						? undefined
						: {
								opacity: [0.18, 0.34, 0.18],
								scale: [0.98, 1, 0.98],
							}
				}
				transition={{
					duration: 5.8,
					ease: "easeInOut",
					repeat: Infinity,
				}}
			/>

			{loopSources.map((source) => {
				const Icon = source.icon;
				const isActive = activeSourceIds.has(source.id);

				return (
					<motion.div
						animate={
							shouldReduceMotion
								? {
										opacity: isActive ? 1 : 0.24,
										scale: isActive ? 1.03 : 1,
									}
								: {
										opacity: isActive ? 1 : 0.2,
										scale: isActive ? [1, 1.045, 1] : 1,
										y: isActive ? [0, -3, 0] : 0,
									}
						}
						className="absolute z-[3] min-w-[132px] border border-verevon-j-text/10 bg-white/52 px-3 py-2 shadow-[0_16px_40px_rgba(63,96,125,0.12)] backdrop-blur-md"
						key={source.id}
						style={{
							left: source.x,
							top: source.y,
						}}
						transition={
							isActive && !shouldReduceMotion
								? {
										duration: 1.55,
										ease: "easeInOut",
										repeat: Infinity,
									}
								: { duration: 0.42, ease: "easeOut" }
						}
					>
						<div className="flex items-center gap-2">
							<span className="grid size-7 place-items-center rounded-[6px] bg-verevon-j-text/7 text-verevon-j-text/62">
								<Icon aria-hidden="true" className="size-3.5" />
							</span>
							<span className="min-w-0">
								<span className="block font-protokoll text-[0.68rem] font-medium uppercase leading-none tracking-[0.13em] text-verevon-j-text/52">
									{source.label}
								</span>
								<span className="mt-1 block truncate font-protokoll text-[0.76rem] font-light leading-none text-verevon-j-text/76">
									{source.detail}
								</span>
							</span>
						</div>
					</motion.div>
				);
			})}

			<svg
				className="absolute inset-0 z-[2] h-full w-full"
				fill="none"
				viewBox="0 0 760 760"
			>
				{loopPaths.map((path, index) => {
					const isPathActive = activeSourceIds.has(path.id);

					return (
						<motion.path
							animate={
								shouldReduceMotion
									? {
											opacity: isPathActive ? 0.42 : 0.12,
											pathLength: 1,
										}
									: {
											opacity: isPathActive ? [0.2, 0.62, 0.2] : 0.11,
											pathLength: isPathActive ? [0.08, 1, 1] : 0.32,
										}
							}
							d={path.d}
							key={path.id}
							stroke="rgba(39,86,126,0.44)"
							strokeDasharray="4 8"
							strokeLinecap="round"
							strokeWidth="1.2"
							transition={{
								delay: index * 0.08,
								duration: 2.2,
								ease: "easeInOut",
								repeat: shouldReduceMotion ? 0 : Infinity,
								repeatDelay: 1.2,
							}}
						/>
					);
				})}
			</svg>

			<div className="absolute inset-x-[13%] top-[15%] z-[4] border border-verevon-j-text/10 bg-white/70 shadow-[0_24px_70px_rgba(66,92,118,0.18)] backdrop-blur-xl">
				<div className="flex items-center justify-between border-b border-verevon-j-text/8 px-4 py-3">
					<div className="flex items-center gap-2">
						<span className="size-2 rounded-full bg-[#c2764c]" />
						<span className="size-2 rounded-full bg-[#d8bc65]" />
						<span className="size-2 rounded-full bg-[#6f9f8d]" />
					</div>
					<span className="font-protokoll text-[0.62rem] font-medium uppercase leading-none tracking-[0.18em] text-verevon-j-text/42">
						Egen handling
					</span>
				</div>

				<div className="p-[clamp(16px,2.4vw,26px)]">
					<AnimatePresence mode="wait">
						<motion.div
							animate={{ opacity: 1, y: 0 }}
							exit={{ opacity: 0, y: -12 }}
							initial={{ opacity: 0, y: 14 }}
							key={step.label}
							transition={{
								duration: shouldReduceMotion ? 0 : 0.42,
								ease: [0.16, 1, 0.3, 1],
							}}
						>
							<div className="flex items-start justify-between gap-5">
								<div>
									<p className="m-0 font-protokoll text-[0.68rem] font-medium uppercase leading-none tracking-[0.18em] text-verevon-j-text/42">
										{step.label}
									</p>
									<h3 className="m-0 mt-3 max-w-[430px] font-arbeit text-[clamp(1.7rem,2.6vw,2.65rem)] font-light leading-[0.98] tracking-[-0.05em] text-verevon-j-text">
										{step.title}
									</h3>
								</div>
								<span
									className="mt-1 inline-flex shrink-0 items-center gap-2 border border-verevon-j-text/10 bg-white/62 px-2.5 py-2 font-protokoll text-[0.62rem] font-medium uppercase leading-none tracking-[0.14em] text-verevon-j-text/58"
									style={{ color: step.accent }}
								>
									<span
										className="size-1.5 rounded-full"
										style={{ backgroundColor: step.accent }}
									/>
									{step.metric}
								</span>
							</div>

							<div className="mt-5 border border-verevon-j-text/8 bg-white/48 p-3.5">
								<p className="m-0 font-protokoll text-[0.62rem] font-medium uppercase leading-none tracking-[0.16em] text-verevon-j-text/38">
									Prompt
								</p>
								<p className="m-0 mt-2 font-protokoll text-[clamp(0.94rem,1vw,1.08rem)] font-light leading-[1.38] text-verevon-j-text/74">
									{step.prompt}
								</p>
							</div>

							<div className="mt-3 grid grid-cols-[minmax(0,0.96fr)_minmax(0,1.04fr)] gap-3 max-[620px]:grid-cols-1">
								<div className="relative overflow-hidden border border-verevon-j-text/10 bg-white/52 p-3.5">
									<motion.span
										aria-hidden="true"
										className="absolute inset-x-0 top-0 h-0.5 origin-left"
										initial={{ scaleX: 0 }}
										animate={{
											scaleX: shouldReduceMotion ? 1 : [0, 1, 1],
										}}
										style={{ backgroundColor: step.accent }}
										transition={{
											duration: shouldReduceMotion ? 0 : 1.35,
											ease: "easeInOut",
											times: [0, 0.72, 1],
										}}
									/>
									<div className="relative flex items-center justify-between gap-3">
										<p className="m-0 font-protokoll text-[0.62rem] font-medium uppercase leading-none tracking-[0.16em] text-verevon-j-text/38">
											Handling
										</p>
										<span
											className="inline-flex items-center gap-1.5 font-protokoll text-[0.58rem] font-medium uppercase leading-none tracking-[0.14em]"
											style={{ color: step.accent }}
										>
											<span
												className="size-1.5 rounded-full"
												style={{ backgroundColor: step.accent }}
											/>
											{step.status}
										</span>
									</div>
									<p className="relative m-0 mt-3 truncate font-protokoll text-[0.86rem] font-medium leading-none text-verevon-j-text/76">
										{step.action}
									</p>
									<p className="relative m-0 mt-2 font-protokoll text-[0.78rem] font-light leading-[1.4] text-verevon-j-text/54">
										{step.actionDetail}
									</p>
								</div>

								<motion.div
									animate={{ opacity: 1, y: 0 }}
									className="border border-verevon-j-text/10 bg-[#f7fbfd]/72 p-3.5"
									initial={{ opacity: 0, y: 8 }}
									transition={{
										delay: shouldReduceMotion ? 0 : 0.16,
										duration: shouldReduceMotion ? 0 : 0.34,
										ease: "easeOut",
									}}
								>
									<div className="flex items-center justify-between gap-3">
										<p className="m-0 font-protokoll text-[0.62rem] font-medium uppercase leading-none tracking-[0.16em] text-verevon-j-text/38">
											Resultat
										</p>
										<span className="grid size-6 place-items-center rounded-[6px] bg-verevon-j-text text-white">
											<Check aria-hidden="true" className="size-3.5" />
										</span>
									</div>
									<p className="m-0 mt-3 font-protokoll text-[0.94rem] font-medium leading-none text-verevon-j-text/78">
										{step.result}
									</p>
									<p className="m-0 mt-2 font-protokoll text-[0.78rem] font-light leading-[1.4] text-verevon-j-text/54">
										{step.resultDetail}
									</p>
								</motion.div>
							</div>
						</motion.div>
					</AnimatePresence>
				</div>
			</div>

			<div className="pointer-events-none absolute inset-0 z-[6] bg-[linear-gradient(90deg,#dfeaf4_0%,rgba(223,234,244,0)_12%,rgba(223,234,244,0)_88%,#dfeaf4_100%),linear-gradient(180deg,#dfeaf4_0%,rgba(223,234,244,0)_12%,rgba(223,234,244,0)_88%,#dfeaf4_100%)]" />
		</div>
	);
}

export function PromptSection() {
	const completedCountRef = useRef(0);
	const promptWindowRef = useRef<HTMLDivElement | null>(null);
	const sectionRef = useRef<HTMLElement | null>(null);
	const shouldReduceMotion = useReducedMotion();
	const [promptFeed, setPromptFeed] =
		useState<PromptFeedState>(initialPromptFeed);

	useEffect(() => {
		const promptWindow = promptWindowRef.current;
		const section = sectionRef.current;

		if (!promptWindow || !section) {
			return;
		}

		if (shouldReduceMotion) {
			const frame = window.requestAnimationFrame(() => {
				setPromptFeed({
					activeIndex: promptLines.length,
					activeText: "",
					completed: promptLines,
					isTyping: false,
				});
			});

			return () => window.cancelAnimationFrame(frame);
		}

		let active = false;
		let disposed = false;
		let timeout: number | undefined;

		const observer = new IntersectionObserver(
			(entries) => {
				const [entry] = entries;
				active = Boolean(entry?.isIntersecting);
			},
			{ threshold: 0.34 },
		);

		observer.observe(section);

		const clearTimer = () => {
			if (timeout !== undefined) {
				window.clearTimeout(timeout);
				timeout = undefined;
			}
		};

		const schedule = (callback: () => void, delay: number) => {
			clearTimer();
			timeout = window.setTimeout(callback, delay);
		};

		const waitUntilVisible = (callback: () => void) => {
			if (disposed) {
				return;
			}

			if (active) {
				callback();
				return;
			}

			schedule(() => waitUntilVisible(callback), 180);
		};

		const resetFeed = () => {
			setPromptFeed(initialPromptFeed);
			promptWindow.scrollTop = 0;
		};

		const typeLine = (lineIndex: number, charIndex: number) => {
			waitUntilVisible(() => {
				if (disposed) {
					return;
				}

				const line = promptLines[lineIndex];

				if (!line) {
					schedule(() => {
						resetFeed();
						typeLine(0, 0);
					}, 1250);
					return;
				}

				if (charIndex <= line.length) {
					setPromptFeed((current) => ({
						...current,
						activeIndex: lineIndex,
						activeText: line.slice(0, charIndex),
						isTyping: true,
					}));

					schedule(
						() => typeLine(lineIndex, charIndex + 1),
						charIndex === 0 ? 240 : 24,
					);
					return;
				}

				schedule(() => {
					setPromptFeed((current) => ({
						activeIndex: lineIndex + 1,
						activeText: "",
						completed: [...current.completed, line],
						isTyping: false,
					}));

					schedule(() => typeLine(lineIndex + 1, 0), 260);
				}, 560);
			});
		};

		typeLine(0, 0);

		return () => {
			disposed = true;
			observer.disconnect();
			clearTimer();
		};
	}, [shouldReduceMotion]);

	useEffect(() => {
		const promptWindow = promptWindowRef.current;

		if (!promptWindow) {
			return;
		}

		const frame = window.requestAnimationFrame(() => {
			const behavior =
				shouldReduceMotion ||
				completedCountRef.current === promptFeed.completed.length
					? "auto"
					: "smooth";

			completedCountRef.current = promptFeed.completed.length;

			promptWindow.scrollTo({
				behavior,
				top: promptWindow.scrollHeight,
			});
		});

		return () => window.cancelAnimationFrame(frame);
	}, [promptFeed.activeText, promptFeed.completed, shouldReduceMotion]);

	return (
		<section
			aria-label="Prompt til Verevon"
			className="promt-section relative isolate min-h-[100svh] overflow-hidden bg-[#dfeaf4] text-verevon-j-text"
			data-promt-section
			id="promt-section"
			ref={sectionRef}
		>
			<div
				aria-hidden="true"
				className="absolute inset-0 z-0 bg-[linear-gradient(90deg,rgba(23,58,105,0.055)_1px,transparent_1px),linear-gradient(180deg,rgba(23,58,105,0.045)_1px,transparent_1px),linear-gradient(155deg,#f3f9fc_0%,#dfeaf4_38%,#bdd6eb_100%)] bg-[length:96px_96px,96px_96px,100%_100%]"
			/>

			<div
				aria-hidden="true"
				className="absolute inset-0 z-[1] bg-[linear-gradient(180deg,rgba(255,255,255,0.42)_0%,rgba(223,234,244,0)_44%),linear-gradient(90deg,rgba(236,245,250,0.98)_0%,rgba(226,239,248,0.86)_30%,rgba(208,226,241,0.28)_62%,rgba(190,213,232,0.68)_100%)]"
			/>

			<div className="relative z-[2] mx-auto grid min-h-[100svh] w-full max-w-[1760px] grid-cols-[minmax(0,0.78fr)_minmax(420px,1fr)] items-center gap-[clamp(42px,5.6vw,112px)] px-[clamp(56px,5.55vw,208px)] py-[clamp(84px,9vh,136px)] max-[1023px]:grid-cols-1 max-[1023px]:px-[clamp(24px,4vw,56px)]">
				<div className="min-w-0">
					<p
						className="fade-out-top m-0 font-protokoll text-[0.72rem] font-medium uppercase leading-none tracking-[0.32em] text-verevon-j-text/38"
						data-fade-out-top
					>
						Skriv målet
					</p>

					<h2
						className="fade-out-top m-0 mt-5 max-w-[720px] font-arbeit text-[clamp(3rem,5vw,7.15rem)] font-light leading-[0.92] tracking-[-0.072em] text-verevon-j-text"
						data-fade-out-top
					>
						Verevon bygger arbeidet.
					</h2>

					<p
						className="fade-out-top m-0 mt-[clamp(24px,2.6vw,38px)] max-w-[640px] font-protokoll text-[clamp(1.02rem,1vw,1.18rem)] font-light leading-[1.5] text-verevon-text-muted"
						data-fade-out-top
					>
						Skriv hva som skal løses. Verevon finner kilder, lager forslag og
						viser hva som må godkjennes før noe skjer ute hos kunden.
					</p>

					<div className="fade-out-top" data-fade-out-top>
						<div
							className="relative mt-[clamp(30px,3.6vw,54px)] h-[min(42svh,430px)] min-h-[300px] overflow-hidden [mask-image:linear-gradient(transparent,#000_14%,#000_86%,transparent)] [-webkit-mask-image:linear-gradient(transparent,#000_14%,#000_86%,transparent)] max-[1023px]:h-[340px] max-[1023px]:min-h-[300px]"
							ref={promptWindowRef}
						>
							<div
								aria-hidden="true"
								className="flex min-h-full flex-col justify-end gap-3 pb-[clamp(92px,11vh,132px)] pt-[clamp(64px,7vh,96px)]"
								data-promt-track
							>
								{promptFeed.completed.map((line, index) => (
									<p
										className="m-0 grid grid-cols-[42px_minmax(0,1fr)] gap-4 border-t border-verevon-j-text/10 pt-4 font-protokoll text-[clamp(1.06rem,1.18vw,1.42rem)] font-light leading-[1.38] text-verevon-j-text"
										data-promt-line
										key={`${line}-${index}`}
									>
										<span className="pt-[0.18em] text-[0.68em] leading-none tracking-[0.18em] text-verevon-j-text/32">
											{String(index + 1).padStart(2, "0")}
										</span>
										<span>{line}</span>
									</p>
								))}

								{promptFeed.activeText ||
								promptFeed.isTyping ? (
									<p
										className="m-0 grid grid-cols-[42px_minmax(0,1fr)] gap-4 border-t border-verevon-j-text/10 pt-4 font-protokoll text-[clamp(1.06rem,1.18vw,1.42rem)] font-light leading-[1.38] text-verevon-j-text"
										data-promt-line
									>
										<span className="pt-[0.18em] text-[0.68em] leading-none tracking-[0.18em] text-verevon-j-text/32">
											{String(
												promptFeed.activeIndex + 1,
											).padStart(2, "0")}
										</span>
										<span>
											{promptFeed.activeText}
											<span
												aria-hidden="true"
												className="ml-1 inline-block translate-y-[-0.04em] animate-pulse text-verevon-coral"
											>
												_
											</span>
										</span>
									</p>
								) : null}
							</div>
						</div>
					</div>

					<ul className="sr-only">
						{promptLines.map((line) => (
							<li key={line}>{line}</li>
						))}
					</ul>

					<p
						className="fade-out-top m-0 mt-5 font-protokoll text-[clamp(0.92rem,0.92vw,1.02rem)] font-light leading-none text-verevon-j-text/52"
						data-fade-out-top
					>
						<span>Åpen instruks.</span>{" "}
						<span className="text-verevon-j-text/70">
							Åpent resultat-_
						</span>{" "}
						<span
							data-promt-glitch-noise
							className="animate-pulse text-verevon-coral/80"
						>
							{"}?!<\\--_/&"}
						</span>
					</p>
				</div>

				<div className="relative flex min-h-[min(760px,78svh)] min-w-0 items-center justify-center max-[1023px]:min-h-[min(620px,72svh)]">
					<VerevonPromptLoop activeIndex={promptFeed.activeIndex} />
					<p className="sr-only">
						En animert Verevon-loop viser hver prompt som en egen handling med
						et eget resultat, fra kundesak og kildesøk til godkjenning og
						revisjonsspor.
					</p>
				</div>
			</div>
		</section>
	);
}

export default PromptSection;
