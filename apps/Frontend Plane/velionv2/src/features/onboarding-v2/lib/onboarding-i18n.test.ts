import { describe, expect, it } from "vitest";
import {
  formatOnboardingText,
  onboardingPlanName,
  onboardingStepLabel,
  ONBOARDING_COPY,
} from "./onboarding-i18n";

describe("onboarding i18n", () => {
  it("interpolates {{tokens}} including repeats", () => {
    expect(formatOnboardingText("Step {{current}} of {{total}}", { current: 2, total: 6 })).toBe(
      "Step 2 of 6",
    );
    expect(formatOnboardingText("{{x}}-{{x}}", { x: "a" })).toBe("a-a");
  });

  it("resolves localized plan names", () => {
    expect(onboardingPlanName("standard", "en")).toBe("Advanced");
    expect(onboardingPlanName("standard", "nb")).toBe("Advanced");
    expect(onboardingPlanName("trial", "nb")).toBe("Gratis");
  });

  it("resolves localized step labels", () => {
    expect(onboardingStepLabel("website", "nb")).toBe("Nettside");
    expect(onboardingStepLabel("website", "en")).toBe("Website");
  });

  it("keeps nb and en plan catalogs in lockstep", () => {
    expect(Object.keys(ONBOARDING_COPY.nb.paywall.plans)).toEqual(
      Object.keys(ONBOARDING_COPY.en.paywall.plans),
    );
  });
});
