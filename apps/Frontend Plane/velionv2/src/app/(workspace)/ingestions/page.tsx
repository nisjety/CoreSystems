import type { Metadata } from "next";
import { VerevonIngestionsPage } from "@/features/ingestions-v2/components/VerevonIngestionsPage";
import { VerevonProductShell } from "@/features/shell-v2/components/VerevonProductShell";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Ingestions | Verevon v2",
  description: "Verevon ingestion operations workspace.",
};

export default async function IngestionsPage() {
  await requireCompletedOnboarding("/ingestions");

  return (
    <VerevonProductShell activeRoute="/ingestions" defaultSidebarExpanded>
      <VerevonIngestionsPage />
    </VerevonProductShell>
  );
}
