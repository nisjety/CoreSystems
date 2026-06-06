import { describe, expect, it } from "vitest";
import {
  buildLocalSummary,
  buildLocalRecommendation,
  buildOnboardingContext,
  countOnboardingSources,
  selectLocalPlanId,
  supportsConnectorCount,
} from "./onboarding-recommendation";
import type { ConnectorPick, OnboardingState } from "./onboarding-machine";

const connectors = (ids: string[]): ConnectorPick[] => ids.map((id) => ({ id, label: id }));

describe("selectLocalPlanId", () => {
  it("returns trial when there is no signal", () => {
    expect(selectLocalPlanId({ connectors: [] })).toBe("trial");
  });

  it("returns hobby for a single small signal", () => {
    expect(selectLocalPlanId({ connectors: connectors(["slack"]) })).toBe("hobby");
  });

  it("returns standard for a mid-size team", () => {
    expect(selectLocalPlanId({ organization: { name: "Acme", employeeCount: 20 }, connectors: [] })).toBe(
      "standard",
    );
  });

  it("returns pro for a 50-99 employee org with expert intent", () => {
    expect(
      selectLocalPlanId({
        organization: { name: "Acme", employeeCount: 60 },
        website: { url: "https://acme.com", agentBrief: "We need SLA reporting and routing" },
        connectors: connectors(["slack", "notion", "github"]),
      }),
    ).toBe("pro");
  });

  it("returns enterprise for a large org with custom complexity", () => {
    expect(
      selectLocalPlanId({
        organization: { name: "Acme", employeeCount: 150 },
        website: { url: "https://acme.com", agentBrief: "governance and SSO and audit" },
        connectors: connectors(["slack", "notion", "github", "gdrive"]),
      }),
    ).toBe("enterprise");
  });
});

describe("countOnboardingSources", () => {
  it("counts unique connectors plus the website", () => {
    expect(
      countOnboardingSources({
        website: { url: "https://a.com", agentBrief: "" },
        additionalWebsites: [{ url: "https://b.com", agentBrief: "" }],
        connectors: connectors(["slack", "slack", "notion"]),
      }),
    ).toBe(4);
  });
});

describe("supportsConnectorCount", () => {
  it("allows Advanced for three connected integrations", () => {
    expect(supportsConnectorCount("standard", 3)).toBe(true);
  });

  it("keeps Essential for simple one-integration setup only", () => {
    expect(supportsConnectorCount("hobby", 1)).toBe(true);
    expect(supportsConnectorCount("hobby", 2)).toBe(false);
  });
});

describe("buildLocalSummary", () => {
  it("keeps fallback copy contextual without quoting raw user typos", () => {
    const summary = buildLocalSummary({
      organization: { name: "AQUATIQ AS" },
      website: { url: "https://aquatiq.com", agentBrief: "terneger en chatbot mot min shopify webshop" },
      connectors: [
        { id: "microsoft365", label: "Microsoft 365" },
        { id: "notion", label: "Notion" },
        { id: "github", label: "GitHub" },
      ],
      planId: "standard",
      locale: "nb",
    });

    expect(summary).toContain("AQUATIQ AS");
    expect(summary).toContain("aquatiq.com, Microsoft 365, Notion og GitHub");
    expect(summary).toContain("nettbutikken");
    expect(summary).not.toContain("terneger");
    expect(summary).not.toContain("beskrev behovet som");
    expect(summary).not.toContain("statisk FAQ");
  });
});

describe("buildLocalRecommendation", () => {
  for (const locale of ["nb", "en"] as const) {
    it(`returns a valid, non-empty recommendation (${locale})`, () => {
      const rec = buildLocalRecommendation({
        organization: { name: "Aquatiq", employeeCount: 30 },
        website: { url: "https://aquatiq.com", agentBrief: "Answer product questions" },
        connectors: connectors(["microsoft365"]),
        reasonPrefix: "Prefix:",
        locale,
      });
      expect(["trial", "hobby", "standard", "pro", "enterprise"]).toContain(rec.planId);
      expect(rec.reason.length).toBeGreaterThan(0);
      expect(rec.summary && rec.summary.length).toBeGreaterThan(0);
      expect(rec.proofPoints?.length).toBeGreaterThan(0);
      expect(rec.scopeSignals?.length).toBeGreaterThan(0);
      expect(rec.opportunities?.length).toBeGreaterThan(0);
      expect(rec.expectedOutcomes?.length).toBeGreaterThan(0);
      expect(rec.generatedAt).toMatch(/\d{4}-\d{2}-\d{2}T/);
      expect(rec.source).toBe("local");
    });
  }
});

describe("buildOnboardingContext", () => {
  it("serializes machine state into a PII-light context", () => {
    const state: OnboardingState = {
      step: "paywall",
      organization: { id: "o1", name: "Acme", size: "medium", employeeCount: 20 },
      website: { url: "https://acme.com", agentBrief: "help" },
      additionalWebsites: [{ url: "https://shop.acme.com", agentBrief: "orders" }],
      connectors: connectors(["slack", "teams"]),
      introPlayed: true,
      startedAt: 0,
    };
    const context = buildOnboardingContext(state, "en");
    expect(context.locale).toBe("en");
    expect(context.organization?.name).toBe("Acme");
    expect(context.sourceCount).toBe(
      countOnboardingSources({
        website: state.website,
        additionalWebsites: state.additionalWebsites,
        connectors: state.connectors,
      }),
    );
    // slack + teams(→microsoft365) = 2 distinct connectors, plus two websites = 4.
    expect(context.sourceCount).toBe(4);
    expect(context.connectors).toHaveLength(2);
  });
});
