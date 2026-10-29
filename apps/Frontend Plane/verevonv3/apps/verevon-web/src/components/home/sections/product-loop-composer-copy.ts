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
		label: "04 / Spør",
		title: "Spør. Se sammenhengen.",
		body: "Beskriv saken. Verevon samler relevant kunnskap, regler og systemdata, slik at dere kan ta neste beslutning på et synlig grunnlag.",
	},
	crawl: {
		label: "04 / Hent",
		title: "Hent innhold. Bygg felles grunnlag.",
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
