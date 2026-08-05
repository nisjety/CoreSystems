import type { Metadata } from "next";
import { Suspense } from "react";
import { VerevonInboxPage } from "@/features/inbox-v2/components/VerevonInboxPage";
import { VerevonProductShell } from "@/features/shell-v2/components/VerevonProductShell";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Inbox | Verevon v2",
  description: "Verevon omnichannel inbox.",
};

export default async function InboxSectionPage({
  params,
}: {
  params: Promise<{ slug?: string[] }>;
}) {
  await requireCompletedOnboarding("/inbox");

  const { slug = [] } = await params;

  return (
    <Suspense fallback={<InboxPageFallback />}>
      <VerevonProductShell activeRoute="/inbox" defaultSidebarExpanded expandedSidebarWidth={356}>
        <VerevonInboxPage routeSlug={slug} />
      </VerevonProductShell>
    </Suspense>
  );
}

function InboxPageFallback() {
  return <div className="h-dvh bg-[#FCFCFD] dark:bg-[#101114]" />;
}
