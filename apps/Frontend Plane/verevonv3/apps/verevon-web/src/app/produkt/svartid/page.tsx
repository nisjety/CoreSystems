import type { Metadata } from "next";
import { ResponseTimePage } from "@/components/platform/ResponseTimePage";

export const metadata: Metadata = {
	title: "Svartid",
	description:
		"Svaret er raskt. Prosessen rundt det er ikke — se hvordan Verevon lar en agent gjøre de fire stegene samtidig.",
	openGraph: {
		title: "Svartid — Verevon",
		description: "Agenten gjør stegene. Dere godkjenner resultatet.",
		locale: "nb_NO",
		type: "website",
	},
};

export default function ResponseTimeRoute() {
	return <ResponseTimePage />;
}
