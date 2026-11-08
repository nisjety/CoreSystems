import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// WS03-S4: org-admin Space effect-policy contracts. Control (user-core) owns the policy
// row, the canonical owner/admin role check and the write switch; the gateway only
// proxies (apps/gateway/src/domains/orgs/space_policy.rs, dispatchers_spaces/ws03.rs).
// Neither id is model-executable: an agent must never widen the entitlement floor
// (model-access.ts closes both, model-eligibility.ts never lists them).

const envelopeStatusOutput = z.object({
  status: z.string(),
})

// Control owns the closed key set; the gateway only checks the shape, so a new
// catalogue row needs no edit here. Names match ^[a-z_]{1,48}$.
const entitlementKey = z.string().regex(/^[a-z_]{1,48}$/)

const spaceSetEffectEntitlementsInput = z.object({
  orgId: z.string().min(1),
  expectedEntitlementRevision: z.number().int().min(0),
  entitlements: z.record(entitlementKey, z.boolean()),
})

const spaceApplyEffectPresetInput = z.object({
  orgId: z.string().min(1),
  expectedEntitlementRevision: z.number().int().min(0),
  presetId: z.string().min(1).max(48),
})

export const ws03SpaceActions = [
  {
    id: 'spaces.set_effect_entitlements',
    label: 'Set Space entitlements',
    description:
      'Set which Space capabilities (threads, retrieval, imports, schedules, agent actions, sandbox, processes) the organization allows. Org owner or admin only; changes the floor for every Space in the organization.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: spaceSetEffectEntitlementsInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.apply_effect_preset',
    label: 'Apply Space entitlement preset',
    description:
      'Apply a named Space entitlement preset (locked up to agent computer) to the organization. Org owner or admin only; reversible by applying another preset.',
    ownerPlane: 'control',
    risk: 'high',
    requiresApproval: true,
    reversible: true,
    inputSchema: spaceApplyEffectPresetInput,
    outputSchema: envelopeStatusOutput,
  },
] as const satisfies readonly ActionDescriptor[]
