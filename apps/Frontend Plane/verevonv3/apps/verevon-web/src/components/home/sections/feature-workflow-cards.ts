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
 * The public carousel organizes the work a first-time visitor can recognize.
 * These are navigation areas, not separate product packages or promises of
 * features that have not been documented elsewhere on the site.
 */
export type ModuleCard = {
	href: string;
	linkLabel: string;
	area: string;
	title: string;
	text: string;
	image: string;
	imageAlt: string;
	imagePosition?: string;
	imageWidth: number;
	imageHeight: number;
};

export const moduleCards: ModuleCard[] = [
	{
		area: "Kunnskap",
		title: "Finn grunnlaget. Se sammenhengen.",
		text: "Kilder, dokumenter og innsikt samlet rundt arbeidet dere skal gjøre.",
		linkLabel: "Se kunnskapsgrunnlaget",
		href: "/plattform/felles-kontekst",
		image: "https://images.pexels.com/photos/7190383/pexels-photo-7190383.jpeg",
		imageAlt: "En hånd over arbeidstegninger på et bord i varmt sidelys",
		imagePosition: "50% center",
		imageWidth: 4000,
		imageHeight: 5000,
	},
	{
		area: "AI og agenter",
		title: "Spør én gang. Deleger neste steg.",
		text: "Fra en enkelt forespørsel til gjentakende oppgaver, innenfor avtalte rammer.",
		linkLabel: "Se arbeidsflyten",
		href: "/produkt/arbeidsflyten",
		image: "https://images.pexels.com/photos/12585842/pexels-photo-12585842.jpeg",
		imageAlt: "En person sitter rolig med en bærbar datamaskin mens en kollega passerer i bevegelse",
		imagePosition: "50% center",
		imageWidth: 5158,
		imageHeight: 3434,
	},
	{
		area: "Verktøy for arbeidet",
		title: "Arbeidet skjer her.",
		text: "Samtaler, innhold og oppfølging på samme arbeidsflate.",
		linkLabel: "Se arbeidet i flyt",
		href: "/produkt/arbeidsflyten",
		image: "https://images.unsplash.com/photo-1511376979163-f804dff7ad7b?auto=format&fit=max&fm=jpg&q=90&w=3565",
		imageAlt: "To kollegaer i samtale ved et bord, sett gjennom glass i varmt ettermiddagslys",
		imagePosition: "50% center",
		imageWidth: 3565,
		imageHeight: 5347,
	},
	{
		area: "Datamaskiner og support",
		title: "Hjelpen kan gå videre.",
		text: "Undersøk problemer og følg opp arbeidet der det skjer.",
		linkLabel: "Se et oppfølgingsløp",
		href: "/produkt/svartid",
		image: "https://images.pexels.com/photos/8837372/pexels-photo-8837372.jpeg",
		imageAlt: "En kollega lener seg inn for å hjelpe en annen med arbeidet ved en dataskjerm",
		imagePosition: "50% 75%",
		imageWidth: 4000,
		imageHeight: 6000,
	},
	{
		area: "Tilgang og kontroll",
		title: "Deleger med tydelige grenser.",
		text: "Bestem tilganger, se hva som skjer og følg opp handlingene.",
		linkLabel: "Se kontrollert arbeid",
		href: "/plattform/kontrollert-arbeid",
		image: "https://images.unsplash.com/photo-1603796846097-bee99e4a601f?auto=format&fit=max&fm=jpg&q=90&w=5973",
		imageAlt: "En person i rustfarget skjorte signerer et dokument sammen med en kollega",
		imagePosition: "50% center",
		imageWidth: 5973,
		imageHeight: 4480,
	},
];
