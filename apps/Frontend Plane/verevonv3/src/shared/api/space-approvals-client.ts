import { z } from 'zod'
import { requestJson } from './http'
import { normalizeApproval, type Approval } from './orchestration-client'

// WS10-S4: the Space side of an approval decision. The gateway answers (apps/gateway/src/domains/
// spaces/approvals.rs):
//   POST /api/v1/spaces/{space_ref}/approvals/{approval_id}/decide
//     400 invalid_request, 403 approval_not_addressed_to_you | approval_scope_not_permitted,
//     404 approval_not_found, 409 approval_already_decided | approval_expired,
//     503 approval_authority_unavailable. A standing rule that could not be recorded is NOT an
//     error: the decision stands as a plain `once` and `grant.status` is `not_recorded`.
//   GET /api/v1/spaces/{space_ref}/approval-grants, DELETE .../{grant_id}, GET .../approval-policy:
//     404 feature_disabled with the gateway flag off (the picker then offers `once` only), and
//     Control's typed refusals (approval_scope_not_permitted, approval_grants_not_entitled,
//     zdr_durable_mutation_forbidden, approval_grant_not_found, space_not_found).
// The action contracts are `spaces.decide_approval` and `spaces.revoke_approval_grant`
// (shared/actions/domains/spaces/ws10.ts). A role check in the UI is presentation: servers decide.

export type ApprovalScope = 'once' | 'thread' | 'always'

/** The Space ceiling Control stores. Absent (flag off or unreadable) means `once` only. */
export type SpaceApprovalPolicy = {
  readonly maxGrantScope: ApprovalScope
  readonly defaultAddresseeKind: string
  readonly quarantineMode: string
  readonly revision: number
}

/** One standing approval rule. Ids, keys and dates only: never content. */
export type SpaceStandingApproval = {
  readonly ruleId: string
  readonly scope: string
  readonly agentSubjectId: string
  readonly toolKey: string
  readonly actionKey: string
  readonly grantedBySubjectId: string
  readonly grantedByRole: string
  readonly createdAt: string
  readonly reviewBy: string
  readonly revokedAt?: string
  readonly useCount: number
  readonly lastUsedAt?: string
}

export type SpaceGrantResult =
  | { readonly status: 'none' }
  | { readonly status: 'recorded'; readonly rule?: SpaceStandingApproval }
  | { readonly status: 'not_recorded'; readonly reason?: string }

export type SpaceApprovalDecisionResult = {
  readonly approval: Approval | null
  readonly grant: SpaceGrantResult
}

const SCOPE_ORDER: readonly ApprovalScope[] = ['once', 'thread', 'always']

const rawRule = z
  .object({
    rule_id: z.string(),
    scope: z.string().default('always'),
    agent_subject_id: z.string().default(''),
    tool_key: z.string().default(''),
    action_key: z.string().default(''),
    granted_by_subject_id: z.string().default(''),
    granted_by_role: z.string().default(''),
    created_at: z.string().default(''),
    review_by: z.string().default(''),
    revoked_at: z.string().nullish(),
    use_count: z.number().default(0),
    last_used_at: z.string().nullish(),
  })
  .passthrough()

function toRule(raw: unknown): SpaceStandingApproval | undefined {
  const parsed = rawRule.safeParse(raw)
  if (!parsed.success) return undefined
  const rule = parsed.data
  return {
    ruleId: rule.rule_id,
    scope: rule.scope,
    agentSubjectId: rule.agent_subject_id,
    toolKey: rule.tool_key,
    actionKey: rule.action_key,
    grantedBySubjectId: rule.granted_by_subject_id,
    grantedByRole: rule.granted_by_role,
    createdAt: rule.created_at,
    reviewBy: rule.review_by,
    revokedAt: rule.revoked_at ?? undefined,
    useCount: rule.use_count,
    lastUsedAt: rule.last_used_at ?? undefined,
  }
}

/** A fresh key for one decision. A retry of the SAME decision must reuse it. */
export function newApprovalIdempotencyKey(): string {
  const bytes = new Uint8Array(12)
  globalThis.crypto.getRandomValues(bytes)
  return `appr-${Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')}`
}

/**
 * The scopes the picker may offer for one approval, from what the server states and nothing
 * more. `once` is always offered. `always` needs the Space ceiling to allow it, a role that may
 * manage standing rules, and an MCP tool (Control refuses a standing rule for anything else, so
 * offering it would be a button that cannot work). `thread` is never offered: it has no store
 * yet and the gateway refuses it.
 */
