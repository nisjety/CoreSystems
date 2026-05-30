import type { Metadata } from "next";
import { Geist, Geist_Mono, Inter } from "next/font/google";
import { Toaster } from "sonner";
import { Providers } from "@/app/providers";
import "./globals.css";

const inter = Inter({
  variable: "--font-inter",
  subsets: ["latin"],
});

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: "Velion v2",
  description:
    "AI helpdesk, knowledge, and agent operations cockpit for enterprise teams.",
  applicationName: "Velion v2",
  metadataBase: new URL("https://velion.local"),
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html
      lang="en"
      suppressHydrationWarning
      className={`${inter.variable} ${geistSans.variable} ${geistMono.variable} h-full bg-background antialiased`}
    >
      <body className="min-h-full bg-background text-foreground">
        <div className="velion-global-background" aria-hidden="true" />
        <div className="velion-app-root">
          <Providers>
            {children}
            <Toaster richColors closeButton position="top-right" />
          </Providers>
        </div>
      </body>
    </html>
  );
}
