import type { Metadata } from "next";
import { Cormorant_Garamond, Geist, Geist_Mono } from "next/font/google";
import "./globals.css";
import { ClientErrorHandler } from "../components/providers/ClientErrorHandler";
import ErrorBoundary from "../components/error-boundary";



const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

const cormorantGaramond = Cormorant_Garamond({
  variable: "--font-cormorant-garamond",
  subsets: ["latin"],
  style: ["normal", "italic"],
});

export const metadata: Metadata = {
  title: "Aquatiq Hub",
  description: "Modulær platform for eiendeler og tidsbank",
};

// Force SSR for the entire app by default
export const dynamic = 'force-dynamic';

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="nb" className={`${geistSans.variable} ${geistMono.variable} ${cormorantGaramond.variable}`} data-scroll-behavior="smooth">
      <body className="font-inter antialiased text-gray-100">
        {/* Animated noise texture background */}
        <div className="noise-background" aria-hidden="true">
          <div>
            <div />
          </div>
        </div>
        
        <div style={{ position: 'relative', zIndex: 1 }}>
          <ClientErrorHandler />
          <ErrorBoundary>
                  {children}
          </ErrorBoundary>
        </div>
      </body>
    </html>
  );
}
