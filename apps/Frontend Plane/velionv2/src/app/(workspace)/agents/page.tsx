import type { Metadata } from "next";
import { VerevonAgentsPage } from "@/features/agents-v2/components/VerevonAgentsPage";
import { VerevonAgentsShell } from "@/features/agents-v2/components/VerevonAgentsShell";
import { getAgentSelectionFromSearch } from "@/features/agents-v2/lib/agent-roles";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Agents | Verevon v2",
  description: "Verevon agent configuration.",
};

export default async function AgentsPage({
  searchParams,
}: {
  searchParams: Promise<{ agent?: string | string[] }>;
}) {
  await requireCompletedOnboarding("/agents");

  const { agent } = await searchParams;
  const agentParam = Array.isArray(agent) ? agent[0] : agent;
  const initialAgentSelection = getAgentSelectionFromSearch(
    agentParam ? new URLSearchParams({ agent: agentParam }).toString() : "",
  );

  return (
    <VerevonAgentsShell initialAgentSelection={initialAgentSelection}>
      <VerevonAgentsPage />
    </VerevonAgentsShell>
  );
}
