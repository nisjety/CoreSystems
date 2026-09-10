import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import { RouteTransition } from "@/components/core/RouteTransition";
import { siteDescription, siteName, siteUrl } from "@/shared/seo/site";
import "./globals.css";

const geistSans = Geist({
	variable: "--font-geist-sans",
	subsets: ["latin"],
});

const geistMono = Geist_Mono({
	variable: "--font-geist-mono",
	subsets: ["latin"],
});

export const metadata: Metadata = {
	metadataBase: siteUrl,
	title: {
		default: "Verevon — Fra kundesignal til godkjent handling",
		template: "%s — Verevon",
	},
	description: siteDescription,
	alternates: {
		canonical: "/",
	},
	openGraph: {
		title: "Verevon — Fra kundesignal til godkjent handling",
		description:
			"Norsk AI-arbeidsbenk for kundearbeid: kilder, svarforslag, godkjenning og revisjonsspor i én flate.",
		locale: "nb_NO",
		type: "website",
		url: "/",
		images: [
			{
				alt: "Verevon — Fra kundesignal til godkjent handling",
				height: 630,
				url: "/opengraph-image",
				width: 1200,
			},
		],
	},
	twitter: {
		card: "summary_large_image",
		description: siteDescription,
		title: "Verevon — Fra kundesignal til godkjent handling",
		images: ["/opengraph-image"],
	},
};

const structuredData = {
	"@context": "https://schema.org",
	"@graph": [
		{
			"@type": "Organization",
			name: siteName,
			url: siteUrl.toString(),
			email: "hei@verevon.ai",
			description: siteDescription,
		},
		{
			"@type": "WebSite",
			name: siteName,
			url: siteUrl.toString(),
			inLanguage: "nb-NO",
		},
		{
			"@type": "SoftwareApplication",
			name: siteName,
			applicationCategory: "BusinessApplication",
			operatingSystem: "Web",
			url: siteUrl.toString(),
			description: siteDescription,
			featureList: [
				"Kildebaserte svar",
				"Godkjenning før handling",
				"Revisjonsspor",
			],
		},
	],
};

export default function RootLayout({
	children,
}: Readonly<{
	children: React.ReactNode;
}>) {
	return (
		<html data-scroll-behavior="smooth" lang="nb">
			<body className={`${geistSans.variable} ${geistMono.variable}`}>
				<script
					dangerouslySetInnerHTML={{
						__html: JSON.stringify(structuredData).replace(/</g, "\\u003c"),
					}}
					type="application/ld+json"
				/>
				<RouteTransition />
				<div className="verevon-page-shell" data-verevon-page-shell>
					{children}
				</div>
			</body>
		</html>
	);
}
