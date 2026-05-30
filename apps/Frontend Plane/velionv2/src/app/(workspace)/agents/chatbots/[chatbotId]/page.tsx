import type { Metadata } from "next";
import { VelionAgentsShell } from "@/features/agents-v2/components/VelionAgentsShell";
import { VelionChatbotStudio } from "@/features/agents-v2/components/VelionChatbotStudio";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Edit Chatbot | Velion v2",
  description: "Edit a Velion chatbot agent.",
};

export default async function EditChatbotPage() {
  await requireCompletedOnboarding("/agents");

  return (
    <VelionAgentsShell initialAgentSelection="chatbot">
      <VelionChatbotStudio />
    </VelionAgentsShell>
  );
}
