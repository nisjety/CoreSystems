import { existsSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { legalDocumentsByLocale } from "../../features/legal/legal-content-en";
import { aiAutomationFaqs, aiAutomationFaqsEn } from "../../features/services/ai-automation-page-data";
import { commerceFaqs, commerceFaqsEn } from "../../features/services/commerce-page-data";
import { consultancyFaqs, consultancyFaqsEn } from "../../features/services/consultancy-page-data";
import { integrationsFaqs, integrationsFaqsEn } from "../../features/services/integrations-page-data";
import { organicTrafficFaqs, organicTrafficFaqsEn } from "../../features/services/organic-traffic-page-data";
import { paidAdsFaqs, paidAdsFaqsEn } from "../../features/services/paid-ads-page-data";
import { serviceDeliveries } from "../../features/services/service-delivery-data";
import { serviceDeliveriesEn } from "../../features/services/service-delivery-data-en";
import { serviceStories, serviceStoriesEn } from "../../features/services/services-page-data";
import { webSolutionFaqs, webSolutionFaqsEn } from "../../features/services/web-solutions-page-data";
import { localizedRoutes } from "./routes";

// The English site is a hand-written translation. These checks make sure it
// keeps the same structure and links as the Norwegian original.
describe("English translation parity", () => {
  it("has a page file for every route in both languages", () => {
    for (const route of localizedRoutes) {
      const nb = route.nb === "/" ? "" : route.nb;
      expect(
        existsSync(new URL(`../../app/(nb)${nb}/page.tsx`, import.meta.url)),
      ).toBe(true);
      expect(
        existsSync(new URL(`../../app/(en)${route.en}/page.tsx`, import.meta.url)),
      ).toBe(true);
    }
  });

  it("translates every service delivery with the same structure and links", () => {
    expect(Object.keys(serviceDeliveriesEn).sort()).toEqual(
      Object.keys(serviceDeliveries).sort(),
    );

    for (const [canonical, delivery] of Object.entries(serviceDeliveries)) {
      const english =
        serviceDeliveriesEn[canonical as keyof typeof serviceDeliveriesEn];
      expect(english.deliverables).toHaveLength(delivery.deliverables.length);
      expect(english.related.map(({ href }) => href)).toEqual(
        delivery.related.map(({ href }) => href),
      );
    }
  });

  it("answers the same number of questions on every service page", () => {
    const pairs = [
      [webSolutionFaqs, webSolutionFaqsEn],
      [commerceFaqs, commerceFaqsEn],
      [organicTrafficFaqs, organicTrafficFaqsEn],
      [integrationsFaqs, integrationsFaqsEn],
      [paidAdsFaqs, paidAdsFaqsEn],
      [aiAutomationFaqs, aiAutomationFaqsEn],
      [consultancyFaqs, consultancyFaqsEn],
    ] as const;

    for (const [nb, en] of pairs) {
      expect(en).toHaveLength(nb.length);
    }
  });

  it("keeps the service overview cards and their links", () => {
    expect(serviceStoriesEn.map(({ href }) => href)).toEqual(
      serviceStories.map(({ href }) => href),
    );
  });

  it("translates every legal section and marks the Norwegian text as binding", () => {
    for (const key of Object.keys(legalDocumentsByLocale.nb) as Array<
      keyof typeof legalDocumentsByLocale.nb
    >) {
      const nb = legalDocumentsByLocale.nb[key];
      const en = legalDocumentsByLocale.en[key];

      expect(en.path).toBe(nb.path);
      expect(en.updatedAt).toBe(nb.updatedAt);
      expect(en.notice).toContain("Norwegian version prevails");
      expect(en.sections.map((section) => section.paragraphs.length)).toEqual(
        nb.sections.map((section) => section.paragraphs.length),
      );
      expect(en.sections.map((section) => section.items?.length)).toEqual(
        nb.sections.map((section) => section.items?.length),
      );
    }
  });
});
