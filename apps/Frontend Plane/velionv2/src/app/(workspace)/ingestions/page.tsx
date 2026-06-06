import type { Metadata } from "next";
import { VelionIngestionsPage } from "@/features/ingestions-v2/components/VelionIngestionsPage";
import { VelionProductShell } from "@/features/shell-v2/components/VelionProductShell";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Ingestions | Velion v2",
  description: "Velion ingestion operations workspace.",
};

export default async function IngestionsPage() {
  await requireCompletedOnboarding("/ingestions");

  return (
    <VelionProductShell activeRoute="/ingestions" defaultSidebarExpanded>
      <VelionIngestionsPage />
    </VelionProductShell>
  );
}
