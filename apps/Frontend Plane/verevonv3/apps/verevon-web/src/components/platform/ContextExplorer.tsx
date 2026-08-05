"use client";

import Image from "next/image";
import { motion, useReducedMotion } from "motion/react";
import { useId, useState } from "react";

type Source = {
	label: string;
	src: string;
};

type ContextView = {
	id: "samtalen" | "kildene" | "neste-steg";
	label: string;
	overline: string;
	title: string;
	description: string;
};

const sources: Source[] = [
	{ label: "Outlook", src: "/brand-logos/outlook.svg" },
	{ label: "Bring", src: "/brand-logos/bring.svg" },
	{ label: "SharePoint", src: "/brand-logos/sharepoint.svg" },
	{ label: "Notion", src: "/brand-logos/notion.svg" },
	{ label: "Slack", src: "/brand-logos/slack.svg" },
];

const contextViews: ContextView[] = [
	{
		id: "samtalen",
		label: "Samtalen",
		overline: "Kundens signal",
		title: "Det som ble sagt, følger saken.",
		description:
			"Verevon samler den pågående samtalen med kundens historikk, så neste person ikke starter fra null.",
	},
	{
		id: "kildene",
		label: "Kildene",
		overline: "Grunnlaget",
		title: "Kildene ligger sammen med svaret.",
		description:
			"I stedet for å lete etter dokumenter og oppdateringer, ser dere hvilke kilder som bærer den aktuelle saken.",
	},
	{
		id: "neste-steg",
		label: "Neste steg",
		overline: "Overleveringen",
		title: "Det som må skje videre, er tydelig.",
		description:
			"Når arbeidet flytter seg mellom mennesker eller systemer, følger konteksten med — og neste vurdering er synlig.",
	},
];

function SourceLogo({ source }: { source: Source }) {
	return (
		<span
			aria-label={source.label}
			className="grid size-9 shrink-0 place-items-center rounded-[11px] border border-verevon-j-text/8 bg-white p-2 shadow-[0_8px_18px_rgba(23,23,23,0.04)]"
			role="img"
			title={source.label}
		>
			<Image
				alt=""
				aria-hidden="true"
				className="max-h-full max-w-full object-contain"
				height={20}
				src={source.src}
				style={{ height: "auto", width: "auto" }}
				width={20}
			/>
		</span>
	);
}

function ConversationPanel() {
	return (
		<div className="grid gap-4">
			<div className="flex items-center justify-between gap-3 rounded-[16px] border border-verevon-j-text/8 bg-verevon-surface p-3.5">
				<div className="flex min-w-0 items-center gap-3">
					<SourceLogo source={sources[0]} />
					<div className="min-w-0">
						<p className="m-0 font-protokoll text-[0.67rem] font-medium uppercase tracking-[0.14em] text-verevon-j-text/42">
							E-post
						</p>
						<p className="m-0 mt-1 truncate font-arbeit text-[1rem] font-normal tracking-[-0.03em] text-verevon-j-text">
							Levering til Nora
						</p>
					</div>
				</div>
				<span className="shrink-0 font-protokoll text-[0.72rem] text-verevon-j-text/42">
					09:42
				</span>
			</div>

			<div className="rounded-[18px] border border-verevon-j-text/8 bg-verevon-surface-soft/72 p-4">
				<p className="m-0 max-w-[38ch] font-protokoll text-[0.92rem] font-light leading-[1.52] text-verevon-j-text/78">
					Hei, kan dere bekrefte hvor pakken min er? Jeg trenger den før torsdag.
				</p>
			</div>

			<div className="rounded-[18px] border border-verevon-j-text/8 bg-white p-4">
				<p className="m-0 font-protokoll text-[0.68rem] font-medium uppercase tracking-[0.14em] text-verevon-j-text/42">
					Dette følger saken
				</p>
				<ul className="m-0 mt-3 grid list-none gap-2 p-0 font-protokoll text-[0.85rem] font-light text-verevon-j-text/68">
					<li className="flex items-center justify-between gap-3">
						<span>Ordre #52481</span>
						<span className="text-emerald-700">Aktiv</span>
					</li>
					<li className="flex items-center justify-between gap-3 border-t border-verevon-j-text/8 pt-2">
						<span>Sist levert til Oslo</span>
						<span>12. juni</span>
					</li>
				</ul>
			</div>
		</div>
	);
}

