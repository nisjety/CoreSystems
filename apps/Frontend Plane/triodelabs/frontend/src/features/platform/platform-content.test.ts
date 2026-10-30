import { describe, expect, it } from "vitest";
import { platformProducts } from "./platform-content";

describe("platform content", () => {
  it("only presents the two deployed TriodeLab products", () => {
    expect(platformProducts).toHaveLength(2);
    expect(platformProducts.map((product) => product.name)).toEqual([
      "Verevon",
      "Agenci",
    ]);
    expect(
      platformProducts.every((product) => product.status === "Abonnement"),
    ).toBe(true);
    expect(platformProducts.map((product) => product.href)).toEqual([
      "https://www.verevon.com/",
      "https://agenci.no/",
    ]);
  });
});
