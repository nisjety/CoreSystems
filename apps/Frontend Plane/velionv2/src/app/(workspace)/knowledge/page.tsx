import type { Metadata } from "next";
import { VerevonKnowledgePage } from "@/features/knowledge-v2/components/VerevonKnowledgePage";
import { VerevonProductShell } from "@/features/shell-v2/components/VerevonProductShell";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Knowledge | Verevon v2",
  description: "Verevon knowledge operations.",
};

export default async function KnowledgePage() {
  await requireCompletedOnboarding("/knowledge");

  return (
    <VerevonProductShell activeRoute="/knowledge" defaultSidebarExpanded>
      <VerevonKnowledgePage />
    </VerevonProductShell>
  );
}