export function offerableScopes(input: {
  readonly policy: SpaceApprovalPolicy | undefined
  readonly viewerRole: string | undefined
  readonly toolKey: string | undefined
}): ApprovalScope[] {
  const offered: ApprovalScope[] = ['once']
  const ceiling = input.policy ? SCOPE_ORDER.indexOf(input.policy.maxGrantScope) : 0
  const mayManage = input.viewerRole === 'manager' || input.viewerRole === 'owner'
  const mcp = (input.toolKey ?? '').startsWith('mcp__')
  if (ceiling >= SCOPE_ORDER.indexOf('always') && mayManage && mcp) offered.push('always')
  return offered
}

/** Record one decision for an approval from inside a Space. */
export async function decideSpaceApproval(
  spaceRef: string,
  approvalId: string,
  input: {
    readonly decision: 'approve' | 'reject'
    readonly scope?: ApprovalScope
    readonly reason?: string
    readonly idempotencyKey?: string
  },
  signal?: AbortSignal,
): Promise<SpaceApprovalDecisionResult> {
  const body: Record<string, unknown> = { decision: input.decision }
  if (input.scope && input.scope !== 'once') body.scope = input.scope
  if (input.reason) body.reason = input.reason
  if (input.scope === 'always') {
    body.idempotency_key = input.idempotencyKey ?? newApprovalIdempotencyKey()
  } else if (input.idempotencyKey) {
    body.idempotency_key = input.idempotencyKey
  }
  const payload = await requestJson<{
    approval?: Record<string, unknown>
    grant?: Record<string, unknown>
  }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/approvals/${encodeURIComponent(approvalId)}/decide`,
    { method: 'POST', body: JSON.stringify(body), signal },
  )
  const grantRaw = payload.grant ?? {}
  const status = grantRaw.status
  const grant: SpaceGrantResult =
    status === 'recorded'
      ? { status: 'recorded', rule: toRule(grantRaw.rule) }
      : status === 'not_recorded'
        ? {
            status: 'not_recorded',
            reason: typeof grantRaw.reason === 'string' ? grantRaw.reason : undefined,
          }
        : { status: 'none' }
  return { approval: payload.approval ? normalizeApproval(payload.approval) : null, grant }
}

/** The Space ceiling, or `undefined` when standing approvals are off or unreadable. */
export async function readSpaceApprovalPolicy(
  spaceRef: string,
  signal?: AbortSignal,
): Promise<SpaceApprovalPolicy | undefined> {
  try {
    const payload = await requestJson<{ policy?: Record<string, unknown> | null }>(
      `/api/v1/spaces/${encodeURIComponent(spaceRef)}/approval-policy`,
      { signal },
    )
    const policy = payload.policy
    if (!policy) return undefined
    const max = policy.max_grant_scope
    return {
      maxGrantScope: max === 'always' || max === 'thread' ? max : 'once',
      defaultAddresseeKind:
        typeof policy.default_addressee_kind === 'string' ? policy.default_addressee_kind : '',
      quarantineMode: typeof policy.quarantine_mode === 'string' ? policy.quarantine_mode : '',
      revision: typeof policy.revision === 'number' ? policy.revision : 0,
    }
  } catch {
    // Flag off or not readable: the picker offers `once` only, which is always true.
    return undefined
  }
}

/** The room's live standing approvals (manager or owner). Throws the typed ApiError otherwise. */
export async function listSpaceStandingApprovals(
  spaceRef: string,
  signal?: AbortSignal,
): Promise<SpaceStandingApproval[]> {
  const payload = await requestJson<{ grants?: unknown[] }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/approval-grants`,
    { signal },
  )
  return (payload.grants ?? [])
    .map(toRule)
    .filter((rule): rule is SpaceStandingApproval => rule !== undefined && rule.revokedAt === undefined)
}

/** Revoke one rule. Idempotent: revoking a revoked rule answers success. */
export async function revokeSpaceStandingApproval(
  spaceRef: string,
  grantId: string,
  signal?: AbortSignal,
): Promise<void> {
  await requestJson<unknown>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/approval-grants/${encodeURIComponent(grantId)}`,
    { method: 'DELETE', signal },
  )
}
