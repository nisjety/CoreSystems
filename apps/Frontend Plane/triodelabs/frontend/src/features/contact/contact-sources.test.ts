import { describe, expect, it } from "vitest";
import {
  contactSources,
  getContactSlugForSource,
  getContactSource,
  getContactSourceFromSlug,
  getProjectTypeForSource,
} from "./contact-sources";
import { contactFormSchema } from "../../lib/validations";

describe("contact service attribution", () => {
  it("maps every known service to a valid project type", () => {
    for (const source of contactSources) {
      expect(getContactSource(source)).toBe(source);
      expect(
        contactFormSchema.shape.projectType.safeParse(
          getProjectTypeForSource(source),
        ).success,
      ).toBe(true);
    }
    expect(getProjectTypeForSource("radgivning-og-konsulentbistand")).toBe(
      "konsultasjon",
    );
    expect(getProjectTypeForSource("ki-og-automatisering")).toBe("ki");
  });

  it("does not retain unknown or personal values from query parameters", () => {
    for (const value of [
      undefined,
      null,
      "person@example.test",
      "invalid",
      ["integrasjoner"],
    ]) {
      expect(getContactSource(value)).toBe("direct");
    }
  });

  it("attributes English service links to the same source", () => {
    expect(getContactSourceFromSlug("ai-and-automation", "en")).toBe(
      "ki-og-automatisering",
    );
    expect(getContactSourceFromSlug("ki-og-automatisering", "en")).toBe(
      "ki-og-automatisering",
    );
    expect(getContactSourceFromSlug("person@example.test", "en")).toBe(
      "direct",
    );
    expect(getContactSourceFromSlug("nettbutikk", "nb")).toBe("nettbutikk");
    expect(getContactSourceFromSlug(["ecommerce"], "en")).toBe("direct");

    for (const source of contactSources.filter((value) => value !== "direct")) {
      const slug = getContactSlugForSource(source, "en");
      expect(slug).not.toBe(source);
      expect(getContactSourceFromSlug(slug, "en")).toBe(source);
      expect(getContactSlugForSource(source, "nb")).toBe(source);
    }
  });
});
