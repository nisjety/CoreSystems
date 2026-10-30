import { ImageResponse } from "next/og";
import type { Localized, Locale } from "@/lib/i18n/locales";

export const ogImageSize = { width: 1200, height: 630 };

const copy: Localized<{ headline: string; location: string; services: string }> =
  {
    nb: {
      headline: "Digitale løsninger",
      services: "Rådgivning · web · nettbutikk · KI",
      location: "Oslo / Norge",
    },
    en: {
      headline: "Digital solutions",
      services: "Advisory · web · e-commerce · AI",
      location: "Oslo / Norway",
    },
  };

// Shared preview image. Served from route handlers at /opengraph-image and
// /en/opengraph-image so the URLs stay stable (metadata image files inside
// route groups get a hashed path).
export function renderOgImage(locale: Locale) {
  const text = copy[locale];

  return new ImageResponse(
    (
      <div
        style={{
          background: "#161616",
          color: "#FCFCFA",
          display: "flex",
          flexDirection: "column",
          height: "100%",
          justifyContent: "space-between",
          padding: "72px",
          width: "100%",
        }}
      >
        <div style={{ color: "#B9D1D5", display: "flex", fontSize: 28, letterSpacing: 4 }}>
          TRIODELAB
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 18 }}>
          <div style={{ display: "flex", fontSize: 72, fontWeight: 500, letterSpacing: -3 }}>
            {text.headline}
          </div>
          <div style={{ color: "#B9D1D5", display: "flex", fontSize: 38 }}>
            {text.services}
          </div>
        </div>
        <div style={{ display: "flex", fontSize: 24, justifyContent: "space-between" }}>
          <span>{text.location}</span>
          <span>www.triodelab.no</span>
        </div>
      </div>
    ),
    ogImageSize,
  );
}
