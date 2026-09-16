import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import Script from "next/script";
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
		default: "Verevon — Én arbeidsflate for mennesker og AI",
		template: "%s — Verevon",
	},
	description: siteDescription,
	alternates: {
		canonical: "/",
	},
	openGraph: {
		title: "Verevon — Én arbeidsflate for mennesker og AI",
		description: siteDescription,
		locale: "nb_NO",
		type: "website",
		url: "/",
		images: [
			{
				alt: "Verevon — Én arbeidsflate for mennesker og AI",
				height: 630,
				url: "/opengraph-image",
				width: 1200,
			},
		],
	},
	twitter: {
		card: "summary_large_image",
		description: siteDescription,
		title: "Verevon — Én arbeidsflate for mennesker og AI",
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

const fullMotionPreferenceScript = `
(() => {
	const nativeMatchMedia = window.matchMedia.bind(window);

	window.matchMedia = (query) => {
		const fullMotionQuery = query
			.replace(/\\(\\s*prefers-reduced-motion\\s*:\\s*no-preference\\s*\\)/gi, "(min-width: 0px)")
			.replace(/\\(\\s*prefers-reduced-motion(?:\\s*:\\s*reduce)?\\s*\\)/gi, "(max-width: 0px)");

		return nativeMatchMedia(fullMotionQuery);
	};
})();
`;

export default function RootLayout({
	children,
}: Readonly<{
	children: React.ReactNode;
}>) {
	return (
		<html data-motion="full" data-scroll-behavior="smooth" lang="nb">
			<head>
				<Script
					dangerouslySetInnerHTML={{ __html: fullMotionPreferenceScript }}
					id="verevon-full-motion-preference"
					strategy="beforeInteractive"
				/>
			</head>
			<body className={`${geistSans.variable} ${geistMono.variable}`}>
				<Script
					dangerouslySetInnerHTML={{
						__html: JSON.stringify(structuredData).replace(/</g, "\\u003c"),
					}}
					id="verevon-structured-data"
					strategy="beforeInteractive"
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
