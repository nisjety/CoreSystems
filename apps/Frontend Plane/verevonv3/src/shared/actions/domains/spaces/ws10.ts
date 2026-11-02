import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// WS10-S4: deciding a Space approval and revoking a standing approval. The decision is recorded by
// Model Plane (only the member who started the run can record it, and the approved action runs as
// that member); a standing rule ("always") lives in Control and covers MCP tools only. The gateway
// is apps/gateway/src/domains/spaces/approvals.rs, the dispatchers dispatchers_spaces/ws10.rs.
// Neither id is model-executable (model-access.ts closes both): an agent must never decide its own
// approval or revoke the rule that limits it. `spaces.set_tool_policy` (WS10-S5b) and
// `spaces.reroute_approval` (WS10-S6) are registered with the routes they call.

const spaceDecideApprovalInput = z.object({
  spaceRef: z.string().min(1),
  approvalId: z.string().min(1),
  decision: z.enum(['approve', 'reject']),
  // `once` is always available. `always` needs Control's entitlement and a manager role.
  // `thread` is refused until Model Plane has a store for it.
  scope: z.enum(['once', 'thread', 'always']).optional(),
  reason: z.string().max(500).optional(),
  // Required for `always` (8 to 200 characters); the action envelope's key is used when absent.
  idempotencyKey: z.string().min(8).max(200).optional(),
})

const spaceDecideApprovalOutput = z.object({
  approval: z.record(z.string(), z.unknown()),
  grant: z.object({
    status: z.enum(['none', 'recorded', 'not_recorded']),
    scope: z.string().optional(),
    reason: z.string().optional(),
    rule: z.record(z.string(), z.unknown()).nullable().optional(),
  }),
})

const spaceRevokeApprovalGrantInput = z.object({
  spaceRef: z.string().min(1),
  grantId: z.string().min(1).max(128),
})

const spaceRevokeApprovalGrantOutput = z.object({
  grant: z.record(z.string(), z.unknown()).nullable(),
})

export const ws10SpaceActions = [
  {
    id: 'spaces.decide_approval',
    label: 'Decide a Space approval',
    description:
      'Approve or deny one paused action in this room. Only the member who started the run can decide; the approved action runs as that member. Scope once covers this call; always records a standing rule for an MCP tool, which a manager can revoke.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceDecideApprovalInput,
    outputSchema: spaceDecideApprovalOutput,
  },
  {
    id: 'spaces.revoke_approval_grant',
    label: 'Revoke a standing approval',
    description:
      'Revoke one standing approval rule in this room. The next matching call pauses for approval again. The creator or a Space manager can revoke; revoking twice is harmless.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceRevokeApprovalGrantInput,
    outputSchema: spaceRevokeApprovalGrantOutput,
  },
] as const satisfies readonly ActionDescriptor[]
