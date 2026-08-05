import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import { PageLoader } from "@/components/core/PageLoader";
import { RouteTransition } from "@/components/core/RouteTransition";
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
	title: {
		default: "Verevon — Fra kundesignal til godkjent handling",
		template: "%s — Verevon",
	},
	description:
		"Verevon er en norsk AI-arbeidsbenk som finner kilder, skriver forslag og stopper for godkjenning før noe sendes, publiseres eller utføres.",
	openGraph: {
		title: "Verevon — Fra kundesignal til godkjent handling",
		description:
			"Norsk AI-arbeidsbenk for kundearbeid: kilder, svarforslag, godkjenning og revisjonsspor i én flate.",
		locale: "nb_NO",
		type: "website",
	},
};

export default function RootLayout({
	children,
}: Readonly<{
	children: React.ReactNode;
}>) {
	return (
		<html data-scroll-behavior="smooth" lang="nb">
			<body className={`${geistSans.variable} ${geistMono.variable}`}>
				<PageLoader />
				<RouteTransition />
				<div className="verevon-page-shell" data-verevon-page-shell>
					{children}
				</div>
			</body>
		</html>
	);
}
