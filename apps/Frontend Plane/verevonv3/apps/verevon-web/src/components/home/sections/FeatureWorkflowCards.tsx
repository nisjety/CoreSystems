import { FeatureCardFilm, type FeatureFilmKind } from "./FeatureCardFilms";

export type WorkflowCard = {
	film: FeatureFilmKind;
	features: string[];
	href: string;
	kicker: string;
	title: string;
	text: string;
};

// These four cards remain the original workflow sequence used by the pinned
// FeatureCardsSection.
export const workflowCards: WorkflowCard[] = [
	{
		film: "build",
		href: "#plattform",
		kicker: "AGENTARBEID",
		title: "Agenter & arbeidsflater",
		text: "Sett mål og regler, og la arbeidet starte i chat, innboks, tickets eller agent runs.",
		features: ["Chat", "Agent runs", "Inbox", "Studio"],
	},
	{
		film: "connect",
		href: "#plattform",
		kicker: "KILDER",
		title: "Kilder & systemer",
		text: "Koble dokumenter, nettsteder, norske datakilder og forretningssystemer til samme kontekst.",
		features: ["Dokumenter", "Nettsteder", "ERP / CRM", "MCP"],
	},
	{
		film: "ground",
		href: "#kunnskap",
		kicker: "GRUNNLAG",
		title: "Kunnskap & søk",
		text: "Gjør kildene søkbare, samle kontekst og vis hva hvert svar eller forslag bygger på.",
		features: ["Søk", "Crawl", "Kildespor", "Norske data"],
	},
	{
		film: "approve",
		href: "/trust",
		kicker: "STYRING",
		title: "Kontroll & drift",
		text: "Følg stegene, sett grenser og godkjenn viktige handlinger før noe sendes eller endres.",
		features: ["Policy", "Godkjenning", "Audit", "Kostnad"],
	},
];

// The public platform carousel expands the workflow into the six surfaces
// visitors should understand: entry, connection, knowledge, research,
// action, and control.
export const platformCards: WorkflowCard[] = [
	...workflowCards.slice(0, 3),
	{
		film: "research",
		href: "#kunnskap",
		kicker: "RESEARCH",
		title: "Research & overvåking",
		text: "Undersøk kilder, følg utvikling og samle det som er relevant før arbeidet går videre.",
		features: ["Web", "Norske data", "Overvåking", "Sammenligning"],
	},
	{
		film: "actions",
		href: "#plattform",
		kicker: "HANDLINGER",
		title: "Handlinger & automatisering",
		text: "Bruk godkjente verktøy og integrasjoner til å oppdatere, sende eller følge opp — innenfor grensene dere setter.",
		features: ["Verktøy", "MCP", "Policy", "Tilbakerulling"],
	},
	workflowCards[3],
];

export function FeatureWorkflowCard({
	active = true,
	animated = true,
	card,
	index,
	total = 4,
	className = "",
}: {
	active?: boolean;
	animated?: boolean;
	card: WorkflowCard;
	index: number;
	total?: number;
	className?: string;
}) {
	return (
		<a
			aria-label={`${card.title}: ${card.text}`}
			className={`group flex min-w-0 flex-col gap-3${animated ? " md:invisible" : ""} ${className}`}
			data-feature-active={active ? "true" : "false"}
			data-feature-index={index}
			href={card.href}
			{...(animated ? { "data-feature-card": "" } : {})}
		>
			<div className="flex h-[15px] select-none items-center justify-between font-protokoll text-[10px] leading-none text-verevon-j-text/42">
				<span className="size-[5px] rounded-full bg-verevon-coral/70" />
				<span>{`0${index + 1} / ${String(total).padStart(2, "0")}`}</span>
			</div>

			<div className="relative aspect-[4/5] overflow-hidden bg-transparent">
				<FeatureCardFilm kind={card.film} />
			</div>

			<h3 className="m-0 max-w-full break-words font-arbeit text-[clamp(1.25rem,1.55vw,1.75rem)] font-light leading-[1.04] tracking-[-0.055em] text-verevon-j-text">
				{card.title}
			</h3>

			<p className="m-0 font-protokoll text-[9px] font-medium uppercase tracking-[0.17em] text-verevon-coral/80">
				{card.kicker}
			</p>

			<div
				aria-label={`${card.kicker}: ${card.features.join(", ")}`}
				className="flex flex-wrap gap-1.5"
			>
				{card.features.map((feature) => (
					<span
						className="rounded-full border border-verevon-j-text/10 px-2 py-1 font-protokoll text-[9px] uppercase tracking-[0.12em] text-verevon-j-text/58"
						key={feature}
					>
						{feature}
					</span>
				))}
			</div>

			<p className="m-0 font-protokoll text-[clamp(0.82rem,0.82vw,0.94rem)] font-light leading-[1.4] text-verevon-text-muted/90">
				{card.text}
			</p>
		</a>
	);
}
