"use client";

import Image from "next/image";
import { useState } from "react";
import { Footer } from "@/components/core/footer/Footer";
import { ArrowButton } from "@/components/ui/ArrowButton";
import { ControlledWorkHeader } from "./ControlledWorkHeader";

type Decision = "approve" | "revise" | "manual";

type DecisionOutcome = {
	label: string;
	description: string;
	audit: string;
	className: string;
};

const decisionOptions: Array<{ id: Decision; label: string; description: string }> = [
	{
		id: "approve",
		label: "Godkjenn",
		description: "Merk forslaget som vurdert og klart for neste avklarte steg.",
	},
	{
		id: "revise",
		label: "Revider",
		description: "Send forslaget tilbake til en ny menneskelig vurdering.",
	},
	{
		id: "manual",
		label: "Gjør selv",
		description: "Legg forslaget til side og fortsett arbeidet manuelt.",
	},
];

const decisionOutcomes: Record<Decision, DecisionOutcome> = {
	approve: {
		label: "Godkjent i dette eksempelet",
		description:
			"Forslaget er markert som vurdert. Denne demonstrasjonen sender eller endrer ikke noe i et tilkoblet system.",
		audit: "Beslutning registrert i demonstrasjonen · Godkjent av Nora H.",
		className: "border-emerald-700/16 bg-emerald-600/[0.08] text-emerald-950",
	},
	revise: {
		label: "Sendt til revisjon i dette eksempelet",
		description:
			"Ingen handling er utført. Forslaget er lagt tilbake til vurdering med det samme grunnlaget synlig.",
		audit: "Beslutning registrert i demonstrasjonen · Revideres av kundeteamet",
		className: "border-amber-700/18 bg-amber-500/[0.10] text-amber-950",
	},
	manual: {
		label: "Manuelt arbeid valgt",
		description:
			"Forslaget er lagt til side. Dere kan fortsette arbeidet selv, uten at Verevon utfører en handling på deres vegne.",
		audit: "Beslutning registrert i demonstrasjonen · Ingen automatisert handling utført",
		className: "border-verevon-j-text/14 bg-verevon-j-text/[0.055] text-verevon-j-text",
	},
};

const proofRows = [
	{
		label: "Kildeutdrag",
		value: "«Ved forsinket leveranse kan kunden få oppdatert status og neste steg før ny levering avtales.»",
	},
	{
		label: "Tidspunkt og ferskhet",
		value: "Returpolicy 04 · sist oppdatert 14. juli 2026, 09:18",
	},
	{
		label: "Kontekst brukt",
		value: "Kundesamtale, ordre #52481 og leveringsstatus fra det valgte systemet.",
	},
	{
		label: "Policytreff",
		value: "Retur og levering / P04 · forslag krever en menneskelig vurdering.",
	},
];

function StatusMark({ className = "" }: { className?: string }) {
	return (
		<svg
			aria-hidden="true"
			className={className}
			fill="none"
			viewBox="0 0 20 20"
			xmlns="http://www.w3.org/2000/svg"
		>
			<circle cx="10" cy="10" r="8" stroke="currentColor" strokeWidth="1.25" />
			<path d="m6.8 10.2 2.05 2.05 4.35-4.5" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="1.35" />
		</svg>
	);
}

function ArrowMark({ className = "" }: { className?: string }) {
	return (
		<svg
			aria-hidden="true"
			className={className}
			fill="none"
			viewBox="0 0 24 24"
			xmlns="http://www.w3.org/2000/svg"
		>
			<path d="M4 12h15M14 6l6 6-6 6" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="1.4" />
		</svg>
	);
}

