// Framework-free card copy for the home/platform feature sections.
//
// This data lives in its own `.ts` module — not inside FeatureWorkflowCards.tsx
// — so the editorial copy can be asserted by tests without pulling React or
// `next/image` into the module graph. Every other unit-tested module in this
// folder (feature-film-playback.ts, product-loop-progress.ts,
// product-loop-composer-copy.ts) follows the same split; FeaturesSection.test.ts
// used to import the .tsx directly, which broke the shared Vitest runner because
// Next.js' own dependencies are not installed for this nested app.
//
// `FeatureFilmKind` is a type-only import, so it is erased at transform time and
// FeatureCardFilms.tsx is never loaded from here.
import type { FeatureFilmKind } from "./FeatureCardFilms";

export type WorkflowCard = {
	film: FeatureFilmKind;
	features: string[];
	href: string;
	kicker: string;
	title: string;
	text: string;
};

// These four cards remain the original workflow sequence used by the pinned
// FeatureCardsSection. They still run on the recorded UI films.
export const workflowCards: WorkflowCard[] = [
	{
		film: "build",
		href: "#plattform",
		kicker: "AGENTER · CHAT",
		title: "Agenter & arbeidsflater",
		text: "Sett mål og regler, og la arbeidet starte i chat, innboks, tickets eller agent runs.",
		features: ["Chat", "Agent runs", "Inbox", "Studio"],
	},
	{
		film: "connect",
		href: "#plattform",
		// Kept as "Kilder", not renamed to "Cloud" — Verevon runs on Azure and
		// isn't sold as an infrastructure/cloud product, so that name would
		// overclaim. This card is genuinely about source/system connections.
		kicker: "KILDER",
		title: "Kilder & systemer",
		text: "Koble dokumenter, nettsteder, norske datakilder og forretningssystemer til samme kontekst.",
		features: ["Dokumenter", "Nettsteder", "ERP / CRM", "MCP"],
	},
	{
		film: "ground",
		href: "#kunnskap",
		kicker: "KUNNSKAP",
		title: "Kunnskap & søk",
		text: "Gjør kildene søkbare, samle kontekst og vis hva hvert svar eller forslag bygger på.",
		features: ["Søk", "Crawl", "Kildespor", "Norske data"],
	},
	{
		film: "approve",
		href: "/trust",
		kicker: "PROOF & TRUST",
		title: "Kontroll & drift",
		text: "Følg stegene, sett grenser og godkjenn viktige handlinger før noe sendes eller endres.",
		features: ["Policy", "Godkjenning", "Audit", "Kostnad"],
	},
];

/**
 * The public platform carousel names the six modules a visitor buys into.
 *
 * Naming note — this six-module set is a *proposal*, not documented product
 * truth. Only "Verevon Support" and the "Verevon Proof Bundle" appear in the
 * platform docs today; Knowledge / Research / Chat / Agents / Trust are new
 * names for surfaces that already exist and ship. A seventh proposed name,
 * "Verevon Cloud", is deliberately left out: Verevon runs on Azure and is not
 * sold as infrastructure, so the name would overclaim exactly the way the
 * trust docs warn against. Connections/integrations stay a capability of
 * Knowledge and Agents rather than a module of their own.
 *
 * The three groups map onto the brand promise: Finn → Forstå → Få gjort.
 */
export type ModuleCard = {
	features: string[];
	href: string;
	/**
	 * Finn / Forstå / Få gjort — the promise beat this module serves.
	 * Two modules share the FINN stage and two share FÅ GJORT, so each of
	 * those four carries a distinguishing second word (grounded in that
	 * card's own title) — otherwise the carousel shows the exact same label
	 * twice in a row, which reads as a bug, not a grouping.
	 */
	beat: string;
	image: string;
	imageAlt: string;
	/** Object-position for the crop, since these are editorial photographs. */
	imagePosition?: string;
	module: string;
	title: string;
	text: string;
};

export const moduleCards: ModuleCard[] = [
	{
		beat: "FINN",
		module: "Verevon Knowledge",
		title: "Kunnskapen deres, søkbar og sporbar.",
		text: "Dokumenter, nettsteder og forretningssystemer blir til én kontekst — der hvert svar peker tilbake på kilden det kom fra.",
		features: ["Søk", "Kildespor", "Dokumenter", "ERP / CRM"],
		href: "#kunnskap",
		image: "/verevon-mood/module-knowledge.jpg",
		imageAlt:
			"Struktur og målepunkter lagt over noe levende — kunnskap hentet ut av materialet dere allerede har.",
		imagePosition: "center 50%",
	},
	{
		beat: "FINN · UTENFOR",
		module: "Verevon Research",
		title: "Følg det som endrer seg utenfor huset.",
		text: "Web, norske registre og kilder dere velger selv — undersøkt, sammenlignet og fulgt over tid, ikke bare slått opp én gang.",
		features: ["Web", "Norske data", "Overvåking", "Sammenligning"],
		href: "#kunnskap",
		image: "/verevon-mood/module-research-v2.jpg",
		imageAlt:
			"En person arbeider ved et vindu, med verden utenfor synlig i samme blikk.",
		imagePosition: "center 40%",
	},
	{
		beat: "FORSTÅ",
		module: "Verevon Chat",
		title: "Spør på norsk. Få et grunnlag, ikke en gjetning.",
		text: "Still spørsmålet som det faktisk stilles internt, og få tilbake sammenhengen, tallene og kildene beslutningen skal hvile på.",
		features: ["Analyse", "Sammenheng", "Kilder", "Bokmål"],
		href: "#plattform",
		image: "/verevon-mood/module-chat-v2.jpg",
		imageAlt:
			"To personer i samtale ved et bord, med byen synlig i vinduet bak.",
		imagePosition: "center 42%",
	},
	{
		beat: "FÅ GJORT",
		module: "Verevon Agents",
		title: "Arbeidet går videre mens dere holder styringen.",
		text: "Sett mål, grenser og verktøy. Agenten utfører stegene, viser hva den gjør underveis, og stopper der dere har bedt den stoppe.",
		features: ["Agent runs", "Verktøy", "MCP", "Tilbakerulling"],
		href: "#plattform",
		image: "/verevon-mood/module-agents.jpg",
		imageAlt:
			"En person står stille mens arbeidet passerer i bevegelse rundt.",
		imagePosition: "center 45%",
	},
	{
		beat: "FÅ GJORT · KUNDER",
		module: "Verevon Support",
		title: "Kundearbeidet samlet på ett sted.",
		text: "Innboks, tickets og AI-utkast i samme flyt — med et menneske som leser gjennom og sender, ikke en bot som svarer på egen hånd.",
		features: ["Innboks", "Tickets", "AI-utkast", "Widget"],
		href: "#plattform",
		image: "/verevon-mood/module-support-v2.jpg",
		imageAlt:
			"En kollega leser gjennom skjermene til teamet før noe sendes videre.",
		imagePosition: "center 45%",
	},
	{
		beat: "KONTROLL",
		module: "Verevon Trust",
		title: "Hva ble brukt, hvem godkjente, hva skjedde.",
		text: "Policy før handling, godkjenning på det som betyr noe, og et spor i etterkant som viser kilde, tilgang og kostnad.",
		features: ["Policy", "Godkjenning", "Audit", "Kostnad"],
		href: "/trust",
		image: "/verevon-mood/module-trust.jpg",
		imageAlt: "En hånd som signerer et dokument på et rolig skrivebord.",
		imagePosition: "center 50%",
	},
];
