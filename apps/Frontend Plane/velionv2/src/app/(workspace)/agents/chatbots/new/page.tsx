import type { Metadata } from "next";
import { VerevonAgentsShell } from "@/features/agents-v2/components/VerevonAgentsShell";
import { VerevonChatbotStudio } from "@/features/agents-v2/components/VerevonChatbotStudio";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Create Chatbot | Verevon v2",
  description: "Create a Verevon chatbot agent.",
};

export default async function CreateChatbotPage() {
  await requireCompletedOnboarding("/agents/chatbots/new");

  return (
    <VerevonAgentsShell initialAgentSelection="chatbot">
      <VerevonChatbotStudio />
    </VerevonAgentsShell>
  );
}
