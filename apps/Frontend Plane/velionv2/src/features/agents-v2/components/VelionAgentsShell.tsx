"use client";

import type { ReactNode } from "react";
import { VerevonProductShell } from "@/features/shell-v2/components/VerevonProductShell";
import { AgentSelectionInitialProvider } from "@/features/agents-v2/lib/use-agent-selection";
import { defaultAgentSelectionId, type AgentSelectionId } from "@/features/agents-v2/lib/agent-roles";

export function VerevonAgentsShell({
  children,
  initialAgentSelection = defaultAgentSelectionId,
}: {
  children: ReactNode;
  initialAgentSelection?: AgentSelectionId;
}) {
  return (
    <AgentSelectionInitialProvider selection={initialAgentSelection}>
      <VerevonAgentsShellContent>{children}</VerevonAgentsShellContent>
    </AgentSelectionInitialProvider>
  );
}

function VerevonAgentsShellContent({ children }: { children: ReactNode }) {
  return (
    <VerevonProductShell
      activeRoute="/agents"
      defaultSidebarExpanded
    >
      {children}
    </VerevonProductShell>
  );
}
