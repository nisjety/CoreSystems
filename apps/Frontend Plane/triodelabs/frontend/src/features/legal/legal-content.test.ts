import { createHash } from "node:crypto";
import { describe, expect, it } from "vitest";
import {
  cookiesDocument,
  legalDocuments,
  privacyDocument,
  salesTermsDocument,
  termsDocument,
} from "./legal-content";
import { legalDocumentsByLocale } from "./legal-content-en";

describe("legal page migration", () => {
  it.each(["nb", "en"] as const)(
    "discloses first-party Web Vitals and bounded retention in %s",
    (locale) => {
      const text = legalDocumentsByLocale[locale].privacy.sections
        .flatMap(({ paragraphs }) => paragraphs)
        .join(" ");
      expect(text).toContain("LCP");
      expect(text).toContain("INP");
      expect(text).toContain("CLS");
      expect(text).toContain("Redis");
      expect(text).toMatch(
        locale === "nb" ? /7 UTC-kalenderdager/ : /7 UTC calendar days/,
      );
    },
  );
  it("preserves every published legal URL", () => {
    expect(legalDocuments.map(({ path }) => path).sort()).toEqual([
      "/cookies",
      "/personvern",
      "/salgsvilkar",
      "/vilkar",
    ]);
  });

  // These digests were computed independently from the headings and paragraphs
  // served at www.triodelab.no on 8 October 2026, not from the local data.
  it.each([
    [
      termsDocument,
      "54433063e53d6a890b8c9d2603312f65fb97305ff2a24e2e6ffaa1094f06429a",
    ],
    [
      salesTermsDocument,
      "d6c18be5c6cef8196a22a165cf340ad646a4cf5f7d28bc86e145e933d1e0a0a9",
    ],
  ] as const)(
    "preserves the published contract terms: $path",
    (document, digest) => {
      const sections = document.sections.map(({ title, paragraphs }) => ({
        title,
        paragraphs,
      }));
      expect(
        createHash("sha256").update(JSON.stringify(sections)).digest("hex"),
      ).toBe(digest);
      expect(document.updatedAt).toBe("2026-09-24");
    },
  );

  it("discloses the revised form and optional chat without the obsolete third-party promise", () => {
    const privacy = privacyDocument.sections
      .flatMap(({ paragraphs }) => paragraphs)
      .join(" ");
    const cookies = cookiesDocument.sections
      .flatMap(({ paragraphs }) => paragraphs)
      .join(" ");
    expect(privacy).toContain("Resend");
    expect(privacy).toContain("Upstash");
    expect(privacy).not.toContain(
      "sender i dag ikke opplysninger videre til en tredjepartsleverandør",
    );
    expect(privacy).toContain("Cookiebot");
    expect(cookies).toContain("Cookiebot");
    expect(cookies).toContain("lastes først når dere velger å åpne den");
    expect(privacyDocument.updatedAt).toBe("2026-10-23");
    expect(cookiesDocument.updatedAt).toBe("2026-10-23");
  });

  it("describes consent-gated GA4 and Clarity tracking", () => {
    const privacy = privacyDocument.sections
      .flatMap(({ paragraphs }) => paragraphs)
      .join(" ");
    const cookies = cookiesDocument.sections
      .flatMap(({ paragraphs }) => paragraphs)
      .join(" ");
    expect(privacy).toContain("Google Analytics 4");
    expect(privacy).toContain("Statistikk");
    expect(privacy).toContain("2 måneder");
    expect(privacy).toContain("14 måneder");
    expect(privacy).toContain("ny aktivitet");
    expect(privacy).toContain("Microsoft Clarity");
    expect(privacy).toContain("øktopptak");
    expect(privacy).toContain("data-clarity-mask");
    expect(cookies).toContain("_ga");
    expect(cookies).toContain("2 år");
    expect(cookies).toContain("_clck");
    expect(cookies).toContain("_clsk");
    expect(cookies).toContain("trekke samtykket tilbake");
    expect(privacy + cookies).not.toContain(
      "Vi har ikke aktivert analyse- eller markedsføringsverktøy",
    );
  });

  it("keeps the English privacy and cookie notices aligned with enabled tracking", () => {
    const privacy = legalDocumentsByLocale.en.privacy.sections
      .flatMap(({ paragraphs }) => paragraphs)
      .join(" ");
    const cookies = legalDocumentsByLocale.en.cookies.sections
      .flatMap(({ paragraphs }) => paragraphs)
      .join(" ");
    expect(privacy).toContain("Google Analytics 4");
    expect(privacy).toContain("Microsoft Clarity");
    expect(privacy).toContain("Statistics");
    expect(cookies).toContain("_ga");
    expect(cookies).toContain("_clck");
    expect(cookies).toContain("cookie preferences");
    expect(privacy + cookies).not.toContain("We have not enabled analytics");
  });
});