function HeroDecisionCard() {
	return (
		<div className="absolute bottom-[clamp(18px,2.4vw,34px)] left-[clamp(18px,2.4vw,34px)] right-[clamp(18px,2.4vw,34px)] rounded-[22px] border border-white/80 bg-white/94 p-[clamp(16px,2vw,24px)] shadow-[0_24px_64px_rgba(23,23,23,0.16)] backdrop-blur-md">
			<div className="flex items-start justify-between gap-5">
				<div>
					<p className="m-0 font-protokoll text-[0.65rem] font-medium uppercase tracking-[0.16em] text-verevon-j-text/64">
						Klar for beslutning
					</p>
					<p className="m-0 mt-2 font-arbeit text-[clamp(1.1rem,1.55vw,1.45rem)] font-normal leading-[1.04] tracking-[-0.04em] text-verevon-j-text">
						Forslag til kundesvar
					</p>
				</div>
				<span className="inline-flex items-center gap-1.5 rounded-full bg-amber-500/10 px-2.5 py-1 font-protokoll text-[0.68rem] font-medium text-amber-900">
					<span className="size-1.5 rounded-full bg-amber-600" />
					Venter
				</span>
			</div>

			<div className="mt-4 grid gap-2 border-t border-verevon-j-text/8 pt-3 font-protokoll text-[0.78rem] font-light text-verevon-j-text/62 sm:grid-cols-2">
				<span>Policy P04 matchet</span>
				<span>Grunnlag synlig</span>
			</div>
		</div>
	);
}

