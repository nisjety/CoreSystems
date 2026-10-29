import type { Metadata } from "next";
import { ControlledWorkPage } from "@/components/platform/ControlledWorkPage";

export const metadata: Metadata = {
	title: "Kontrollert arbeid",
	description:
		"Verevon gjør grunnlaget, rammene og den menneskelige beslutningen synlig før arbeid skjer.",
	alternates: {
		canonical: "/plattform/kontrollert-arbeid",
	},
	openGraph: {
		title: "Kontrollert arbeid — Verevon",
		description:
			"Se grunnlaget før noe sendes, endres eller publiseres.",
		locale: "nb_NO",
		type: "website",
		url: "/plattform/kontrollert-arbeid",
	},
};

export default function ControlledWorkRoute() {
	return <ControlledWorkPage />;
}
