import type { Metadata } from "next";
import { Cormorant_Garamond, Geist, Geist_Mono } from "next/font/google";
import "./globals.css";
import { QueryProvider } from "../components/auth/providers/QueryProvider";
import { ClientErrorHandler } from "../components/auth/providers/ClientErrorHandler";
import { GlobalLanguageProvider } from '@/components/core/contexts/GlobalLanguageContext';
import ErrorBoundary from "../components/auth/error-boundary";
import { AuthProvider } from "../components/auth/hooks/use-auth";
import { ConvexClientProvider } from "../components/providers/convex-client-provider";
import { MotionProvider } from "../components/providers/MotionProvider";
import { getServerSession } from "../components/auth/lib/auth-server";


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

export default async function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  const session = await getServerSession();
  const initialUser = session?.user
    ? {
      id: session.user.id,
      email: session.user.email,
      name: session.user.name,
      emailVerified: session.user.emailVerified,
    }
    : null;

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
            <QueryProvider>
              <AuthProvider initialUser={initialUser}>
                <ConvexClientProvider>
                  <GlobalLanguageProvider>
                    <MotionProvider>
                      {children}
                    </MotionProvider>
                  </GlobalLanguageProvider>
                </ConvexClientProvider>
              </AuthProvider>
            </QueryProvider>
          </ErrorBoundary>
        </div>
      </body>
    </html>
  );
}