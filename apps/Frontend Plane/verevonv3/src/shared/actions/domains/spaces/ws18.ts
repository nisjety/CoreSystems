import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// WS18-S5: the binding switch for agent self-API, the room agent's own thread tools (rename, pin, and later
// post a card). Application (convex-core `spaceAgents`) stores `selfApiEnabled` on the binding; the gateway
// gates owner or manager from Control's live role (apps/gateway/src/domains/spaces/self_api.rs and
// dispatchers_spaces/ws18.rs). The switch grants nothing: Control's run-action decision still decides each
// tool. The id is not model-executable (model-access.ts closes it): a room agent may not turn its own tools on.

const spaceSetAgentSelfApiInput = z.object({
  spaceRef: z.string().min(1),
  bindingRef: z.string().min(1),
  // Whether this bound agent may act on the thread it runs in. Off is the default for every binding.
  enabled: z.boolean(),
})

const spaceSetAgentSelfApiOutput = z.object({
  binding_ref: z.string(),
  self_api_enabled: z.boolean(),
  projection_version: z.number().nullable(),
  changed: z.boolean(),
})

export const ws18SpaceActions = [
  {
    id: 'spaces.set_agent_self_api',
    label: 'Let a Space agent act on its own thread',
    description:
      'Choose whether one bound agent in this room may rename and pin the thread it runs in, and later post cards. Only the room\'s owner or a manager may change it. It grants nothing on its own: each action still needs Control\'s decision for that run.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceSetAgentSelfApiInput,
    outputSchema: spaceSetAgentSelfApiOutput,
  },
] as const satisfies readonly ActionDescriptor[]
