import type { Metadata } from "next";
import { VerevonHome } from "@/features/dashboard-v2/components/VerevonHome";
import { VerevonProductShell } from "@/features/shell-v2/components/VerevonProductShell";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Verevon Home | Verevon v2",
  description: "Verevon home dashboard and control cockpit.",
};

export default async function DashboardPage() {
  await requireCompletedOnboarding("/dashboard");

  return (
    <VerevonProductShell activeRoute="/dashboard">
      <VerevonHome />
    </VerevonProductShell>
  );
}
