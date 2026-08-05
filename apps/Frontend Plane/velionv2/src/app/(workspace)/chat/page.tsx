import type { Metadata } from "next";
import { VerevonChatPage } from "@/features/chat-v2/components/VerevonChatPage";
import { VerevonChatWorkspaceProvider } from "@/features/chat-v2/lib/chat-workspace";
import { VerevonProductShell } from "@/features/shell-v2/components/VerevonProductShell";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Verevon Chat | Verevon v2",
  description: "Verevon agent workspace.",
};

export default async function ChatPage() {
  await requireCompletedOnboarding("/chat");

  return (
    <VerevonChatWorkspaceProvider>
      <VerevonProductShell activeRoute="/chat">
        <VerevonChatPage />
      </VerevonProductShell>
    </VerevonChatWorkspaceProvider>
  );
}
