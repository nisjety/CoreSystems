import { describe, expect, it } from "vitest";
import { insights } from "../features/insights/insights-content";
import sitemap from "./sitemap";

describe("sitemap", () => {
  it("lists canonical public pages, including KI and automation", () => {
    const urls = sitemap().map((entry) => entry.url);

    expect(urls).toContain("https://www.triodelab.no");
    expect(urls).toContain(
      "https://www.triodelab.no/tjenester/ki-og-automatisering",
    );
    expect(urls).not.toContain("https://triodelab.no/");
    expect(urls).toContain(
      "https://www.triodelab.no/tjenester/radgivning-og-konsulentbistand",
    );
    for (const path of ["personvern", "cookies", "vilkar", "salgsvilkar"]) {
      expect(urls).toContain(`https://www.triodelab.no/${path}`);
    }
    expect(urls).toContain("https://www.triodelab.no/innsikt");
    for (const insight of insights) {
      expect(urls).toContain(`https://www.triodelab.no${insight.canonical}`);
    }
    expect(new Set(urls).size).toBe(urls.length);
    expect(urls).toHaveLength(36);
    expect(sitemap().every((entry) => entry.lastModified === undefined)).toBe(
      true,
    );
  });

  it("lists every English page with hreflang alternates", () => {
    const entries = sitemap();
    const urls = entries.map((entry) => entry.url);

    expect(urls).toContain("https://www.triodelab.no/en");
    expect(urls).toContain(
      "https://www.triodelab.no/en/services/ai-and-automation",
    );
    expect(urls).toContain("https://www.triodelab.no/en/terms-of-sale");

    const about = entries.find(
      (entry) => entry.url === "https://www.triodelab.no/en/about",
    );
    expect(about?.alternates?.languages).toEqual({
      "nb-NO": "https://www.triodelab.no/om-oss",
      en: "https://www.triodelab.no/en/about",
      "x-default": "https://www.triodelab.no/om-oss",
    });
  });

  it("does not advertise English alternates for Norwegian-only insight pages", () => {
    const entry = sitemap().find(
      (item) =>
        item.url ===
        "https://www.triodelab.no/innsikt/systemintegrasjon-for-nettbutikk",
    );

    expect(entry?.alternates?.languages).toEqual({
      "nb-NO":
        "https://www.triodelab.no/innsikt/systemintegrasjon-for-nettbutikk",
      "x-default":
        "https://www.triodelab.no/innsikt/systemintegrasjon-for-nettbutikk",
    });
  });
});
