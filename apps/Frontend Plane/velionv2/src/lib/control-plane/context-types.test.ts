import { describe, it, expect } from "vitest";
import {
  hasFeature,
  ANONYMOUS_CONTROL_PLANE_CONTEXT,
  type ControlPlaneContextValue,
} from "@/lib/control-plane/context-types";

// ---------------------------------------------------------------------------
// ANONYMOUS_CONTROL_PLANE_CONTEXT shape
// ---------------------------------------------------------------------------
describe("ANONYMOUS_CONTROL_PLANE_CONTEXT", () => {
  it("has null user", () => {
    expect(ANONYMOUS_CONTROL_PLANE_CONTEXT.user).toBeNull();
  });

  it("has null orgId", () => {
    expect(ANONYMOUS_CONTROL_PLANE_CONTEXT.orgId).toBeNull();
  });

  it("has null organization", () => {
    expect(ANONYMOUS_CONTROL_PLANE_CONTEXT.organization).toBeNull();
  });

  it("has null role", () => {
    expect(ANONYMOUS_CONTROL_PLANE_CONTEXT.role).toBeNull();
  });

  it("has null onboardingStatus", () => {
    expect(ANONYMOUS_CONTROL_PLANE_CONTEXT.onboardingStatus).toBeNull();
  });

  it("has onboardingComplete false", () => {
    expect(ANONYMOUS_CONTROL_PLANE_CONTEXT.onboardingComplete).toBe(false);
  });

  it("has null entitlements", () => {
    expect(ANONYMOUS_CONTROL_PLANE_CONTEXT.entitlements).toBeNull();
  });

  it("has null appearance", () => {
    expect(ANONYMOUS_CONTROL_PLANE_CONTEXT.appearance).toBeNull();
  });

  it("has null navbar", () => {
    expect(ANONYMOUS_CONTROL_PLANE_CONTEXT.navbar).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// hasFeature
// ---------------------------------------------------------------------------
describe("hasFeature", () => {
  const baseCtx: ControlPlaneContextValue = {
    user: { id: "user-1" },
    orgId: "org-1",
    organization: {
      id: "org-1",
      name: "Acme AS",
      plan: "pro",
    },
    role: "admin",
    onboardingStatus: "complete",
    onboardingComplete: true,
    entitlements: {
      plan: "pro",
      subscriptionStatus: "active",
      features: { "feature.sso": true, "feature.audit": false },
      quotas: { documents: 1000 },
      credits: 100,
    },
    appearance: {
      theme: "system",
      colorScheme: null,
    },
    navbar: null,
  };

  it("returns true for a granted feature flag", () => {
    expect(hasFeature(baseCtx, "feature.sso")).toBe(true);
  });

  it("returns false for an explicitly false feature flag", () => {
    expect(hasFeature(baseCtx, "feature.audit")).toBe(false);
  });

  it("returns false for a missing feature flag", () => {
    expect(hasFeature(baseCtx, "feature.nonexistent")).toBe(false);
  });

  it("returns false when entitlements is null", () => {
    const ctx: ControlPlaneContextValue = {
      ...ANONYMOUS_CONTROL_PLANE_CONTEXT,
      entitlements: null,
    };
    expect(hasFeature(ctx, "feature.sso")).toBe(false);
  });

  it("returns false when features map is empty", () => {
    const ctx: ControlPlaneContextValue = {
      ...baseCtx,
      entitlements: {
        plan: "free",
        subscriptionStatus: "active",
        features: {},
        quotas: {},
        credits: 0,
      },
    };
    expect(hasFeature(ctx, "feature.sso")).toBe(false);
  });
});
