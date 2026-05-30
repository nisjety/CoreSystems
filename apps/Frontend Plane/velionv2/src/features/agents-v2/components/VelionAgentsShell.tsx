"use client";

import type { ReactNode } from "react";
import { VelionProductShell } from "@/features/shell-v2/components/VelionProductShell";
import { AgentSelectionInitialProvider } from "@/features/agents-v2/lib/use-agent-selection";
import { defaultAgentSelectionId, type AgentSelectionId } from "@/features/agents-v2/lib/agent-roles";

export function VelionAgentsShell({
  children,
  initialAgentSelection = defaultAgentSelectionId,
}: {
  children: ReactNode;
  initialAgentSelection?: AgentSelectionId;
}) {
  return (
    <AgentSelectionInitialProvider selection={initialAgentSelection}>
      <VelionAgentsShellContent>{children}</VelionAgentsShellContent>
    </AgentSelectionInitialProvider>
  );
}

function VelionAgentsShellContent({ children }: { children: ReactNode }) {
  return (
    <VelionProductShell
      activeRoute="/agents"
      defaultSidebarExpanded
    >
      {children}
    </VelionProductShell>
  );
}
