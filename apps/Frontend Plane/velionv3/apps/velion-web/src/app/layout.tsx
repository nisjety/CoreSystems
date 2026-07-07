import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
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
		default: "Velion — Fra kundesignal til godkjent handling",
		template: "%s — Velion",
	},
	description:
		"Velion er en norsk AI-arbeidsbenk som finner kilder, skriver forslag og stopper for godkjenning før noe sendes, publiseres eller utføres.",
	openGraph: {
		title: "Velion — Fra kundesignal til godkjent handling",
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
		<html lang="nb">
			<body className={`${geistSans.variable} ${geistMono.variable}`}>
				{children}
			</body>
		</html>
	);
}
