import type { Metadata } from "next";
import Script from "next/script";
import { ChatLauncher } from "@/components/chat/chat-launcher";
import { ConsentAnalytics } from "@/features/analytics/consent-analytics";
import { ConsentWebVitals } from "@/features/web-vitals/consent-web-vitals";
import { JsonLd } from "@/components/seo/json-ld";
import { siteConfig } from "@/lib/constants";
import { htmlLang, openGraphLocale, type Locale } from "@/lib/i18n/locales";
import {
  getLanguageAlternates,
  getOrganizationJsonLd,
  siteCopy,
  websiteJsonLd,
} from "@/lib/seo";

const skipLinkLabel: Record<Locale, string> = {
  nb: "Hopp til hovedinnhold",
  en: "Skip to main content",
};

export function getRootMetadata(locale: Locale): Metadata {
  const { description, ogImage, title } = siteCopy[locale];

  return {
    title,
    description,
    applicationName: siteConfig.name,
    creator: "TriodeLab",
    category: "business",
    metadataBase: new URL(siteConfig.url),
    alternates: getLanguageAlternates("/", locale),
    openGraph: {
      title,
      description,
      url: getLanguageAlternates("/", locale).canonical,
      siteName: siteConfig.name,
      images: [
        {
          url: ogImage,
          width: 1200,
          height: 630,
          alt: siteConfig.name,
        },
      ],
      locale: openGraphLocale[locale],
      type: "website",
    },
    twitter: {
      card: "summary_large_image",
      title,
      description,
      images: [ogImage],
    },
    robots: {
      index: true,
      follow: true,
      googleBot: {
        index: true,
        follow: true,
        "max-video-preview": -1,
        "max-image-preview": "large",
        "max-snippet": -1,
      },
    },
    verification: {
      google: process.env.GOOGLE_SITE_VERIFICATION,
    },
  };
}

export function RootDocument({
  children,
  locale,
}: {
  children: React.ReactNode;
  locale: Locale;
}) {
  return (
    <html lang={htmlLang[locale]}>
      <head>
        <link
          rel="preload"
          href="/fonts/Manrope/Manrope-VariableFont_wght.woff2"
          as="font"
          type="font/woff2"
          crossOrigin="anonymous"
        />
        <link rel="icon" href="/favicon.svg" type="image/svg+xml" />
        <link rel="alternate icon" href="/favicon.ico" />
        <link
          rel="apple-touch-icon"
          sizes="180x180"
          href="/apple-touch-icon.png"
        />
        <link
          rel="icon"
          type="image/png"
          sizes="32x32"
          href="/favicon-32x32.png"
        />
        <link
          rel="icon"
          type="image/png"
          sizes="16x16"
          href="/favicon-16x16.png"
        />
        <link rel="manifest" href="/site.webmanifest" />
        <meta name="theme-color" content="#000000" />
      </head>
      <body className="font-sans antialiased relative" data-clarity-mask="True">
        <JsonLd data={getOrganizationJsonLd(locale)} />
        <JsonLd data={websiteJsonLd} />
        <ConsentAnalytics />
        <ConsentWebVitals />
        <a href="#main-content" className="skip-link">
          {skipLinkLabel[locale]}
        </a>
        {children}
        {/* Cookiebot is deferred until after hydration to avoid DOM mutations
            before React has attached to the page. data-culture picks the
            banner language. */}
        <Script
          id="Cookiebot"
          src="https://consent.cookiebot.com/uc.js"
          data-cbid="f5c7faab-7e09-432f-bf5f-d2f7dbb2c0a4"
          data-blockingmode="auto"
          data-consentmode="disabled"
          data-culture={locale}
          strategy="afterInteractive"
        />
        <ChatLauncher locale={locale} />
      </body>
    </html>
  );
}
