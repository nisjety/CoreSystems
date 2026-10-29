import { describe, expect, it } from "vitest";
import {
  getAlternatePath,
  getLocaleFromPathname,
  localizeHref,
  localizePath,
  localizedRoutes,
} from "./routes";

describe("localized routes", () => {
  it("gives every page a unique address in both languages", () => {
    const nb = localizedRoutes.map((route) => route.nb);
    const en = localizedRoutes.map((route) => route.en);

    expect(new Set(nb).size).toBe(nb.length);
    expect(new Set(en).size).toBe(en.length);
    expect(en.every((path) => path === "/en" || path.startsWith("/en/"))).toBe(
      true,
    );
    expect(nb.some((path) => path.startsWith("/en"))).toBe(false);
  });

  it("uses English slugs for English pages", () => {
    expect(localizePath("/tjenester/ki-og-automatisering", "en")).toBe(
      "/en/services/ai-and-automation",
    );
    expect(localizePath("/om-oss", "en")).toBe("/en/about");
    expect(localizePath("/om-oss", "nb")).toBe("/om-oss");
  });

  it("localizes internal hrefs and keeps queries, hashes and external links", () => {
    expect(localizeHref("/kontakt", "en")).toBe("/en/contact");
    expect(localizeHref("/kontakt?tjeneste=seo", "en")).toBe(
      "/en/contact?tjeneste=seo",
    );
    expect(localizeHref("/#tjenester", "en")).toBe("/en#tjenester");
    expect(localizeHref("/tjenester/", "en")).toBe("/en/services");
    expect(localizeHref("#kontakt", "en")).toBe("#kontakt");
    expect(localizeHref("https://agenci.no/", "en")).toBe("https://agenci.no/");
    expect(localizeHref("mailto:post@triodelab.no", "en")).toBe(
      "mailto:post@triodelab.no",
    );
    expect(localizeHref("/kontakt", "nb")).toBe("/kontakt");
  });

  it("finds the same page in the other language for the switcher", () => {
    expect(getLocaleFromPathname("/en/services")).toBe("en");
    expect(getLocaleFromPathname("/en")).toBe("en");
    expect(getLocaleFromPathname("/english")).toBe("nb");
    expect(getAlternatePath("/tjenester/nettbutikk", "en")).toBe(
      "/en/services/ecommerce",
    );
    expect(getAlternatePath("/en/services/ecommerce", "nb")).toBe(
      "/tjenester/nettbutikk",
    );
    expect(getAlternatePath("/en", "nb")).toBe("/");
    expect(getAlternatePath("/finnes-ikke", "en")).toBe("/en");
  });
});
