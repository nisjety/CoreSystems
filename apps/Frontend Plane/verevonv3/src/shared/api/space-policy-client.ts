import { ApiError, requestJson } from './http'

// WS03-S6: org-admin Space effect-policy client. Control (user-core) owns the policy row, the
// canonical owner/admin role check, the write switch and the audit trail; the gateway only
// proxies (apps/gateway/src/domains/orgs/space_policy.rs). Nothing here decides who may change
// the policy: every refusal is shown as the server worded it. No org, actor or role is ever
// sent in a body (the gateway rejects them); the org is the path segment.

/** The closed catalogue Control owns. Unknown keys from a newer server are shown by key name. */
export const SPACE_ENTITLEMENT_KEYS = [
  'thread_create',
  'thread_read',
  'retrieval_read',
  'import_write',
  'agent_action',
  'schedule_fire',
  'sandbox_capability',
  'process_registry',
] as const

export type SpaceEntitlementKey = (typeof SPACE_ENTITLEMENT_KEYS)[number]

/** `compatible` works under Space zero retention; the other two cannot be newly enabled under it. */
export type SpaceEntitlementZdrClass = 'compatible' | 'zdr_inert' | 'zdr_unproven'

export interface SpacePolicyEntitlementView {
  key: string
  /** The stored bit. */
  enabled: boolean
  /** What a Space request actually gets after the org floor is applied. */
  effective: boolean
  zdr: SpaceEntitlementZdrClass
  /** True when the bit cannot be newly enabled while Space ZDR is on. */
  zdr_blocked: boolean
  decisions: string[]
}

export interface SpacePolicyPresetView {
  id: string
  entitlements: Record<string, boolean>
  available: boolean
  blocked_by: string[]
}

/** The read-only privacy attestation. Set by Verevon operations, never by this client. */
export interface SpacePolicyAttestation {
  privacy_policy_ref: string
  purpose: string
  lawful_basis: string
  privacy_class: string
  third_party_processing_allowed: boolean
  retention_class: string
  residency: string
  deletion_scope: string
  zero_data_retention: boolean
}

export interface SpacePolicyActor {
  type: string
  id: string
  name?: string
  role?: string
}

export interface SpacePolicyLastChange {
  event_id: number
  at: string
  source: string
  actor: SpacePolicyActor
  change_kind: string
  preset_id?: string
  changed_keys: string[]
}

export interface SpacePolicyView {
  org_id: string
  state: 'provisioned' | 'not_provisioned'
  capabilities: { write: boolean; write_denied_reason: string | null }
  revisions: { policy: number; entitlement: number }
  updated_at: string | null
  attestation: SpacePolicyAttestation | null
  entitlements: SpacePolicyEntitlementView[]
  presets: SpacePolicyPresetView[]
  matches_preset: string | null
  active_spaces: number
  last_change: SpacePolicyLastChange | null
}

export interface SpacePolicyEvent {
  event_id: number
  at: string
  source: string
  actor: SpacePolicyActor
  change_kind: string
  preset_id?: string
  changed: Array<{ key: string; from: boolean; to: boolean }>
  entitlement_revision_after: number
  spaces_fenced: number
}

export interface SpacePolicyWriteResult {
  changed: boolean
  replayed: boolean
  spaces_fenced: number
  policy: SpacePolicyView
}

/** Exactly one of `preset_id` or `entitlements` (all eight keys) is sent. */
export type SpacePolicyWrite =
  | { preset_id: string; entitlements?: undefined }
  | { entitlements: Record<string, boolean>; preset_id?: undefined }

export const SPACE_POLICY_ADMIN_DISABLED = 'space_policy_admin_disabled'
export const SPACE_POLICY_REVISION_CONFLICT = 'space_policy_revision_conflict'

const base = (orgId: string) => `/api/v1/orgs/${encodeURIComponent(orgId)}/space-policy`

export function getSpacePolicy(orgId: string): Promise<SpacePolicyView> {
  return requestJson<SpacePolicyView>(base(orgId))
}

export function listSpacePolicyEvents(orgId: string, limit = 20): Promise<SpacePolicyEvent[]> {
  return requestJson<SpacePolicyEvent[]>(`${base(orgId)}/events?limit=${encodeURIComponent(String(limit))}`)
}

/**
 * Apply a preset or the full set of eight keys. `idempotencyKey` is chosen by the caller and
 * reused on a retry of the same edit; it is never generated here, so a retry cannot double-apply.
 */
export function setSpacePolicyEntitlements(
  orgId: string,
  input: {
    idempotencyKey: string
    expectedEntitlementRevision: number
    change: SpacePolicyWrite
  },
): Promise<SpacePolicyWriteResult> {
  return requestJson<SpacePolicyWriteResult>(`${base(orgId)}/entitlements`, {
    method: 'PUT',
    body: JSON.stringify({
      idempotency_key: input.idempotencyKey,
      expected_entitlement_revision: input.expectedEntitlementRevision,
      ...(input.change.preset_id !== undefined
        ? { preset_id: input.change.preset_id }
        : { entitlements: input.change.entitlements }),
    }),
  })
}

/** A fresh idempotency key (8..128 chars) for one distinct edit. */
export function newSpacePolicyIdempotencyKey(): string {
  const uuid = typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
    ? crypto.randomUUID()
    : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 12)}`
  return `space-policy-${uuid}`
}

/** True when the gateway reports the admin API switched off: the console hides itself. */
export function isSpacePolicyAdminDisabled(err: unknown): boolean {
  return err instanceof ApiError && err.status === 404 && err.code === SPACE_POLICY_ADMIN_DISABLED
}

/**
 * Whether the console should be offered for this org. Asked only for an admin; the gateway
 * flag answers 404 `space_policy_admin_disabled`, a non-admin gets 403. Any error or a
 * mismatch means "not offered": a nav entry must never lead to a console that cannot load.
 */
export async function probeSpacePolicyAdminAvailable(orgId: string): Promise<boolean> {
  try {
    await getSpacePolicy(orgId)
    return true
  } catch {
    return false
  }
}
