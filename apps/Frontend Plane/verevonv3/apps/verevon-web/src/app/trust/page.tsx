import type { Metadata } from "next";
import { Footer } from "@/components/core/footer/Footer";
import { TrustCenter } from "@/components/trust/TrustCenter";
import { TrustHeader } from "@/components/trust/TrustHeader";

export const metadata: Metadata = {
	title: "Trust Center — sikkerhet, personvern og AI-styring",
	description:
		"Verevons Trust Center: forsvarbare, live kontroller (EU-residens i Sweden Central, valgfri Zero Data Retention på Pro/Enterprise, godkjenning per handling, «Brukt av AI?»-revisjon), en ærlig sertifiserings­løype (ISO 42001, EU AI Act, SOC 2), underleverandører og dataflyt — med CLOUD Act åpent oppgitt.",
	alternates: {
		canonical: "/trust",
	},
	openGraph: {
		title: "Verevon Trust Center",
		description:
			"Forsvarbare, live kontroller og en ærlig sertifiserings­løype. EU-residens som standard, valgfri Zero Data Retention (Pro/Enterprise), og CLOUD Act åpent oppgitt.",
		locale: "nb_NO",
		type: "website",
		url: "/trust",
	},
};

export default function TrustPage() {
	return (
		<div className="min-h-screen bg-background text-verevon-text [--verevon-edge:clamp(56px,5.55vw,208px)] [--verevon-page-pad:clamp(24px,4vw,56px)] [--verevon-section-vpad:clamp(80px,11vh,140px)]">
			<TrustHeader />
			<TrustCenter />

			<div className="relative overflow-clip bg-verevon-footer-bg">
				<Footer />
			</div>
		</div>
	);
}