function SourcesPanel() {
	return (
		<div className="grid gap-3">
			<div className="rounded-[18px] border border-verevon-j-text/8 bg-white p-4">
				<div className="flex items-center justify-between gap-4">
					<div>
						<p className="m-0 font-protokoll text-[0.68rem] font-medium uppercase tracking-[0.14em] text-verevon-j-text/42">
							Kilder i eksempelet
						</p>
						<p className="m-0 mt-1 font-arbeit text-[1.04rem] tracking-[-0.03em] text-verevon-j-text">
							Fire flater, én sak
						</p>
					</div>
					<span className="rounded-full bg-emerald-500/10 px-2.5 py-1 font-protokoll text-[0.7rem] font-medium text-emerald-800">
						Samlet
					</span>
				</div>
			</div>

			{sources.slice(0, 4).map((source, index) => (
				<div
					className="flex items-center gap-3 rounded-[16px] border border-verevon-j-text/8 bg-white p-3"
					key={source.label}
				>
					<SourceLogo source={source} />
					<div className="min-w-0 flex-1">
						<p className="m-0 font-arbeit text-[0.95rem] tracking-[-0.025em] text-verevon-j-text">
							{[
								"Kundens siste melding",
								"Sporingsstatus for sendingen",
								"Leveringsvilkår for kunden",
								"Teamets svarmal for forsinkelser",
							][index]}
						</p>
						<p className="m-0 mt-0.5 font-protokoll text-[0.72rem] font-light text-verevon-j-text/48">
							{source.label} · oppdatert nylig
						</p>
					</div>
				</div>
			))}
		</div>
	);
}

function NextStepPanel() {
	return (
		<div className="grid gap-4">
			<div className="rounded-[20px] border border-verevon-j-text/8 bg-verevon-j-text p-5 text-white shadow-[0_24px_58px_rgba(23,23,23,0.13)]">
				<div className="flex items-start justify-between gap-3">
					<div>
						<p className="m-0 font-protokoll text-[0.66rem] font-medium uppercase tracking-[0.15em] text-white/48">
							Neste overlevering
						</p>
						<p className="m-0 mt-2 font-arbeit text-[1.25rem] font-light leading-[1.06] tracking-[-0.04em]">
							Svarutkast er klart
						</p>
					</div>
					<span className="rounded-full bg-white/12 px-2.5 py-1 font-protokoll text-[0.68rem] text-white/76">
						Klar
					</span>
				</div>
				<p className="m-0 mt-4 max-w-[42ch] font-protokoll text-[0.88rem] font-light leading-[1.48] text-white/66">
					Bekreft oppdatert leveringsstatus med kunden. Kildene og den pågående samtalen følger med.
				</p>
			</div>

			<div className="rounded-[18px] border border-verevon-j-text/8 bg-white p-4">
				<div className="flex items-center justify-between gap-4">
					<p className="m-0 font-protokoll text-[0.82rem] font-light text-verevon-j-text/62">
						Neste vurdering
					</p>
					<span className="size-2 rounded-full bg-verevon-coral" />
				</div>
				<p className="m-0 mt-2 font-arbeit text-[1rem] tracking-[-0.03em] text-verevon-j-text">
						Trenger kunden en alternativ leveringsdato?
				</p>
			</div>

			<div className="grid grid-cols-2 gap-3">
				<div className="rounded-[16px] border border-verevon-j-text/8 bg-verevon-surface-soft/70 p-3">
					<p className="m-0 font-protokoll text-[0.66rem] uppercase tracking-[0.13em] text-verevon-j-text/42">
						Kilder
					</p>
					<p className="m-0 mt-1 font-arbeit text-[1rem] tracking-[-0.03em] text-verevon-j-text">4</p>
				</div>
				<div className="rounded-[16px] border border-verevon-j-text/8 bg-verevon-surface-soft/70 p-3">
					<p className="m-0 font-protokoll text-[0.66rem] uppercase tracking-[0.13em] text-verevon-j-text/42">
						Åpent punkt
					</p>
					<p className="m-0 mt-1 font-arbeit text-[1rem] tracking-[-0.03em] text-verevon-j-text">1</p>
				</div>
			</div>
		</div>
	);
}

function ActivePanel({ view }: { view: ContextView["id"] }) {
	if (view === "kildene") {
		return <SourcesPanel />;
	}

	if (view === "neste-steg") {
		return <NextStepPanel />;
	}

	return <ConversationPanel />;
}

