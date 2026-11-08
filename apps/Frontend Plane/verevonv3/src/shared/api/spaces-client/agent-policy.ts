// WS08-S5: who may wake a bound agent. `PATCH /api/v1/spaces/{space_ref}/agents/{binding_ref}/policy`
// answers 404 `feature_disabled` with the gateway flag off, 403 `space_role_cannot_manage_agent`
// for anyone but the room's owner or manager, 400 `unsupported_policy_field` / `invalid_policy`
// (`details.reason`), 404 `agent_binding_not_found`, 409 `policy_conflict` (a stale
// `expectedUpdatedAt`), 503 `agent_binding_unavailable`. They arrive as an ApiError carrying the
// code. The action contract is `spaces.set_agent_wake_policy` (shared/actions/domains/spaces/ws08.ts).
import { requestJson } from '../http'

export type SpaceAgentRespondTo = 'members' | 'managers' | 'allowlist'

export type SpaceAgentWakePolicy = {
  readonly bindingRef: string
  readonly respondTo: SpaceAgentRespondTo
  /** Control subject ids; empty unless `respondTo` is `allowlist`. */
  readonly respondToSubjects: readonly string[]
  readonly projectionVersion: number | null
  /** The binding's new `updated_at`: send it back as `expectedUpdatedAt` on the next change. */
  readonly updatedAt: number | null
  readonly changed: boolean
}

/**
 * Set who may wake one bound agent in this room. `expectedUpdatedAt` is the binding's
 * `updated_at` as last read. `respondToSubjects` is required for `allowlist` and refused for the
 * other two modes.
 */
export async function setSpaceAgentWakePolicy(
  spaceRef: string,
  bindingRef: string,
  policy: {
    readonly respondTo: SpaceAgentRespondTo
    readonly respondToSubjects?: readonly string[]
    readonly expectedUpdatedAt: number
  },
): Promise<SpaceAgentWakePolicy> {
  const body: Record<string, unknown> = {
    respond_to: policy.respondTo,
    expected_updated_at: policy.expectedUpdatedAt,
  }
  if (policy.respondToSubjects && policy.respondToSubjects.length > 0) {
    body.respond_to_subjects = policy.respondToSubjects
  }
  const response = await requestJson<{
    binding_ref?: string
    respond_to?: SpaceAgentRespondTo
    respond_to_subjects?: readonly string[]
    projection_version?: number | null
    updated_at?: number | null
    changed?: boolean
  }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/agents/${encodeURIComponent(bindingRef)}/policy`,
    { method: 'PATCH', body: JSON.stringify(body) },
  )
  return {
    bindingRef: response.binding_ref ?? bindingRef,
    respondTo: response.respond_to ?? policy.respondTo,
    respondToSubjects: response.respond_to_subjects ?? [],
    projectionVersion: response.projection_version ?? null,
    updatedAt: response.updated_at ?? null,
    changed: response.changed === true,
  }
}
