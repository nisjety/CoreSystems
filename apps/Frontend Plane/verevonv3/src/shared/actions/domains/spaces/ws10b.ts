import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// WS10-S5b: the per-tool policy of a shared Space. Control (`space_tool_policies`) stores two
// layers, the Space ceiling and one layer per agent, and merges them tighten-only (allow < ask <
// block). The gateway is apps/gateway/src/domains/spaces/tool_policy.rs, the dispatcher
// dispatchers_spaces/ws10b.rs. One action writes either layer: without `bindingRef` it replaces
// the Space ceiling, with it that agent's layer. Control decides who may (a Space manager or
// owner) and compares `expectedRevision` per layer (409 `tool_policy_revision_conflict`). The id
// is not model-executable (model-access.ts closes it): an agent must never edit the policy that
// limits it.

const toolPolicyEntry = z.object({
  toolKey: z.string().min(1).max(200),
  level: z.enum(['allow', 'ask', 'block']),
})

const spaceSetToolPolicyInput = z.object({
  spaceRef: z.string().min(1),
  // Present: replace this agent's layer. Absent: replace the Space ceiling (every agent).
  bindingRef: z.string().min(1).optional(),
  // The whole layer; "*" is the key for every tool without its own row. At most 200 entries.
  entries: z.array(toolPolicyEntry).max(200),
  // The layer revision the caller last read; 0 for a layer never written.
  expectedRevision: z.number().int().min(0),
})

const spaceSetToolPolicyOutput = z.object({
  policy: z.record(z.string(), z.unknown()),
  merged: z.record(z.string(), z.unknown()),
  binding_ref: z.string().optional(),
  projection: z.enum(['synced', 'pending']).optional(),
})

export const ws10bSpaceActions = [
  {
    id: 'spaces.set_tool_policy',
    label: 'Set a Space tool policy',
    description:
      'Choose, per tool, whether a call is allowed, asks for approval or is blocked: for every agent of this room (the Space ceiling) or for one bound agent. A policy only tightens: it never lets a call past a default gate, and the stricter layer wins. Only the room\'s owner or manager may change it.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceSetToolPolicyInput,
    outputSchema: spaceSetToolPolicyOutput,
  },
] as const satisfies readonly ActionDescriptor[]
