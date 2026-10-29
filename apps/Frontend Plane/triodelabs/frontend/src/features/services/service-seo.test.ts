import { describe, expect, it } from "vitest";
import { commerceSeoEn } from "./service-seo-en";
import {
  aiAutomationSeo,
  commerceSeo,
  integrationsSeo,
  servicesIndexSeo,
  webSolutionsSeo,
} from "./service-seo";

describe("service search targeting", () => {
  it("targets distinct, commercially relevant Norwegian search phrases", () => {
    expect(webSolutionsSeo.title.toLowerCase()).toContain(
      "nettside for bedrift",
    );
    expect(webSolutionsSeo.description.toLowerCase()).toContain("webdesign");
    expect(commerceSeo.title.toLowerCase()).toContain("nettbutikk");
    expect(commerceSeo.description.toLowerCase()).toContain("lage nettbutikk");
  });

  it("keeps canonical service URLs unique", () => {
    expect(webSolutionsSeo.canonical).not.toBe(commerceSeo.canonical);
    expect(webSolutionsSeo.canonical).toBe("/tjenester/web-losninger");
    expect(commerceSeo.canonical).toBe("/tjenester/nettbutikk");
  });

  it("keeps English service titles concise enough for search results", () => {
    expect(commerceSeoEn.title.length).toBeLessThanOrEqual(60);
  });

  it("uses the photographic integrations image for social previews", () => {
    expect(integrationsSeo.image).toBe(
      "/images/services/integrations-real-colleagues-lines.webp",
    );
  });

  it("makes the services overview discoverable for KI and automation", () => {
    expect(servicesIndexSeo.canonical).toBe("/tjenester");
    expect(servicesIndexSeo.title).toContain("KI");
    expect(servicesIndexSeo.description).toContain("automatisering");
  });

  it("keeps KI and automation separate from system integrations", () => {
    expect(aiAutomationSeo.canonical).toBe("/tjenester/ki-og-automatisering");
    expect(aiAutomationSeo.canonical).not.toBe(integrationsSeo.canonical);
    expect(aiAutomationSeo.serviceType).toContain("KI");
  });
});
