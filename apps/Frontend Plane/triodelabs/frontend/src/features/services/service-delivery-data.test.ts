import { existsSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { serviceDeliveries } from "./service-delivery-data";
import { consultancySeo } from "./service-seo";

describe("service delivery navigation", () => {
  it("keeps all contextual links crawlable and pointing to actual service routes", () => {
    for (const [canonical, delivery] of Object.entries(serviceDeliveries)) {
      expect(delivery.related.length).toBeGreaterThan(0);

      for (const link of delivery.related) {
        expect(link.href).not.toBe(canonical);
        expect(serviceDeliveries[link.href]).toBeDefined();
        expect(
          existsSync(
            new URL(`../../app/(nb)${link.href}/page.tsx`, import.meta.url),
          ),
        ).toBe(true);
      }
    }
  });

  it("makes the new consultancy page reachable from relevant services", () => {
    const inboundPages = Object.entries(serviceDeliveries)
      .filter(([, delivery]) =>
        delivery.related.some((link) => link.href === consultancySeo.canonical),
      )
      .map(([canonical]) => canonical);

    expect(inboundPages).toContain("/tjenester/web-losninger");
    expect(inboundPages).toContain("/tjenester/ki-og-automatisering");
  });
});