export function ControlledWorkPage() {
	const [decision, setDecision] = useState<Decision | null>(null);
	const outcome = decision ? decisionOutcomes[decision] : null;

	return (
		<div className="min-h-screen bg-background text-verevon-text [--verevon-edge:clamp(32px,5.55vw,208px)] [--verevon-page-pad:clamp(20px,4vw,56px)] [--verevon-section-vpad:clamp(80px,11vw,160px)]">
			<ControlledWorkHeader />

			<main>
				<section
					className="border-b border-verevon-j-text/8 px-[var(--verevon-edge)] pb-[clamp(72px,9vw,144px)] pt-[clamp(74px,10vw,148px)] max-[760px]:px-[var(--verevon-page-pad)]"
					id="top"
				>
					<div className="mx-auto grid max-w-[1680px] items-center gap-x-[clamp(44px,7.2vw,138px)] gap-y-12 xl:grid-cols-[minmax(0,0.9fr)_minmax(520px,1.1fr)]">
						<div className="max-w-[700px]">
							<p className="verevon-eyebrow !text-[#686867]">Plattform / Kontrollert arbeid</p>
							<h1 className="verevon-display mt-7 max-w-[10ch] text-balance">
								Arbeid som kan forklares før det skjer.
							</h1>
							<p className="verevon-body-lg mt-8 max-w-[48ch] text-pretty">
								Verevon foreslår. Dere bestemmer.
							</p>
							<p className="mt-5 max-w-[54ch] font-protokoll text-[var(--text-body)] font-light leading-[1.55] text-verevon-j-text/60 text-pretty">
								Før noe sendes, endres eller publiseres, kan dere se hva forslaget bygger på, hvilken ramme som gjelder og hvem som tar neste valg.
							</p>

							<div className="mt-10 flex flex-wrap items-center gap-x-8 gap-y-5">
								<ArrowButton href="#forslag">Prøv beslutningskortet</ArrowButton>
								<a
									className="font-protokoll text-[0.92rem] font-light text-verevon-j-text/62 underline-offset-4 transition-colors hover:text-verevon-j-text hover:underline focus-visible:text-verevon-j-text focus-visible:outline-none"
									href="#grenser"
								>
									Se grensene
								</a>
							</div>
						</div>

						<figure className="relative isolate min-h-[420px] overflow-hidden rounded-[30px] border border-verevon-j-text/8 bg-verevon-surface-soft shadow-[0_32px_100px_rgba(23,23,23,0.10)] sm:min-h-[520px]">
							<Image
								alt="En person i samtale som illustrerer en menneskelig vurdering før neste arbeidssteg."
								className="object-cover object-[52%_center]"
								fill
								priority
								sizes="(max-width: 1280px) calc(100vw - 40px), 50vw"
								src="/man-talking-and-delegating.jpg"
							/>
							<HeroDecisionCard />
						</figure>
					</div>
				</section>

				<section
					className="border-b border-verevon-j-text/8 px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]"
					id="forslag"
				>
					<div className="mx-auto grid max-w-[1680px] items-start gap-x-[clamp(48px,8vw,150px)] gap-y-12 xl:grid-cols-[minmax(0,0.74fr)_minmax(560px,1.26fr)]">
						<div className="max-w-[580px] xl:sticky xl:top-[120px]">
							<p className="verevon-eyebrow !text-[#686867]">Et konkret forslag</p>
							<h2 className="verevon-h2 mt-6 max-w-[12ch] text-balance">
								Grunnlaget er der før avgjørelsen.
							</h2>
							<p className="verevon-body-lg mt-7 max-w-[48ch] text-pretty">
								Dette er en interaktiv demonstrasjon av hvordan en sak kan holdes åpen for menneskelig vurdering. Velg det løpet som passer arbeidet deres.
							</p>
							<p className="mt-6 max-w-[49ch] font-protokoll text-[var(--text-body)] font-light leading-[1.55] text-verevon-j-text/62 text-pretty">
								Forslaget gjør ikke en handling av seg selv her. Det viser grunnlaget, rammen og valget som står igjen hos dere.
							</p>
						</div>

						<article
							aria-describedby="decision-card-description"
							aria-labelledby="decision-card-title"
							className="overflow-hidden rounded-[28px] border border-verevon-j-text/12 bg-white shadow-[0_26px_80px_rgba(23,23,23,0.08)]"
						>
							<div className="flex flex-wrap items-start justify-between gap-5 border-b border-verevon-j-text/10 px-[clamp(20px,3vw,36px)] py-[clamp(18px,2.4vw,28px)]">
								<div>
									<p className="m-0 font-protokoll text-[0.68rem] font-medium uppercase tracking-[0.16em] text-verevon-j-text/64">
										Forslag til handling
									</p>
									<h3 className="m-0 mt-2 font-arbeit text-[clamp(1.65rem,2.25vw,2.4rem)] font-light leading-[0.98] tracking-[-0.05em] text-verevon-j-text" id="decision-card-title">
										Klargjør svar om forsinket levering
									</h3>
								</div>
								<span className="inline-flex items-center gap-2 rounded-full border border-amber-700/15 bg-amber-500/[0.09] px-3 py-1.5 font-protokoll text-[0.72rem] font-medium text-amber-950">
									<span className="size-1.5 rounded-full bg-amber-600" />
									Middels risiko · vurdering
								</span>
							</div>

							<div className="grid gap-0 lg:grid-cols-[minmax(0,1.02fr)_minmax(250px,0.78fr)]">
								<div className="p-[clamp(20px,3vw,36px)]">
									<p className="m-0 font-protokoll text-[0.78rem] font-medium uppercase tracking-[0.12em] text-verevon-j-text/64">
										Hva forslaget gjør
									</p>
									<p className="m-0 mt-3 max-w-[47ch] font-protokoll text-[clamp(1.02rem,1.3vw,1.2rem)] font-light leading-[1.5] text-verevon-j-text/76" id="decision-card-description">
										Svarutkastet forklarer forsinkelsen, viser neste leveringssteg og ber kunden bekrefte om leveringstidspunktet fortsatt passer.
									</p>

									<blockquote className="m-0 mt-8 rounded-[18px] border border-verevon-j-text/8 bg-verevon-surface-soft px-5 py-5 font-protokoll text-[0.94rem] font-light leading-[1.5] text-verevon-j-text/70">
										<p className="m-0 text-[0.67rem] font-medium uppercase tracking-[0.15em] text-verevon-j-text/64">
											Kundehenvendelse
										</p>
										<p className="m-0 mt-2">
											«Pakken min har stått som sendt i flere dager. Kan dere si hva som skjer?»
										</p>
									</blockquote>

									<div className="mt-8 border-t border-verevon-j-text/10 pt-5">
										<p className="m-0 font-protokoll text-[0.68rem] font-medium uppercase tracking-[0.16em] text-verevon-j-text/64">
											Beslutningsroller i eksempelet
										</p>
										<div className="mt-3 grid gap-2.5 font-protokoll text-[0.88rem] font-light leading-[1.4] text-verevon-j-text/62 sm:grid-cols-3">
											<div className="rounded-xl border border-verevon-j-text/8 px-3.5 py-3">
												<span className="block text-[0.66rem] font-medium uppercase tracking-[0.12em] text-verevon-j-text/64">Godkjenner</span>
												<span className="mt-1 block text-verevon-j-text/76">Nora H.</span>
											</div>
											<div className="rounded-xl border border-verevon-j-text/8 px-3.5 py-3">
												<span className="block text-[0.66rem] font-medium uppercase tracking-[0.12em] text-verevon-j-text/64">Reviderer</span>
												<span className="mt-1 block text-verevon-j-text/76">Kundeteamet</span>
											</div>
											<div className="rounded-xl border border-verevon-j-text/8 px-3.5 py-3">
												<span className="block text-[0.66rem] font-medium uppercase tracking-[0.12em] text-verevon-j-text/64">Stopper</span>
												<span className="mt-1 block text-verevon-j-text/76">Ansvarlig leder</span>
											</div>
										</div>
									</div>
								</div>

								<aside className="border-t border-verevon-j-text/10 bg-verevon-surface-soft p-[clamp(20px,3vw,32px)] lg:border-l lg:border-t-0" aria-label="Grunnlaget for forslaget">
									<p className="m-0 font-protokoll text-[0.68rem] font-medium uppercase tracking-[0.16em] text-verevon-j-text/64">
										Grunnlag
									</p>
									<dl className="m-0 mt-4 grid gap-0 border-t border-verevon-j-text/10">
										{proofRows.map((item) => (
											<div className="border-b border-verevon-j-text/10 py-4" key={item.label}>
												<dt className="font-protokoll text-[0.67rem] font-medium uppercase tracking-[0.12em] text-verevon-j-text/64">{item.label}</dt>
												<dd className="m-0 mt-1.5 font-protokoll text-[0.84rem] font-light leading-[1.45] text-verevon-j-text/68">{item.value}</dd>
											</div>
										))}
									</dl>
								</aside>
							</div>

							<div className="border-t border-verevon-j-text/10 px-[clamp(20px,3vw,36px)] py-[clamp(20px,3vw,30px)]">
								<p className="m-0 font-protokoll text-[0.68rem] font-medium uppercase tracking-[0.16em] text-verevon-j-text/64">
									Velg neste steg
								</p>
								<div className="mt-4 grid gap-2.5 sm:grid-cols-3" role="group" aria-label="Valg for forslaget">
									{decisionOptions.map((option) => {
										const isSelected = decision === option.id;

										return (
											<button
												aria-pressed={isSelected}
												className={[
											"group min-h-[124px] rounded-[17px] border p-4 text-left transition-[background-color,border-color,color,transform] duration-300 ease-out focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-verevon-j-text",
													isSelected
														? "border-verevon-j-text bg-verevon-j-text text-white"
														: "border-verevon-j-text/12 bg-white text-verevon-j-text hover:-translate-y-0.5 hover:border-verevon-j-text/30 hover:bg-verevon-surface-soft",
												].join(" ")}
												onClick={() => setDecision(option.id)}
												type="button"
												key={option.id}
											>
												<span className="flex items-center justify-between gap-3 font-arbeit text-[1.15rem] font-normal tracking-[-0.035em]">
													{option.label}
											<ArrowMark className="size-4 transition-transform duration-300 group-hover:translate-x-1" />
												</span>
												<span className={[
													"mt-2 block font-protokoll text-[0.78rem] font-light leading-[1.42]",
													isSelected ? "text-white/66" : "text-verevon-j-text/62",
												].join(" ")}>{option.description}</span>
											</button>
										);
									})}
								</div>

								<div aria-live="polite" className="mt-4 min-h-[96px]">
									{outcome ? (
										<div className={[
											"flex items-start gap-3 rounded-[17px] border px-4 py-4 animate-[fade-in_280ms_ease-out]",
											outcome.className,
										].join(" ")}>
											<StatusMark className="mt-0.5 size-5 shrink-0" />
											<div>
												<p className="m-0 font-protokoll text-[0.84rem] font-medium">{outcome.label}</p>
												<p className="m-0 mt-1.5 font-protokoll text-[0.8rem] font-light leading-[1.45] opacity-72">{outcome.description}</p>
												<p className="m-0 mt-3 border-t border-current/10 pt-3 font-protokoll text-[0.7rem] font-medium tracking-[0.01em] opacity-62">{outcome.audit}</p>
											</div>
										</div>
									) : (
										<p className="m-0 flex min-h-[96px] items-center rounded-[17px] border border-dashed border-verevon-j-text/15 px-4 font-protokoll text-[0.83rem] font-light leading-[1.45] text-verevon-j-text/62">
											Velg en handling for å se det tydelige utfallet. Ingenting blir sendt eller endret i denne demonstrasjonen.
										</p>
									)}
								</div>
							</div>
						</article>
					</div>
				</section>

				<section
					className="border-b border-verevon-j-text/8 px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]"
					id="grunnlag"
				>
					<div className="mx-auto grid max-w-[1680px] items-center gap-x-[clamp(48px,8vw,150px)] gap-y-12 xl:grid-cols-[minmax(0,0.88fr)_minmax(480px,1.12fr)]">
						<div className="max-w-[580px]">
							<p className="verevon-eyebrow !text-[#686867]">Etterpåvisning</p>
							<h2 className="verevon-h2 mt-6 max-w-[12ch] text-balance">
								Se hvorfor forslaget kom.
							</h2>
							<p className="verevon-body-lg mt-7 max-w-[49ch] text-pretty">
								Et godt forslag er ikke bare et godt formulert svar. Det gjør både materialet og den menneskelige beslutningen mulig å se i samme flate.
							</p>
							<ul className="m-0 mt-9 grid list-none gap-0 border-t border-verevon-j-text/12 p-0">
								{[
									"Åpne kildeutdraget uten å lete etter det",
									"Se når materialet sist var oppdatert",
									"Se hvilke rammer som møtte forslaget",
									"Se hvilken beslutning som ble valgt i eksempelet",
								].map((item, index) => (
									<li className="grid grid-cols-[auto_1fr] gap-4 border-b border-verevon-j-text/12 py-4 font-protokoll text-[0.94rem] font-light leading-[1.45] text-verevon-j-text/66" key={item}>
										<span className="font-medium text-verevon-coral">0{index + 1}</span>
										<span>{item}</span>
									</li>
								))}
							</ul>
						</div>

						<figure className="relative min-h-[480px] overflow-hidden rounded-[30px] border border-verevon-j-text/8 bg-verevon-surface-soft shadow-[0_24px_80px_rgba(23,23,23,0.08)]">
							<Image
								alt="Et nærbilde av et øye med refleksjoner av kode, brukt som et redaksjonelt bilde på å kunne se grunnlaget for et forslag."
								className="object-cover object-center"
								fill
								sizes="(max-width: 1280px) calc(100vw - 40px), 50vw"
								src="/aruc-launcher-after.jpg"
							/>
							<div className="absolute bottom-5 left-5 right-5 rounded-[20px] border border-white/82 bg-white/94 p-5 shadow-[0_18px_50px_rgba(23,23,23,0.16)] backdrop-blur-md sm:bottom-7 sm:left-7 sm:right-7">
								<div className="flex flex-wrap items-start justify-between gap-3">
									<div>
										<p className="m-0 font-protokoll text-[0.65rem] font-medium uppercase tracking-[0.16em] text-verevon-j-text/64">
											Kildegrunnlag
										</p>
										<p className="m-0 mt-1.5 font-arbeit text-[1.12rem] tracking-[-0.035em] text-verevon-j-text">
											Returpolicy 04 · avsnitt 3
										</p>
									</div>
									<time className="rounded-full bg-verevon-surface-soft px-2.5 py-1 font-protokoll text-[0.68rem] text-verevon-j-text/62" dateTime="2026-07-14T09:18:00+02:00">
										Oppdatert 14. juli
									</time>
								</div>
								<p className="m-0 mt-4 font-protokoll text-[0.85rem] font-light leading-[1.45] text-verevon-j-text/62">
									En kilde kan være synlig før en beslutning tas – ikke gjemt bak svaret etterpå.
								</p>
							</div>
						</figure>
					</div>
				</section>

				<section
					className="overflow-hidden bg-verevon-j-text px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] text-white max-[760px]:px-[var(--verevon-page-pad)]"
					id="grenser"
				>
					<div className="mx-auto grid max-w-[1680px] items-center gap-x-[clamp(48px,8vw,150px)] gap-y-12 xl:grid-cols-[minmax(0,0.88fr)_minmax(480px,1.12fr)]">
						<div className="max-w-[585px]">
							<p className="font-protokoll text-[0.72rem] font-medium uppercase tracking-[0.28em] text-white/46">
								Grenser
							</p>
							<h2 className="mt-6 max-w-[12ch] font-arbeit text-[clamp(2.45rem,4.1vw,5rem)] font-light leading-[0.94] tracking-[-0.06em] text-white text-balance">
								Verevon kan ikke utvide egne rettigheter.
							</h2>
							<p className="mt-7 max-w-[50ch] font-protokoll text-[var(--text-body-lg)] font-light leading-[1.52] text-white/64 text-pretty">
								Verevon kan forberede arbeidet innenfor den tilgangen og arbeidsflyten dere har definert. Det kan ikke gi seg selv flere systemer, roller eller handlingsrom.
							</p>
							<ul className="m-0 mt-9 grid list-none border-t border-white/16 p-0">
								{[
									{
										title: "Tilgang kommer fra dere",
										body: "Det som ikke er koblet til eller gitt tilgang til, er ikke en ny snarvei Verevon kan opprette selv.",
									},
									{
										title: "En beslutning kan stoppes",
										body: "Godkjenning, revisjon eller manuelt arbeid kan være de synlige neste stegene når saken krever det.",
									},
									{
										title: "Tilbakerulling er betinget",
										body: "Der dette er tilgjengelig i den konkrete handlingen og det tilkoblede systemet, kan en tilbakerullingsstatus vises. Det er ikke et universelt løfte.",
									},
								].map((item, index) => (
									<li className="grid grid-cols-[auto_1fr] gap-4 border-b border-white/16 py-5" key={item.title}>
										<span className="pt-0.5 font-protokoll text-[0.8rem] font-medium text-verevon-coral">0{index + 1}</span>
										<div>
											<h3 className="m-0 font-protokoll text-[0.96rem] font-medium text-white/84">{item.title}</h3>
											<p className="m-0 mt-1.5 max-w-[48ch] font-protokoll text-[0.9rem] font-light leading-[1.48] text-white/58">{item.body}</p>
										</div>
									</li>
								))}
							</ul>
						</div>

						<figure className="relative min-h-[480px] overflow-hidden rounded-[30px] border border-white/12 bg-white/8">
							<Image
								alt="Et rolig digitalt landskap med en lys kule, brukt som et bilde på at agentarbeid skjer innenfor synlige rammer."
								className="object-cover object-center"
								fill
								sizes="(max-width: 1280px) calc(100vw - 40px), 50vw"
								src="/agent-run-console-running.jpg"
							/>
							<div className="absolute bottom-5 left-5 right-5 rounded-[20px] border border-white/18 bg-verevon-j-text/72 p-5 shadow-[0_18px_50px_rgba(0,0,0,0.26)] backdrop-blur-md sm:bottom-7 sm:left-7 sm:right-7">
								<div className="flex items-start gap-3">
									<StatusMark className="mt-0.5 size-5 shrink-0 text-emerald-300" />
									<div>
										<p className="m-0 font-protokoll text-[0.67rem] font-medium uppercase tracking-[0.16em] text-white/48">
											Status i dette eksempelet
										</p>
										<p className="m-0 mt-1.5 font-arbeit text-[1.16rem] leading-[1.08] tracking-[-0.035em] text-white">
											Rammer synlige før neste steg.
										</p>
									</div>
								</div>
							</div>
						</figure>
					</div>
				</section>

				<section className="px-[var(--verevon-edge)] py-[clamp(92px,12vw,180px)] max-[760px]:px-[var(--verevon-page-pad)]">
					<div className="mx-auto grid max-w-[1680px] gap-10 border-t border-verevon-j-text/12 pt-[clamp(38px,5vw,72px)] xl:grid-cols-[minmax(0,1fr)_auto] xl:items-end">
						<div>
							<p className="verevon-eyebrow !text-[#686867]">Arbeidet deres</p>
							<h2 className="verevon-h2 mt-6 max-w-[14ch] text-balance">
								Mer framdrift. Samme ansvar.
							</h2>
							<p className="mt-6 max-w-[52ch] font-protokoll text-[var(--text-body)] font-light leading-[1.55] text-verevon-j-text/60 text-pretty">
								Verevon kan gjøre neste steg tydeligere. Det menneskelige ansvaret og rammene blir værende hos dere.
							</p>
						</div>
						<div className="flex flex-wrap items-center gap-x-8 gap-y-5 xl:justify-end">
							<ArrowButton href="/plattform/felles-kontekst">Se felles kontekst</ArrowButton>
							<ArrowButton href="/produkt/arbeidsflyten" variant="muted">
								Se arbeidsflyten
							</ArrowButton>
						</div>
					</div>
				</section>
			</main>

			<div className="relative overflow-clip bg-verevon-footer-bg">
				<Footer />
			</div>
		</div>
	);
}

export default ControlledWorkPage;
