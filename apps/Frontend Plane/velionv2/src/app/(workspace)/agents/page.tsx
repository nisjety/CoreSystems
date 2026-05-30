import type { Metadata } from "next";
import { VelionAgentsPage } from "@/features/agents-v2/components/VelionAgentsPage";
import { VelionAgentsShell } from "@/features/agents-v2/components/VelionAgentsShell";
import { getAgentSelectionFromSearch } from "@/features/agents-v2/lib/agent-roles";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Agents | Velion v2",
  description: "Velion agent configuration.",
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
    <VelionAgentsShell initialAgentSelection={initialAgentSelection}>
      <VelionAgentsPage />
    </VelionAgentsShell>
  );
}
