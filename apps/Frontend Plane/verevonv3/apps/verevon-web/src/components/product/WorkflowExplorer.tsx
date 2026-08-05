"use client";

import Image from "next/image";
import { type KeyboardEvent, useId, useState } from "react";
import {
	ArrowRight,
	Check,
	CircleDotDashed,
	FileText,
	ShieldCheck,
	Undo2,
} from "lucide-react";

type WorkflowStage = {
	body: string;
	detail: string;
	id: string;
	image: string;
	label: string;
	mediaAlt: string;
	meta: string;
	title: string;
};

const workflowStages: WorkflowStage[] = [
	{
		id: "context",
		label: "Kontekst",
		title: "Samle det som faktisk gjelder.",
		body: "Et signal får historikk, kunnskap og relevant virksomhetskontekst før noen trenger å lete på nytt.",
		detail: "Kundens sak, sporingen og gjeldende retningslinje følger samme arbeidsflate.",
		meta: "Tre kilder funnet",
		image: "/verevon-product-shots/dashboard-live-overview.png",
		mediaAlt:
			"Verevon-oversikten med teamets daglige arbeid og innganger til samtaler, kilder og handlinger.",
	},
	{
		id: "draft",
		label: "Utkast",
		title: "Gjør grunnlaget om til et forslag.",
		body: "Verevon forbereder et utkast med det den har funnet, slik at teamet kan se både svaret og hvorfor det ble foreslått.",
		detail: "Et utkast er et forslag — ikke en handling som skjer av seg selv.",
		meta: "Kilder vedlagt utkastet",
		image: "/verevon-product-shots/chat-draft-sources-focus.jpg",
		mediaAlt:
			"Verevon-chat med et svarutkast og synlige retningslinjekilder som ligger til grunn.",
	},
	{
		id: "approval",
		label: "Godkjenning",
		title: "La riktig person bestemme.",
		body: "Når en oppgave trenger vurdering, er forslag, kilde og konsekvens samlet før noen godkjenner eller reviderer.",
		detail: "Mennesket kan endre, stoppe eller gjennomføre den samme oppgaven manuelt.",
		meta: "Venter på godkjenning",
		image: "/verevon-product-shots/chat-draft-answer-expanded.png",
		mediaAlt:
			"Verevon-chat med et utkast til svar og en utvidet produktflate for vurdering.",
	},
	{
		id: "revision",
		label: "Revider",
		title: "Forbedre regelen, ikke bare svaret.",
		body: "Når arbeidet må endres, kan teamet oppdatere kilden eller regelen bak. Neste runde starter med det nye grunnlaget.",
		detail: "Det som endres er synlig i etterkant, sammen med beslutningen som ble tatt.",
		meta: "Endring gjort sporbar",
		image: "/verevon-product-shots/chat-agent-steps.png",
		mediaAlt:
			"Verevon-chat med synlige agentsteg som viser hvordan arbeidet er bygget opp.",
	},
];

const stageIcons = [CircleDotDashed, FileText, ShieldCheck, Undo2];

