import { describe, expect, it, vi } from "vitest";
import { createConsentTracking, type TrackingPort } from "./consent-tracking";
import { triodelabTrackingConfig } from "./site-tracking-config";

describe("Triodelab's consent-gated GA4 and Clarity installation", () => {
  it("keeps the actual stream disabled unless activation is explicit", () => {
    expect(triodelabTrackingConfig()).toEqual({});
    expect(triodelabTrackingConfig({ enabled: "false" })).toEqual({});
    expect(triodelabTrackingConfig({ enabled: "TRUE" })).toEqual({});
  });

  it("selects the public GA4 and Clarity IDs only after activation", () => {
    expect(triodelabTrackingConfig({ enabled: "true" })).toEqual({
      measurementId: "G-ZEWPHF6X3K",
      clarityProjectId: "z5gsvus553",
    });
  });

  it("supports an explicit valid deployment override", () => {
    expect(
      triodelabTrackingConfig({
        enabled: "true",
        measurementId: "G-OVERRIDE12",
        clarityProjectId: "test123456",
      }),
    ).toEqual({
      measurementId: "G-OVERRIDE12",
      clarityProjectId: "test123456",
    });
  });

  it.each(["", "invalid", "G-X<script>"])(
    "does not silently replace an invalid or deliberately empty override: %s",
    (measurementId) => {
      expect(
        triodelabTrackingConfig({
          enabled: "true",
          measurementId,
          clarityProjectId: "",
        }),
      ).toEqual({});
    },
  );

  it("requires Statistics consent before requesting either public script", () => {
    let statistics = false;
    const port: TrackingPort = {
      url: () => "https://www.triodelab.no/",
      referrer: () => "",
      statistics: () => statistics,
      privacySignal: () => false,
      loadScript: vi.fn(),
      google: vi.fn(),
      clarity: vi.fn(),
      disableGoogle: vi.fn(),
      clearCookies: vi.fn(),
      reload: vi.fn(),
    };
    const runtime = createConsentTracking(
      triodelabTrackingConfig({ enabled: "true" }),
      port,
    );
    runtime.reconcile();
    expect(port.loadScript).not.toHaveBeenCalled();
    statistics = true;
    runtime.reconcile();
    expect(port.loadScript).toHaveBeenCalledWith(
      "triodelab-ga4",
      "https://www.googletagmanager.com/gtag/js?id=G-ZEWPHF6X3K",
    );
    expect(port.loadScript).toHaveBeenCalledWith(
      "triodelab-clarity",
      "https://www.clarity.ms/tag/z5gsvus553",
    );
  });
});
