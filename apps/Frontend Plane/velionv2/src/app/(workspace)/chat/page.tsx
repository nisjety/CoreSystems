import type { Metadata } from "next";
import { VelionChatPage } from "@/features/chat-v2/components/VelionChatPage";
import { VelionChatWorkspaceProvider } from "@/features/chat-v2/lib/chat-workspace";
import { VelionProductShell } from "@/features/shell-v2/components/VelionProductShell";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Velion Chat | Velion v2",
  description: "Velion agent workspace.",
};

export default async function ChatPage() {
  await requireCompletedOnboarding("/chat");

  return (
    <VelionChatWorkspaceProvider>
      <VelionProductShell activeRoute="/chat">
        <VelionChatPage />
      </VelionProductShell>
    </VelionChatWorkspaceProvider>
  );
}
