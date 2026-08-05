import { notFound } from 'next/navigation';
import { AgentWorkspaceView } from '@/components/agents/AgentWorkspaceView';
import { getAgentById, MOCK_AGENT_IDS } from '@/components/agents/data';
import { SidebarSectionPage } from '@/components/dashboard/SidebarSectionPage';
import { getSidebarSectionPage } from '@/components/dashboard/sidebar-sections';
import { convexQuery } from '@/app/api/_lib/convex-client';
import { resolveChatActor } from '@/app/api/chat/_lib/session-store';
import type { PersistedAgent } from '@/components/agents/types';

const VALID_WORKSPACE_SLUGS = new Set([
  'playground',
  'train',
  'test',
  'deploy',
  'analyze',
  'changelog',
  'settings',
  'workflows',
  'automations',
]);
const RESERVED_AGENT_SECTION_SLUGS = new Set(['settings', 'actions']);

export default async function AgentWorkspacePage({
  params,
}: {
  params: Promise<{ agentId: string; slug?: string[] }>;
}) {
  const { agentId, slug } = await params;

  if (RESERVED_AGENT_SECTION_SLUGS.has(agentId)) {
    return <SidebarSectionPage {...getSidebarSectionPage('agents', [agentId])} />;
  }

  const viewId = slug?.[0];
  if (viewId && !VALID_WORKSPACE_SLUGS.has(viewId)) {
    notFound();
  }

  // Try to load persisted agent from Convex first.
  let persistedAgent: PersistedAgent | null = null
  try {
    const actor = await resolveChatActor()
    persistedAgent = await convexQuery<PersistedAgent | null>('agents:getById', {
      agentId,
      orgId: actor.convexOrgId,
    })
  } catch {
    // Convex unreachable or auth failed — let the next branch decide.
  }

  if (persistedAgent) {
    return <AgentWorkspaceView agent={persistedAgent} viewId={viewId} />
  }

  // Wave 8 (ui-ux-verevon-gap.md §18): only fall back to MOCK_AGENTS
  // when the URL points at a *known demo id* (used by the seed flow
  // and design previews). Any other unknown id is a real 404 — we no
  // longer silently render fixtures to mask a broken lookup.
  if (MOCK_AGENT_IDS.has(agentId)) {
    const mockAgent = getAgentById(agentId)
    if (mockAgent) {
      return <AgentWorkspaceView agent={mockAgent} viewId={viewId} />
    }
  }

  notFound()
}
