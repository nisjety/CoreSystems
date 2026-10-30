import { describe, expect, it } from "vitest";
import { commercePrinciples, commerceProcess } from "./commerce-page-data";

describe("commerce service content", () => {
  it("covers both the purchase experience and operational workflow", () => {
    expect(commerceProcess.map((step) => step.title)).toEqual([
      "Forretning og sortiment",
      "Plattform og integrasjoner",
      "Kjøpsopplevelse",
      "Betaling, frakt og retur",
      "Innhold og synlighet",
      "Måling og forbedring",
    ]);
  });

  it("keeps the three commerce principles distinct", () => {
    expect(commercePrinciples).toHaveLength(3);
    expect(
      new Set(commercePrinciples.map((principle) => principle.title)).size,
    ).toBe(3);
  });
});
