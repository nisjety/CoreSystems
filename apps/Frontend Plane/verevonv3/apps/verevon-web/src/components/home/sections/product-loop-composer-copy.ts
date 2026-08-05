import type { VerevonComposerMode } from "@/components/ui/VerevonComposerPreview";

type ProductLoopComposerCopy = {
	body: string;
	label: string;
	title: string;
};

const productLoopComposerCopy: Record<
	VerevonComposerMode,
	ProductLoopComposerCopy
> = {
	chat: {
		label: "04 / Chat",
		title: "Spør. Få arbeidet i gang.",
		body: "Beskriv oppgaven. Verevon bruker virksomhetens kunnskap, regler og verktøy til å foreslå neste steg — klart for deres godkjenning.",
	},
	crawl: {
		label: "04 / Crawl",
		title: "Hent innhold. Bygg kunnskap.",
		body: "Hent nettsider og eksterne kilder inn i Verevon, behold kildehenvisningene og gjør innholdet klart for søk og agenter.",
	},
	search: {
		label: "04 / Søk",
		title: "Søk på tvers. Se grunnlaget.",
		body: "Finn relevant kunnskap på tvers av dokumenter, systemer, web og norske datakilder — med synlige kilder i resultatet.",
	},
};

export function getProductLoopComposerCopy(mode: VerevonComposerMode) {
	return productLoopComposerCopy[mode];
}
