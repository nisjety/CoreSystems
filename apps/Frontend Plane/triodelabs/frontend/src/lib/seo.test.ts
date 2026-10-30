import { describe, expect, it } from "vitest";
import {
  createPageMetadata,
  getOrganizationJsonLd,
  organizationJsonLd,
  siteUrl,
  toAbsoluteUrl,
  websiteJsonLd,
} from "./seo";

describe("site SEO foundation", () => {
  it("uses the deployed www domain as the canonical source", () => {
    expect(siteUrl).toBe("https://www.triodelab.no");
    expect(toAbsoluteUrl("/")).toBe("https://www.triodelab.no");
    expect(toAbsoluteUrl("tjenester")).toBe(
      "https://www.triodelab.no/tjenester",
    );
  });

  it("publishes accurate organization signals for search and answer engines", () => {
    expect(organizationJsonLd["@type"]).toBe("ProfessionalService");
    expect(organizationJsonLd.name).toBe("TriodeLab");
    expect(organizationJsonLd.email).toBe("post@triodelab.no");
    expect(organizationJsonLd.areaServed).toBe("NO");
  });

  it("describes the website as Norwegian and English", () => {
    expect(websiteJsonLd["@type"]).toBe("WebSite");
    expect(websiteJsonLd.inLanguage).toEqual(["nb-NO", "en"]);
    expect(websiteJsonLd.url).toBe("https://www.triodelab.no");
  });

  it("links each page to its counterpart in the other language", () => {
    const nb = createPageMetadata({
      title: "Kontakt",
      description: "Kontakt oss",
      path: "/kontakt",
    });
    const en = createPageMetadata({
      title: "Contact",
      description: "Contact us",
      path: "/kontakt",
      locale: "en",
    });

    expect(nb.alternates?.canonical).toBe("/kontakt");
    expect(en.alternates?.canonical).toBe("/en/contact");
    expect(en.alternates?.languages).toEqual({
      "nb-NO": "/kontakt",
      en: "/en/contact",
      "x-default": "/kontakt",
    });
    expect(en.openGraph).toMatchObject({ locale: "en_GB", url: "/en/contact" });
  });

  it("does not claim an English alternate for a Norwegian-only insight page", () => {
    const insight = createPageMetadata({
      title: "Innsikt",
      description: "Fagartikler",
      path: "/innsikt/systemintegrasjon-for-nettbutikk",
    });

    expect(insight.alternates?.canonical).toBe(
      "/innsikt/systemintegrasjon-for-nettbutikk",
    );
    expect(insight.alternates?.languages).toEqual({
      "nb-NO": "/innsikt/systemintegrasjon-for-nettbutikk",
      "x-default": "/innsikt/systemintegrasjon-for-nettbutikk",
    });
  });

  it("describes the organization in the page language", () => {
    expect(getOrganizationJsonLd("en").description).toContain(
      "digital partner",
    );
    expect(organizationJsonLd.description).toContain("digital partner for");
  });
});
