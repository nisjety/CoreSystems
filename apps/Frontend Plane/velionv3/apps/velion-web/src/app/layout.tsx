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
		default: "Velion — Suveren AI som handler, forankret og godkjent",
		template: "%s — Velion",
	},
	description:
		"Den norske AI-arbeidsbenken som gjør forankret innsikt til godkjent handling. EU-residens som standard, Brreg-forankring og menneskelig godkjenning på hvert steg.",
	openGraph: {
		title: "Velion — Suveren AI som handler, forankret og godkjent",
		description:
			"Den norske AI-arbeidsbenken: forankrede svar, observerbare agent-kjøringer og menneskelig godkjenning — med data i EU/Sverige som standard.",
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