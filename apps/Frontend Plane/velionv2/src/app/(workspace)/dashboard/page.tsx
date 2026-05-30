import type { Metadata } from "next";
import { VelionHome } from "@/features/dashboard-v2/components/VelionHome";
import { VelionProductShell } from "@/features/shell-v2/components/VelionProductShell";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Velion Home | Velion v2",
  description: "Velion home dashboard and control cockpit.",
};

export default async function DashboardPage() {
  await requireCompletedOnboarding("/dashboard");

  return (
    <VelionProductShell activeRoute="/dashboard">
      <VelionHome />
    </VelionProductShell>
  );
}