export function WorkflowExplorer() {
	const [activeStageId, setActiveStageId] = useState(workflowStages[0].id);
	const tabGroupId = useId();
	const activeStageIndex = workflowStages.findIndex(
		(stage) => stage.id === activeStageId,
	);
	const activeStage = workflowStages[activeStageIndex];
	const ActiveIcon = stageIcons[activeStageIndex];
	const panelId = `workflow-stage-panel-${tabGroupId}`;
	const activeTabId = `workflow-stage-tab-${tabGroupId}-${activeStage.id}`;

	const handleTabKeyDown = (
		event: KeyboardEvent<HTMLButtonElement>,
		currentIndex: number,
	) => {
		const keyToIndex: Record<string, number> = {
			ArrowDown: (currentIndex + 1) % workflowStages.length,
			ArrowLeft:
				(currentIndex - 1 + workflowStages.length) % workflowStages.length,
			ArrowRight: (currentIndex + 1) % workflowStages.length,
			ArrowUp:
				(currentIndex - 1 + workflowStages.length) % workflowStages.length,
			End: workflowStages.length - 1,
			Home: 0,
		};
		const nextIndex = keyToIndex[event.key];

		if (nextIndex === undefined) {
			return;
		}

		event.preventDefault();
		const nextStage = workflowStages[nextIndex];
		setActiveStageId(nextStage.id);

		requestAnimationFrame(() => {
			document
				.getElementById(`workflow-stage-tab-${tabGroupId}-${nextStage.id}`)
				?.focus();
		});
	};

	return (
		<section
			aria-label="Arbeidsløkken i Verevon"
			className="border-b border-verevon-j-text/8 px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]"
			id="arbeidsflyt"
		>
			<div className="mx-auto max-w-[1680px]">
				<div
					aria-label="Steg i Verevons arbeidsflyt"
					className="grid gap-8 border-y border-verevon-j-text/10 py-5 lg:grid-cols-4 lg:gap-0"
					role="tablist"
				>
					{workflowStages.map((stage, index) => {
						const isActive = stage.id === activeStageId;
						const tabId = `workflow-stage-tab-${tabGroupId}-${stage.id}`;

						return (
							<button
								aria-controls={panelId}
								aria-selected={isActive}
								className={[
									"group grid grid-cols-[auto_1fr] items-start gap-3 rounded-sm text-left transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-verevon-j-text lg:border-l lg:px-7 lg:first:border-l-0 lg:first:pl-0 lg:last:pr-0",
									isActive
										? "text-verevon-j-text"
										: "text-verevon-j-text/72 hover:text-verevon-j-text",
								].join(" ")}
								id={tabId}
								key={stage.id}
								onClick={() => setActiveStageId(stage.id)}
								onKeyDown={(event) => handleTabKeyDown(event, index)}
								role="tab"
								tabIndex={isActive ? 0 : -1}
								type="button"
							>
								<span
									className={[
										"grid size-8 shrink-0 place-items-center rounded-full border text-[0.7rem] transition-colors",
									isActive
										? "border-verevon-j-text bg-verevon-j-text text-white"
										: "border-verevon-j-text/28 bg-white text-verevon-j-text/68 group-hover:border-verevon-j-text/44",
								].join(" ")}
								>
									{String(index + 1).padStart(2, "0")}
								</span>
								<span>
									<span className="block font-arbeit text-[1.08rem] font-normal tracking-[-0.035em]">
										{stage.label}
									</span>
									<span
										className={[
											"mt-1.5 block max-w-[23ch] font-protokoll text-[0.79rem] font-light leading-[1.35] transition-colors",
											isActive
												? "text-verevon-j-text/68"
												: "text-verevon-j-text/62 group-hover:text-verevon-j-text/72",
										].join(" ")}
									>
										{stage.detail}
									</span>
								</span>
							</button>
						);
					})}
				</div>

				<div
					aria-labelledby={activeTabId}
					className="mt-10 grid items-center gap-x-[clamp(46px,7vw,132px)] gap-y-10 xl:grid-cols-[minmax(0,0.78fr)_minmax(500px,1.22fr)]"
					id={panelId}
					role="tabpanel"
					tabIndex={0}
				>
					<div className="max-w-[570px]">
						<div className="flex items-center gap-3 font-protokoll text-[0.72rem] font-medium uppercase tracking-[0.18em] text-verevon-coral">
							<ActiveIcon aria-hidden="true" className="size-4" strokeWidth={1.6} />
							{String(activeStageIndex + 1).padStart(2, "0")} / {activeStage.label}
						</div>
						<h2 className="verevon-h2 mt-5 max-w-[12ch] text-balance">
							{activeStage.title}
						</h2>
						<p className="verevon-body-lg mt-7 max-w-[49ch] text-pretty">
							{activeStage.body}
						</p>
						<p className="mt-5 max-w-[49ch] font-protokoll text-[var(--text-body)] font-light leading-[1.55] text-verevon-j-text/62 text-pretty">
							{activeStage.detail}
						</p>
						<div className="mt-9 inline-flex items-center gap-3 border-y border-verevon-j-text/12 py-3 font-protokoll text-[0.82rem] font-light text-verevon-j-text/62">
							<Check aria-hidden="true" className="size-4 text-emerald-700" strokeWidth={1.8} />
							{activeStage.meta}
						</div>
					</div>

					<figure className="relative aspect-[1.48] overflow-hidden rounded-[26px] border border-verevon-j-text/10 bg-verevon-surface-soft shadow-[0_28px_84px_rgba(23,23,23,0.10)]">
						<Image
							alt={activeStage.mediaAlt}
							className="object-cover object-center"
							fill
							sizes="(max-width: 1280px) calc(100vw - 40px), 57vw"
							src={activeStage.image}
						/>
						<div className="absolute inset-x-0 top-0 h-24 bg-[linear-gradient(180deg,rgba(248,248,247,0.84),rgba(248,248,247,0))]" />
						<div className="absolute left-4 top-4 flex items-center gap-2.5 rounded-full border border-white/84 bg-white/90 px-3 py-2 font-protokoll text-[0.72rem] text-verevon-j-text/66 shadow-[0_8px_20px_rgba(23,23,23,0.08)] backdrop-blur-md sm:left-6 sm:top-6">
							<span className="size-1.5 rounded-full bg-verevon-coral" />
							Arbeidsflate i Verevon
						</div>
						<div className="absolute bottom-4 left-4 right-4 flex items-center justify-between gap-4 rounded-[16px] border border-white/80 bg-white/92 px-4 py-3 font-protokoll text-[0.74rem] font-light text-verevon-j-text/62 shadow-[0_14px_34px_rgba(23,23,23,0.10)] backdrop-blur-md sm:bottom-6 sm:left-6 sm:right-6 sm:px-5">
							<span className="truncate">{activeStage.meta}</span>
							<ArrowRight aria-hidden="true" className="size-4 shrink-0 text-verevon-j-text/62" strokeWidth={1.5} />
						</div>
					</figure>
				</div>
			</div>
		</section>
	);
}

export default WorkflowExplorer;
