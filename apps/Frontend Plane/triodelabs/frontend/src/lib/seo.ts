import type { Metadata } from "next";
import { siteConfig } from "./constants";
import {
  htmlLang,
  openGraphLocale,
  type Locale,
  type Localized,
} from "./i18n/locales";
import { localizedRoutes, type RoutePath } from "./i18n/routes";

export const siteUrl = siteConfig.url;

export function toAbsoluteUrl(path = "/") {
  if (path === "/") {
    return siteUrl;
  }

  const normalizedPath = path.startsWith("/") ? path : `/${path}`;

  return new URL(normalizedPath, `${siteUrl}/`).toString();
}

export const siteCopy: Localized<{
  description: string;
  ogImage: string;
  title: string;
}> = {
  nb: {
    title: siteConfig.title,
    description: siteConfig.description,
    ogImage: siteConfig.ogImage,
  },
  en: {
    title: "Digital advisory, web and AI | TriodeLab",
    description:
      "TriodeLab is a digital partner for ambitious businesses. We combine advisory, web development, e-commerce, integrations, search visibility and AI.",
    ogImage: "/en/opengraph-image",
  },
};

/** Canonical URL plus hreflang links to the same page in every language. */
export function getLanguageAlternates(path: RoutePath, locale: Locale) {
  const route = localizedRoutes.find((entry) => entry.nb === path);
  if (!route) {
    return {
      canonical: path,
      languages: {
        [htmlLang.nb]: path,
        "x-default": path,
      },
    };
  }

  return {
    canonical: route[locale],
    languages: {
      [htmlLang.nb]: route.nb,
      [htmlLang.en]: route.en,
      "x-default": route.nb,
    },
  };
}

type PageMetadataOptions = {
  description: string;
  image?: string;
  imageAlt?: string;
  locale?: Locale;
  /** The page's Norwegian route; the English URL is derived from it. */
  path: RoutePath;
  title: string;
  type?: "article" | "website";
};

export function createPageMetadata({
  description,
  image,
  imageAlt = siteConfig.name,
  locale = "nb",
  path,
  title,
  type = "website",
}: PageMetadataOptions): Metadata {
  const alternates = getLanguageAlternates(path, locale);
  const previewImage = image ?? siteCopy[locale].ogImage;

  return {
    title,
    description,
    alternates,
    openGraph: {
      title,
      description,
      url: alternates.canonical,
      siteName: siteConfig.name,
      locale: openGraphLocale[locale],
      alternateLocale: openGraphLocale[locale === "nb" ? "en" : "nb"],
      type,
      images: [{ url: previewImage, alt: imageAlt }],
    },
    twitter: {
      card: "summary_large_image",
      title,
      description,
      images: [previewImage],
    },
  };
}

export function getOrganizationJsonLd(locale: Locale) {
  return {
    "@context": "https://schema.org",
    "@type": "ProfessionalService",
    "@id": `${siteUrl}/#organization`,
    name: siteConfig.name,
    url: toAbsoluteUrl(),
    description: siteCopy[locale].description,
    image: toAbsoluteUrl(siteConfig.ogImage),
    email: siteConfig.links.email,
    telephone: siteConfig.links.phone,
    address: {
      "@type": "PostalAddress",
      streetAddress: siteConfig.address.street,
      postalCode: "0655",
      addressLocality: "Oslo",
      addressCountry: "NO",
    },
    areaServed: "NO",
    sameAs: [siteConfig.links.linkedin, siteConfig.links.github],
  } as const;
}

export const organizationJsonLd = getOrganizationJsonLd("nb");

export const websiteJsonLd = {
  "@context": "https://schema.org",
  "@type": "WebSite",
  "@id": `${siteUrl}/#website`,
  name: siteConfig.name,
  url: toAbsoluteUrl(),
  inLanguage: [htmlLang.nb, htmlLang.en],
  publisher: { "@id": `${siteUrl}/#organization` },
} as const;
