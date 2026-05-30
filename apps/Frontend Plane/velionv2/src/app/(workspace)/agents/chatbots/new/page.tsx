import type { Metadata } from "next";
import { VelionAgentsShell } from "@/features/agents-v2/components/VelionAgentsShell";
import { VelionChatbotStudio } from "@/features/agents-v2/components/VelionChatbotStudio";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Create Chatbot | Velion v2",
  description: "Create a Velion chatbot agent.",
};

export default async function CreateChatbotPage() {
  await requireCompletedOnboarding("/agents/chatbots/new");

  return (
    <VelionAgentsShell initialAgentSelection="chatbot">
      <VelionChatbotStudio />
    </VelionAgentsShell>
  );
}
