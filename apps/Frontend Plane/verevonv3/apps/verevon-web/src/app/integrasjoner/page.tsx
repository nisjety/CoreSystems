import type { Metadata } from "next";
import { IntegrationsPage } from "@/components/integrations/IntegrationsPage";

export const metadata: Metadata = {
	title: "Integrasjoner",
	description: "Se hvilke systemer som kan gi Verevon relevant arbeidskontekst, fra dokumenter og samarbeid til drift og offentlige data.",
	alternates: { canonical: "/integrasjoner" },
	openGraph: {
		title: "Integrasjoner — Verevon",
		description: "Koble systemene dere allerede bruker til en tydeligere arbeidskontekst.",
		locale: "nb_NO",
		type: "website",
		url: "/integrasjoner",
	},
};

export default function IntegrationsRoute() {
	return <IntegrationsPage />;
}
