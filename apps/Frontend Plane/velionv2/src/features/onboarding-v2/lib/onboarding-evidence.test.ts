import { describe, expect, it } from "vitest";

import type { CrawlEvidence } from "./onboarding-machine";
import {
  DEFAULT_ONBOARDING_ACCENT,
  allOnboardingWebsites,
  appendCrawlSnippet,
  buildSafeConnectorMetadata,
  countWebsiteSources,
  displayOrganizationName,
  removeWebsiteFromState,
  resolveBrandThemeColor,
  upsertWebsiteInState,
} from "./onboarding-evidence";
import type { ConnectorPick, OnboardingState, WebsitePayload } from "./onboarding-machine";

const baseState = (overrides: Partial<OnboardingState> = {}): OnboardingState => ({
  step: "website",
  connectors: [],
  introPlayed: true,
  startedAt: 0,
  ...overrides,
});

function website(url: string): WebsitePayload {
  return { url, agentBrief: "", crawlEvidence: { status: "completed", pages: 1, elements: 8, snippets: [], contentTypes: [], warnings: [], lastUpdatedAt: "2026-06-01T00:00:00.000Z" } };
}

describe("onboarding website evidence", () => {
  it("keeps the first website as primary and adds later websites as additional", () => {
    const first = upsertWebsiteInState(baseState(), website("https://first.example"));
    const second = upsertWebsiteInState(first, website("https://second.example"));

    expect(second.website?.url).toBe("https://first.example");
    expect(second.additionalWebsites?.map((item) => item.url)).toEqual(["https://second.example"]);
    expect(allOnboardingWebsites(second).map((item) => item.url)).toEqual([
      "https://first.example",
      "https://second.example",
    ]);
    expect(countWebsiteSources(second)).toBe(2);
  });

  it("removes websites reversibly and promotes the next website when primary is removed", () => {
    const state = upsertWebsiteInState(
      upsertWebsiteInState(baseState({ website: website("https://first.example") }), website("https://second.example")),
      website("https://third.example"),
    );

    const withoutPrimary = removeWebsiteFromState(state, "https://first.example");
    expect(withoutPrimary.website?.url).toBe("https://second.example");
    expect(withoutPrimary.additionalWebsites?.map((item) => item.url)).toEqual(["https://third.example"]);

    const withoutAdditional = removeWebsiteFromState(withoutPrimary, "https://third.example");
    expect(allOnboardingWebsites(withoutAdditional).map((item) => item.url)).toEqual(["https://second.example"]);
  });

  it("caps and dedupes crawl snippets", () => {
    const evidence = Array.from({ length: 15 }).reduce<CrawlEvidence | undefined>(
      (current, _, index) =>
        appendCrawlSnippet(current, {
          id: `snippet-${index}`,
          kind: "text",
          title: `Page ${index}`,
          url: `https://example.com/${index}`,
          excerpt: "A public page excerpt",
        }),
      undefined,
    );
    const deduped = appendCrawlSnippet(evidence, {
      id: "dup",
      kind: "text",
      title: "Duplicate",
      url: "https://example.com/14",
    });

    expect(deduped.snippets).toHaveLength(12);
    expect(deduped.snippets.at(-1)?.url).toBe("https://example.com/14");
    expect(deduped.snippets.filter((item) => item.url === "https://example.com/14")).toHaveLength(1);
  });
});

describe("onboarding brand evidence", () => {
  it("chooses the first non-neutral brand color before falling back", () => {
    expect(
      resolveBrandThemeColor({
        themeColor: "#123456",
        palette: ["#abcdef"],
      }),
    ).toBe("#123456");
    expect(resolveBrandThemeColor({ themeColor: "#111111", palette: ["#000000", "#abcdef"] })).toBe("#abcdef");
    expect(resolveBrandThemeColor({ themeColor: "#111111", palette: ["#232458", "#4e60ad"] })).toBe("#232458");
    expect(resolveBrandThemeColor({ themeColor: "#232458", palette: ["#4e60ad"] })).toBe("#232458");
    expect(resolveBrandThemeColor({ siteName: "Aquatiq", themeColor: "#111111", palette: ["#000000"] })).toBe(
      DEFAULT_ONBOARDING_ACCENT,
    );
    expect(resolveBrandThemeColor(undefined)).toBe(DEFAULT_ONBOARDING_ACCENT);
  });

  it("normalizes loud organization names for paywall headings", () => {
    expect(displayOrganizationName("AQUATIQ AS")).toBe("Aquatiq AS");
    expect(displayOrganizationName("VEREVON")).toBe("Verevon");
    expect(displayOrganizationName("acme crm asa")).toBe("Acme CRM ASA");
  });
});

describe("safe connector metadata", () => {
  it("builds bounded metadata and strips sensitive-looking samples", () => {
    const connector: ConnectorPick = { id: "microsoft365", label: "Microsoft 365" };
    const metadata = buildSafeConnectorMetadata(connector, {
      sampleEntities: [
        "Intranet",
        "CEO salary.xlsx",
        "support@example.com",
        "Shared policies",
        "Q2 board deck",
      ],
      entityCounts: { sites: 4, files: 120, messages: 2500 },
      scopes: ["sharepoint", "outlook", "teams", "private-mailbox"],
      workspaceName: "Aquatiq",
    });

    expect(metadata.status).toBe("ready");
    expect(metadata.sensitivity).toBe("safe_metadata_only");
    expect(metadata.sampleEntities).toEqual(["Intranet", "Shared policies", "Q2 board deck"]);
    expect(metadata.entityCounts).toEqual({ sites: 4, files: 120 });
    expect(metadata.scopes).toEqual(["sharepoint", "outlook", "teams"]);
  });
});
