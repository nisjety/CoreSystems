import { describe, expect, it } from "vitest";
import { computeReadinessScore } from "@/lib/quality/score";

describe("computeReadinessScore", () => {
  it("normalizes capability scores against targets", () => {
    const score = computeReadinessScore([
      { name: "streaming", current: 90, target: 100 },
      { name: "virtualization", current: 100, target: 100 },
      { name: "drafts", current: 45, target: 90 },
    ]);

    expect(score).toBe(80);
  });

  it("handles empty score sets", () => {
    expect(computeReadinessScore([])).toBe(0);
  });
});
