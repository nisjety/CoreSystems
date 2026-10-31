import { requestJson } from './http'

// The org model-entitlement admin API (WS10-S5, ADR-0005). org-core stores and explains the
// policy, inference-core enforces it, and the gateway only forwards: nothing here decides who may
// use a model. Every shape below holds model ids, role names and effort names, never content.

export type ModelAllowMode = 'all' | 'listed'

/** The default a chat surface starts with when the caller names nothing. */
export interface ModelSurfaceDefault {
  model?: string
  effort?: string
}

/**
 * One policy layer. `allow` and `deny` hold exact model ids or `provider:<id>` selectors, no
 * patterns. A deny wins over an allow.
 */
export interface ModelPolicy {
  allow_mode: ModelAllowMode
  allow?: string[]
  deny?: string[]
  max_effort?: string
  min_privacy_tier?: string
  defaults?: Record<string, ModelSurfaceDefault>
}

/** The stored policy set. `revision` 0 means nothing was ever written. */
export interface ModelPolicySet {
  organization_id: string
  revision: number
  org_default: ModelPolicy | null
  roles: Record<string, ModelPolicy>
  updated_at: string | null
  updated_by: string
}

/**
 * What GET answers. With the admin switch off the gateway answers only `{ enabled: false }`, so
 * the section is hidden; with it on, the policy set carries `enabled: true`.
 */
export type ModelEntitlementState = { enabled: false } | ({ enabled: true } & ModelPolicySet)

/** A replacement for the whole policy set, edited from `expected_revision` (0 for the first write). */
export interface ModelPolicyPut {
  expected_revision: number
  org_default?: ModelPolicy | null
  roles?: Record<string, ModelPolicy>
}

/** One layer a resolution came from. */
export interface ModelEntitlementSource {
  layer: 'org_default' | 'role_override'
  role?: string
}

/** Why a person sees the models they see: the layer, the revision and the policy that applied. */
export interface ModelEntitlementExplanation {
  organization_id: string
  user_id: string
  state: 'none' | 'org' | 'role' | 'member_not_found'
  revision: number
  role: string
  can_manage: boolean
  policy: ModelPolicy | null
  sources: ModelEntitlementSource[]
}

function base(orgId: string): string {
  return `/api/v1/orgs/${encodeURIComponent(orgId)}/model-entitlement`
}

export function getModelEntitlement(orgId: string): Promise<ModelEntitlementState> {
  return requestJson<ModelEntitlementState>(base(orgId))
}

/**
 * Replace the whole policy set. A stale `expected_revision` is refused with 409
 * `revision_conflict` and the current revision, never overwritten; a policy that would leave a
 * Zero Data Retention org with no attested model is refused with 409
 * `model_policy_leaves_no_zdr_model`.
 */
export function putModelEntitlement(orgId: string, body: ModelPolicyPut): Promise<ModelPolicySet> {
  return requestJson<ModelPolicySet>(base(orgId), {
    method: 'PUT',
    body: JSON.stringify(body),
  })
}

/** Explain the caller's own access, or another member's (owner or admin only). */
export function explainModelEntitlement(
  orgId: string,
  userId?: string,
): Promise<ModelEntitlementExplanation> {
  const query = userId === undefined ? '' : `?user_id=${encodeURIComponent(userId)}`
  return requestJson<ModelEntitlementExplanation>(`${base(orgId)}/explain${query}`)
}
