import type { Metadata } from "next";
import { VelionOnboardingPage } from "@/features/onboarding-v2/components/VelionOnboardingPage";
import { requireOnboardingAccess } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Onboarding | Velion v2",
  description: "Velion zero-input onboarding flow.",
};

export default async function OnboardingPage() {
  await requireOnboardingAccess();

  return <VelionOnboardingPage />;
}
