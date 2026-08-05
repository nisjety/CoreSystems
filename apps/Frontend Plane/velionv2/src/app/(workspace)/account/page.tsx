import type { Metadata } from "next";
import { VerevonSettingsPage } from "@/features/settings-v2/components/VerevonSettingsPage";
import { VerevonProductShell } from "@/features/shell-v2/components/VerevonProductShell";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Account | Verevon v2",
  description: "Your Verevon profile and account settings.",
};

export default async function AccountPage() {
  await requireCompletedOnboarding("/account");

  return (
    <VerevonProductShell activeRoute="/account" defaultSidebarExpanded>
      <VerevonSettingsPage />
    </VerevonProductShell>
  );
}
