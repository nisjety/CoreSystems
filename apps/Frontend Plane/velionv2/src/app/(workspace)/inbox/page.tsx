import type { Metadata } from "next";
import { Suspense } from "react";
import { VelionInboxPage } from "@/features/inbox-v2/components/VelionInboxPage";
import { VelionProductShell } from "@/features/shell-v2/components/VelionProductShell";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Inbox | Velion v2",
  description: "Velion omnichannel inbox.",
};

export default async function InboxPage() {
  await requireCompletedOnboarding("/inbox");

  return (
    <Suspense fallback={<InboxPageFallback />}>
      <VelionProductShell activeRoute="/inbox" defaultSidebarExpanded expandedSidebarWidth={356}>
        <VelionInboxPage />
      </VelionProductShell>
    </Suspense>
  );
}

function InboxPageFallback() {
  return <div className="h-dvh bg-[#FCFCFD] dark:bg-[#101114]" />;
}
