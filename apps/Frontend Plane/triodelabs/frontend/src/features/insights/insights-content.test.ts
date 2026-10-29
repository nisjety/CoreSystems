import { describe, expect, it } from "vitest";
import { insights } from "./insights-content";

describe("insight content", () => {
  it("starts the resource library with a decision-focused integration guide", () => {
    expect(insights[0]).toMatchObject({
      slug: "systemintegrasjon-for-nettbutikk",
      canonical: "/innsikt/systemintegrasjon-for-nettbutikk",
      relatedService: "/tjenester/integrasjoner",
    });
    expect(insights[0].title.toLowerCase()).toContain("systemintegrasjon");
    expect(insights[0].title.toLowerCase()).toContain("nettbutikk");
    expect(insights[0]).toMatchObject({
      contentType: "Guide",
      trend: expect.objectContaining({
        sourceHref:
          "https://cloud.google.com/resources/content/ai-agent-trends-2026",
      }),
    });
    expect(insights[0].faqs).toHaveLength(5);
  });

  it("adds a practical KI-pilot guide without presenting AI as a black box", () => {
    expect(insights).toHaveLength(3);

    const kiPilot = insights.find(
      (insight) => insight.slug === "ki-pilot-for-bedrifter",
    );
    expect(kiPilot).toMatchObject({
      canonical: "/innsikt/ki-pilot-for-bedrifter",
      relatedService: "/tjenester/ki-og-automatisering",
    });
    expect(kiPilot?.title.toLowerCase()).toContain("ki-pilot");
    expect(kiPilot?.checklist).toEqual(
      expect.arrayContaining([
        expect.stringMatching(/kild/i),
        expect.stringMatching(/mennesk/i),
      ]),
    );
    expect(kiPilot).toMatchObject({
      contentType: "Guide",
      trend: expect.objectContaining({
        sourceHref:
          "https://www.ssb.no/teknologi-og-innovasjon/informasjons-og-kommunikasjonsteknologi-ikt/statistikk/bruk-av-ikt-i-naeringslivet/artikler/bruken-av-ki-i-naeringslivet-akselererer",
      }),
    });
    expect(kiPilot?.faqs).toHaveLength(5);
  });

  it("adds a cross-channel advertising article grounded in useful measurement", () => {
    const paidMedia = insights.find(
      (insight) => insight.slug === "google-meta-tiktok-snapchat-annonsering",
    );

    expect(paidMedia).toMatchObject({
      canonical: "/innsikt/google-meta-tiktok-snapchat-annonsering",
      contentType: "Fagartikkel",
      relatedService: "/tjenester/google-og-meta-ads",
      trend: expect.objectContaining({
        sourceHref:
          "https://blog.google/products/ads-commerce/google-ads-ai-transparency-labels/",
      }),
    });
    expect(paidMedia?.title.toLowerCase()).toEqual(
      expect.stringContaining("google ads"),
    );
    expect(paidMedia?.title.toLowerCase()).toEqual(
      expect.stringContaining("meta"),
    );
    expect(paidMedia?.checklist).toEqual(
      expect.arrayContaining([
        expect.stringMatching(/samtykke/i),
        expect.stringMatching(/rettighet/i),
        expect.stringMatching(/kvalifiserte/i),
      ]),
    );
    expect(paidMedia?.trend.additionalSources).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ label: expect.stringMatching(/^Meta:/) }),
        expect.objectContaining({ label: expect.stringMatching(/^TikTok:/) }),
        expect.objectContaining({ label: expect.stringMatching(/^Snap:/) }),
      ]),
    );
    expect(paidMedia?.faqs).toHaveLength(5);
  });

  it("keeps the article substantive and internally connected", () => {
    const [insight] = insights;

    expect(insight.sections.length).toBeGreaterThanOrEqual(5);
    expect(insight.checklist.length).toBeGreaterThanOrEqual(6);
    expect(insight.relatedLinks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ href: "/tjenester/nettbutikk" }),
        expect.objectContaining({
          href: "/tjenester/radgivning-og-konsulentbistand",
        }),
      ]),
    );
  });
});
