import type { Metadata } from "next";
import { SharedContextPage } from "@/components/platform/SharedContextPage";

export const metadata: Metadata = {
	title: "Felles kontekst",
	description:
		"Verevon samler historikken, kunnskapen og det som må skje videre i en felles arbeidskontekst.",
	alternates: {
		canonical: "/plattform/felles-kontekst",
	},
	openGraph: {
		title: "Felles kontekst — Verevon",
		description:
			"Historikk, kilder og neste steg i ett felles utgangspunkt for arbeidet.",
		locale: "nb_NO",
		type: "website",
		url: "/plattform/felles-kontekst",
	},
};

export default function SharedContextRoute() {
	return <SharedContextPage />;
}
