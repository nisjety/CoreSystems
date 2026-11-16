import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// W3-DEC-ORGAGENT (owner decision D19): org owners and admins suspend, reactivate and retire the
// org-visible agents, which are ownerless since W3-DEC-CTL1. Control (user-core) owns the agent
// rows, the canonical owner-or-admin check, the room cascade and the audit row. The gateway only
// proxies (apps/gateway/src/domains/orgs/org_agents.rs, dispatchers_spaces/org_agents.rs). None of
// the three is model-executable (model-access.ts closes them): an agent must not switch off, or
// switch back on, the organization's own agents. Each change carries the revision the admin read,
// and a stale revision is refused with agent_state_changed.

const orgAgentIdInput = z.string().regex(/^agent-[A-Za-z0-9_-]{6,64}$/)

const orgAgentChangeInput = z.object({
  orgId: z.string().min(1),
  agentSubjectId: orgAgentIdInput,
  expectedRevision: z.number().int().min(0),
})

const orgAgentChangeOutput = z.object({
  status: z.string(),
})

export const ws17OrgAgentActions = [
  {
    id: 'spaces.suspend_org_agent',
    label: 'Suspend organization agent',
    description:
      'Suspend an organization agent. It is taken out of every room it sits in, and no room can add it until an organization owner or admin reactivates it. Org owner or admin only.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: orgAgentChangeInput,
    outputSchema: orgAgentChangeOutput,
  },
  {
    id: 'spaces.reactivate_org_agent',
    label: 'Reactivate organization agent',
    description:
      'Reactivate a suspended organization agent. Rooms do not get it back on their own: a room manager adds it again. Org owner or admin only.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: true,
    reversible: true,
    inputSchema: orgAgentChangeInput,
    outputSchema: orgAgentChangeOutput,
  },
  {
    id: 'spaces.retire_org_agent',
    label: 'Retire organization agent',
    description:
      'Retire an organization agent for good. It is withdrawn from every room and kept for audit, and it cannot be reactivated. Org owner or admin only.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: false,
    inputSchema: orgAgentChangeInput,
    outputSchema: orgAgentChangeOutput,
  },
] as const satisfies readonly ActionDescriptor[]
