import type { Metadata } from "next";
import { VerevonHomeV2 } from "@/components/home/VerevonHomeV2";

export const metadata: Metadata = {
	title: "Forhåndsvisning V2 — Fra kundesignal til godkjent handling",
	description:
		"Roligere, premium variant av Verevon-forsiden: én ambient hero, bevis-rad, felles CTA-stil og tydelig typografi. Forhåndsvisning.",
	robots: { index: false, follow: false },
};

export default function V2Page() {
	return <VerevonHomeV2 />;
}
