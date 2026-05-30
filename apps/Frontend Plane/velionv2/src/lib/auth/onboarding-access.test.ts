import { describe, expect, it } from "vitest";
import { isCompletedOnboardingContext } from "@/lib/auth/onboarding-access";

describe("onboarding access", () => {
  it("treats user-core COMPLETED as onboarding complete", () => {
    expect(isCompletedOnboardingContext({ onboardingStatus: "COMPLETED" })).toBe(true);
  });

  it("treats pending connector status as incomplete", () => {
    expect(isCompletedOnboardingContext({ onboardingStatus: "CONNECTORS_PENDING" })).toBe(false);
  });

  it("accepts legacy onboarding_complete flags from user-core", () => {
    expect(isCompletedOnboardingContext({ onboarding_complete: true })).toBe(true);
  });
});
