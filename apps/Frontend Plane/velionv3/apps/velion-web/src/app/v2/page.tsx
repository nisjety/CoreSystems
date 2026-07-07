import type { Metadata } from "next";
import { VelionHomeV2 } from "@/components/home/VelionHomeV2";

export const metadata: Metadata = {
	title: "Forhåndsvisning V2 — Fra kundesignal til godkjent handling",
	description:
		"Roligere, premium variant av Velion-forsiden: én ambient hero, bevis-rad, felles CTA-stil og tydelig typografi. Forhåndsvisning.",
	robots: { index: false, follow: false },
};

export default function V2Page() {
	return <VelionHomeV2 />;
}
