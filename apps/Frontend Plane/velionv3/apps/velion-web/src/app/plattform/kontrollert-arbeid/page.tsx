import type { Metadata } from "next";
import { ControlledWorkPage } from "@/components/platform/ControlledWorkPage";

export const metadata: Metadata = {
	title: "Kontrollert arbeid",
	description:
		"Velion gjør grunnlaget, rammene og den menneskelige beslutningen synlig før arbeid skjer.",
	openGraph: {
		title: "Kontrollert arbeid — Velion",
		description:
			"Se grunnlaget før noe sendes, endres eller publiseres.",
		locale: "nb_NO",
		type: "website",
	},
};

export default function ControlledWorkRoute() {
	return <ControlledWorkPage />;
}
