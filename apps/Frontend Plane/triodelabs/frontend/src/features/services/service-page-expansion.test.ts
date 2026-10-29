import { describe, expect, it } from "vitest";
import { aiAutomationFaqs } from "./ai-automation-page-data";
import { integrationsFaqs } from "./integrations-page-data";
import { organicTrafficFaqs } from "./organic-traffic-page-data";
import { paidAdsFaqs } from "./paid-ads-page-data";
import {
  aiAutomationSeo,
  integrationsSeo,
  organicTrafficSeo,
  paidAdsSeo,
} from "./service-seo";

describe("expanded service search targeting", () => {
  it("gives each new service a distinct commercial search intent and canonical URL", () => {
    const services = [
      organicTrafficSeo,
      integrationsSeo,
      paidAdsSeo,
      aiAutomationSeo,
    ];

    expect(organicTrafficSeo.title.toLowerCase()).toContain("organisk trafikk");
    expect(organicTrafficSeo.description.toLowerCase()).toContain("seo");
    expect(integrationsSeo.title.toLowerCase()).toContain("integrasjoner");
    expect(paidAdsSeo.title.toLowerCase()).toContain("google ads");
    expect(paidAdsSeo.title.toLowerCase()).toContain("meta ads");
    expect(aiAutomationSeo.title.toLowerCase()).toContain(
      "ki og automatisering",
    );
    expect(aiAutomationSeo.description.toLowerCase()).toContain("ai");
    expect(new Set(services.map((service) => service.canonical)).size).toBe(4);
  });

  it("answers beginner questions without reusing the web and ecommerce topics", () => {
    const faqGroups = [
      organicTrafficFaqs,
      integrationsFaqs,
      paidAdsFaqs,
      aiAutomationFaqs,
    ];

    expect(faqGroups.every((faqs) => faqs.length === 2)).toBe(true);
    expect(
      faqGroups.every((faqs) => faqs.every((faq) => faq.answer.length > 120)),
    ).toBe(true);
    expect(organicTrafficFaqs[0].question).toContain("organisk trafikk");
    expect(integrationsFaqs[0].question).toContain("systemintegrasjon");
    expect(paidAdsFaqs[0].question).toContain("Google Ads");
    expect(aiAutomationFaqs[0].question).toContain("KI");
  });
});
