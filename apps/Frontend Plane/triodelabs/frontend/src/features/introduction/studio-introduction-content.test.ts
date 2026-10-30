import { describe, expect, it } from "vitest";
import { studioIntroduction } from "./studio-introduction-content";

describe("studio introduction content", () => {
  it("gives the carousel a distinct follow-up headline", () => {
    expect("eyebrow" in studioIntroduction).toBe(false);
    expect(studioIntroduction.heading).toContain("retningen");
    expect(studioIntroduction.paragraphs).toHaveLength(2);
  });

  it("keeps the introduction focused on strategy, design and technology", () => {
    expect(studioIntroduction.paragraphs.join(" ")).toMatch(
      /strategi, design og teknologi/i,
    );
  });
});
