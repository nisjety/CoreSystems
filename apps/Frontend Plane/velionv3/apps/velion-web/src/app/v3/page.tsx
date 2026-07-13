import type { Metadata } from "next";
import { VelionHomeV3 } from "@/components/home/VelionHomeV3";

export const metadata: Metadata = {
	title: "Forhåndsvisning V3 — Fra kundesignal til godkjent handling",
	description:
		"Argument-drevet variant av Velion-forsiden: hver seksjon gjør ett distinkt steg — problem, prinsipper, produkt, godkjenning, plattform. Forhåndsvisning.",
	robots: { index: false, follow: false },
};

export default function V3Page() {
	return <VelionHomeV3 />;
}
