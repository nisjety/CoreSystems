import type { Metadata } from "next";
import { VerevonOnboardingPage } from "@/features/onboarding-v2/components/VerevonOnboardingPage";
import { requireOnboardingAccess } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Onboarding | Verevon v2",
  description: "Verevon zero-input onboarding flow.",
};

export default async function OnboardingPage() {
  await requireOnboardingAccess();

  return <VerevonOnboardingPage />;
}
