import type { Metadata } from "next";
import { VerevonHomeV3 } from "@/components/home/VerevonHomeV3";

export const metadata: Metadata = {
	title: "Forhåndsvisning V3 — Fra kundesignal til godkjent handling",
	description:
		"Argument-drevet variant av Verevon-forsiden: hver seksjon gjør ett distinkt steg — problem, prinsipper, produkt, godkjenning, plattform. Forhåndsvisning.",
	robots: { index: false, follow: false },
};

export default function V3Page() {
	return <VerevonHomeV3 />;
}
