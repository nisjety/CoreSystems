import type { Metadata } from "next";
import { VerevonAgentsShell } from "@/features/agents-v2/components/VerevonAgentsShell";
import { VerevonChatbotStudio } from "@/features/agents-v2/components/VerevonChatbotStudio";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Edit Chatbot | Verevon v2",
  description: "Edit a Verevon chatbot agent.",
};

export default async function EditChatbotPage() {
  await requireCompletedOnboarding("/agents");

  return (
    <VerevonAgentsShell initialAgentSelection="chatbot">
      <VerevonChatbotStudio />
    </VerevonAgentsShell>
  );
}
