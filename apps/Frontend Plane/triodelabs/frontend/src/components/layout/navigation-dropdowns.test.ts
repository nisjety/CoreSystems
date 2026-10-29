import { describe, expect, it } from "vitest";
import { navigationDropdowns } from "./navigation-dropdowns";

const { nb, en } = navigationDropdowns;

describe("navigation dropdowns", () => {
  it("keeps overview links and their service or product destinations together", () => {
    expect(nb.services.overview.href).toBe("/tjenester");
    expect(nb.services.items).toHaveLength(6);
    expect(nb.services.items.at(-1)).toMatchObject({
      number: "06",
      label: "KI og automatisering",
      href: "/tjenester/ki-og-automatisering",
    });
    expect(nb.products.overview.href).toBe("/plattform");
    expect(nb.products.items.map((item) => item.label)).toEqual([
      "Verevon",
      "Agenci",
    ]);
    expect(nb.products.items.every((item) => item.external)).toBe(true);
  });

  it("uses the service-specific imagery in every service menu card", () => {
    expect(nb.services.items.map((item) => item.image)).toEqual([
      "/images/Triodelab-weblosning.jpg",
      "/images/Nettbutikk.jpg",
      "/images/OrganiskTraffikk.jpg",
      "/images/Integrasjoner.jpg",
      "/images/Google-meta-Ads.jpg",
      "/images/AI-automation.jpg",
    ]);
  });

  it("uses the selected laptop image for the web solutions menu card", () => {
    const webSolutions = nb.services.items.find(
      (item) => item.href === "/tjenester/web-losninger",
    );

    expect(webSolutions).toMatchObject({
      image: "/images/Triodelab-weblosning.jpg",
      imagePosition: "50% 66%",
    });
  });

  it("gives the English menu the same destinations and images, in English", () => {
    for (const id of ["services", "products"] as const) {
      const strip = (items: typeof nb.services.items) =>
        items.map(({ href, image, imagePosition, number, external }) => ({
          href,
          image,
          imagePosition,
          number,
          external,
        }));

      expect(strip(en[id].items)).toEqual(strip(nb[id].items));
      expect(en[id].overview.href).toBe(nb[id].overview.href);
      expect(en[id].id).toBe(id);
    }
    expect(en.services.title).toBe("Services");
    expect(en.services.items.at(-1)?.label).toBe("AI and automation");
  });
});
