import type { Metadata } from "next";
import { VerevonHome } from "@/components/home/VerevonHome";
import { siteDescription } from "@/shared/seo/site";

export const metadata: Metadata = {
	alternates: {
		canonical: "/",
	},
	openGraph: {
		title: "Verevon — Én styrt arbeidsflate for mennesker og AI",
		description: siteDescription,
		locale: "nb_NO",
		type: "website",
		url: "/",
		images: [
			{
				alt: "Verevon — Én styrt arbeidsflate for mennesker og AI",
				height: 630,
				url: "/opengraph-image",
				width: 1200,
			},
		],
	},
};

export default function HomePage() {
	return <VerevonHome />;
}
