import { describe, expect, it } from "vitest";
import { ownedProducts, serviceHighlights } from "./portfolio-content";

describe("portfolio content", () => {
  it("keeps service highlights and owned products in separate collections", () => {
    expect(serviceHighlights).toHaveLength(3);
    expect(ownedProducts).toHaveLength(2);
  });

  it("includes the named service highlights and owned products", () => {
    expect(ownedProducts.map((product) => product.title)).toEqual([
      "Agenci",
      "Verevon",
    ]);
    expect(serviceHighlights.map((highlight) => highlight.title)).toEqual([
      "WordPress",
      "Nettbutikk",
      "Portefølje",
    ]);
  });

  it("links the WordPress and ecommerce highlights to their relevant services", () => {
    expect(
      serviceHighlights.slice(0, 2).map((highlight) => highlight.href),
    ).toEqual(["/tjenester/web-losninger", "/tjenester/nettbutikk"]);
    expect(serviceHighlights[2]).toMatchObject({
      category: "Utvalgte arbeider",
      description: expect.stringMatching(/prosjekter/i),
    });
    expect(serviceHighlights[2].href).toBeUndefined();
  });

  it("provides the content needed by the selectable product pre-footer", () => {
    expect(ownedProducts).toEqual([
      {
        number: "01",
        title: "Agenci",
        accentColor: "#427655",
        accentTextColor: "#FFFFFF",
        category: "Eget produkt · KI-chatassistent",
        href: "https://agenci.no/",
        description:
          "En KI-chatassistent som gjør virksomhetens egen kunnskap tilgjengelig hele døgnet.",
        image: "/images/Triodelab-bilder/agenci-macbook-mockup.jpg",
      },
      {
        number: "02",
        title: "Verevon",
        accentColor: "#EE7A50",
        accentTextColor: "#161616",
        category: "Eget produkt · KI-arbeidsflyt",
        href: "https://www.verevon.com/",
        description:
          "En KI-arbeidsflyt som gjør signaler om til tydelige, kontrollerte handlinger.",
        image: "/images/Triodelab-bilder/verevon-hover-mockup.jpg",
        video: "/videos/verevon-mockup-preview.mp4",
      },
    ]);
  });
});
