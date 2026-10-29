import type { Metadata } from "next";
import { WorkflowPage } from "@/components/product/WorkflowPage";

export const metadata: Metadata = {
	title: "Arbeidsflyten",
	description:
		"Se hvordan et signal blir til et kildebasert utkast, en menneskelig beslutning og et inspiserbart resultat i Verevon.",
	alternates: {
		canonical: "/produkt/arbeidsflyten",
	},
	openGraph: {
		title: "Arbeidsflyten — Verevon",
		description:
			"Fra signal til kontrollert handling, med synlig grunnlag og menneskelig beslutning.",
		locale: "nb_NO",
		type: "website",
		url: "/produkt/arbeidsflyten",
	},
};

export default function WorkflowRoute() {
	return <WorkflowPage />;
}