export function ContextExplorer() {
	const [activeId, setActiveId] = useState<ContextView["id"]>("samtalen");
	const shouldReduceMotion = useReducedMotion();
	const tabsId = useId();
	const activeView = contextViews.find((view) => view.id === activeId) ?? contextViews[0];

	return (
		<section
			aria-labelledby="shared-context-example-title"
			className="border-b border-verevon-j-text/8 bg-verevon-surface-soft/38 px-[var(--verevon-edge)] py-[clamp(92px,12vw,168px)] max-[760px]:px-[var(--verevon-page-pad)]"
			id="eksempel"
		>
			<div className="mx-auto grid max-w-[1680px] items-start gap-x-[clamp(48px,8vw,150px)] gap-y-12 xl:grid-cols-[minmax(0,0.82fr)_minmax(520px,1.18fr)]">
				<div className="max-w-[560px] xl:sticky xl:top-[116px]">
					<p className="verevon-eyebrow">Et eksempel</p>
					<h2
						className="verevon-h2 mt-6 max-w-[12ch] text-balance"
						id="shared-context-example-title"
					>
						Én sak. Ikke fem faner.
					</h2>
					<p className="verevon-body-lg mt-7 max-w-[48ch] text-pretty">
						Se hvordan en kundesak kan gå fra en pågående samtale til et tydelig neste steg — uten at teamet må bygge opp konteksten på nytt.
					</p>

					<div className="mt-9 flex flex-wrap gap-2.5" aria-label="Kilder i eksempelet">
						{sources.map((source) => (
							<SourceLogo key={source.label} source={source} />
						))}
					</div>
				</div>

				<div className="overflow-hidden rounded-[30px] border border-verevon-j-text/10 bg-white shadow-[0_34px_110px_rgba(23,23,23,0.08)]">
					<div className="flex flex-wrap gap-1 border-b border-verevon-j-text/8 bg-white px-3 pt-3 sm:px-5 sm:pt-5" role="tablist">
						{contextViews.map((view, index) => {
							const isActive = activeId === view.id;

							return (
								<button
									aria-controls={`${tabsId}-${view.id}`}
									aria-selected={isActive}
									className={[
										"relative min-h-11 rounded-t-[12px] px-3.5 font-protokoll text-[0.77rem] font-medium uppercase tracking-[0.08em] transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-verevon-coral focus-visible:ring-offset-2 sm:px-5",
										isActive
											? "bg-verevon-surface-soft text-verevon-j-text"
											: "text-verevon-j-text/45 hover:text-verevon-j-text/78",
									].join(" ")}
									id={`${tabsId}-${view.id}-tab`}
									key={view.id}
									onClick={() => setActiveId(view.id)}
									onKeyDown={(event) => {
										const keyToIndex: Record<string, number> = {
											ArrowRight: (index + 1) % contextViews.length,
											ArrowLeft:
												(index - 1 + contextViews.length) % contextViews.length,
											Home: 0,
											End: contextViews.length - 1,
										};
										const nextIndex = keyToIndex[event.key];

										if (nextIndex === undefined) {
											return;
										}

										event.preventDefault();
										const nextView = contextViews[nextIndex];
										setActiveId(nextView.id);
										window.requestAnimationFrame(() => {
											document.getElementById(`${tabsId}-${nextView.id}-tab`)?.focus();
										});
									}}
									role="tab"
									tabIndex={isActive ? 0 : -1}
									type="button"
								>
									{view.label}
								</button>
							);
						})}
					</div>

					<div className="grid gap-7 bg-verevon-surface-soft/58 p-[clamp(20px,3.2vw,42px)] lg:grid-cols-[minmax(0,0.78fr)_minmax(280px,1.1fr)]">
						<div className="flex min-h-[300px] flex-col justify-between">
							<div>
								<p className="verevon-eyebrow">{activeView.overline}</p>
								<h3 className="mt-5 max-w-[14ch] font-arbeit text-[clamp(1.7rem,2.1vw,2.65rem)] font-light leading-[0.98] tracking-[-0.055em] text-verevon-j-text text-balance">
									{activeView.title}
								</h3>
							</div>
							<p className="m-0 max-w-[40ch] font-protokoll text-[0.95rem] font-light leading-[1.55] text-verevon-j-text/62 text-pretty">
								{activeView.description}
							</p>
						</div>

						<div
							aria-labelledby={`${tabsId}-${activeView.id}-tab`}
							className="min-h-[300px]"
							id={`${tabsId}-${activeView.id}`}
							role="tabpanel"
						>
							<motion.div
								animate={{ opacity: 1, y: 0 }}
								initial={shouldReduceMotion ? false : { opacity: 0, y: 12 }}
								key={activeView.id}
								transition={{
									duration: shouldReduceMotion ? 0 : 0.28,
									ease: [0.22, 1, 0.36, 1],
								}}
							>
								<ActivePanel view={activeView.id} />
							</motion.div>
						</div>
					</div>
				</div>
			</div>
		</section>
	);
}

export default ContextExplorer;
