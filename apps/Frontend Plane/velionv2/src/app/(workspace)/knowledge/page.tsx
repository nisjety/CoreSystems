import type { Metadata } from "next";
import { VelionKnowledgePage } from "@/features/knowledge-v2/components/VelionKnowledgePage";
import { VelionProductShell } from "@/features/shell-v2/components/VelionProductShell";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Knowledge | Velion v2",
  description: "Velion knowledge operations.",
};

export default async function KnowledgePage() {
  await requireCompletedOnboarding("/knowledge");

  return (
    <VelionProductShell activeRoute="/knowledge" defaultSidebarExpanded>
      <VelionKnowledgePage />
    </VelionProductShell>
  );
}
