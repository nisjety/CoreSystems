import type { Metadata } from "next";
import { VelionSettingsPage } from "@/features/settings-v2/components/VelionSettingsPage";
import { VelionProductShell } from "@/features/shell-v2/components/VelionProductShell";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Account | Velion v2",
  description: "Your Velion profile and account settings.",
};

export default async function AccountPage() {
  await requireCompletedOnboarding("/account");

  return (
    <VelionProductShell activeRoute="/account" defaultSidebarExpanded>
      <VelionSettingsPage />
    </VelionProductShell>
  );
}
