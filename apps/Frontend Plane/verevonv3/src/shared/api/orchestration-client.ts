import type { z } from 'zod'
import {
  approvalsConfirmRuleInput,
  approvalsListGrantsInput,
  approvalsRevokeGrantInput,
  getActionDescriptor,
  type integrationsConnectorPoliciesOutput,
  integrationsSetConnectorPolicyInput,
  orchestrationDecideApprovalInput,
} from '@/shared/actions/action-registry'
import { causeCodeOf, type ApprovalCause, type CauseCode } from '../approvals/approval-copy'
import { APPROVAL_EDIT_CODES, type MailSendPatch } from '../approvals/approval-edit-limits'
import { ApiError, requestJson } from './http'
import { readSseStream, type SseEvent } from './sse'

export type { ApprovalCause, CauseCode } from '../approvals/approval-copy'

// The closed refusal vocabulary of `continueRun` lives in the action contract; the
// client re-exports its guards so a caller reads a failure from one place.
export {
  CHAT_RUN_CONTINUE_FRAME_REFUSALS,
  CHAT_RUN_CONTINUE_HTTP_REFUSALS,
  chatRunContinueFrameRefusalOf,
  chatRunContinueHttpRefusalOf,
  chatRunContinueParkFailureOf,
  chatRunContinueSpentConnected,
  chatRunContinueTransportFailureOf,
  chatRunContinueUnflaggedRefusalOf,
} from '../actions/chat-run-continue-contract'
export type {
  ChatRunContinueEffect,
  ChatRunContinueFrameRefusal,
  ChatRunContinueFrameRefusalCode,
  ChatRunContinueHttpRefusal,
  ChatRunContinueHttpRefusalCode,
  ChatRunContinueParkFailure,
  ChatRunContinueRunStaysParked,
  ChatRunContinueTransportFailure,
  ChatRunContinueTransportReason,
} from '../actions/chat-run-continue-contract'

// Human-in-the-loop run control for agentic chat runs. These call the gateway
// `/api/v1/orchestration/*` proxy (→ model-gateway → session-core/execution-core).
// Used by the chat "internal Claude Code" approval UI + plan mode: when an
// agentic run pauses for a risky tool (a `step_update` with status "paused"),
// the chat lists the run's pending approvals here, the user decides, and the
// run is resumed.

export type ApprovalDecision = 'approve' | 'reject'

/**
 * How far an approval decision reaches (build plan WS13 S6). `once` is the one decision that always
 * exists; `thread` and `run` are standing grants, which the server offers only where the approval's
 * cause allows them and the deployment has switched them on.
 */
export type ApprovalScope = 'once' | 'thread' | 'run'

const APPROVAL_SCOPES: readonly ApprovalScope[] = ['once', 'thread', 'run']
/** What approving does with the paused work (build plan WS10 S1). Server-stated, never guessed. */
export type ApprovalResumeClass = 'action_continuation' | 'live_poll' | 'none'

/** Where an approved action stands. Derived server-side from the delivery rows. */
export type ApprovalContinuationState =
  | 'queued'
  | 'running'
  | 'completed'
  | 'failed'
  | 'not_resumable'
  | 'live'
  | 'awaiting_continue'

export type ApprovalContinuation = {
  state: ApprovalContinuationState
  failureCode?: string
  /** Independent read-back of the outcome, when one exists. */
  verification?: string
  providerReceiptId?: string
}

/** A pending/decided approval as reported by model-gateway's orchestration API. */
export type Approval = {
  id: string
  runId?: string
  planId?: string
  stepId?: string
  /**
   * Approval kind as the provider names it (`APPROVAL_KIND_*`). A token for code, never for a
   * reader: `shared/approvals/approval-copy` words every surface from `cause` instead.
   */
  kind?: string
  /** Why the pause was raised, when the creator stated it. Absent on an older approval. */
  cause?: ApprovalCause
  /** Stable key of the gated tool, when the creator stated it. */
  toolKey?: string
  /** Short allowlisted label of the gated action. Never raw arguments. */
  actionLabel?: string
  /** What approving does. Absent on an approval created before the card context existed. */
  resumeClass?: ApprovalResumeClass
  /** Where the approved action stands. Absent until the approval is granted. */
  continuation?: ApprovalContinuation
  /** PENDING | GRANTED | DENIED | EXPIRED (uppercased provider enum name). */
  status?: string
  requestedBy?: string
  /** Free-form detail/summary of what needs approval, when present. */
  detail?: string
}

type RawApproval = Record<string, unknown>

function str(value: unknown): string | undefined {
  return typeof value === 'string' && value.length > 0 ? value : undefined
}

/**
 * Canonicalize a provider approval-state string to the short form
 * `Approval.status` documents (`PENDING`/`GRANTED`/`DENIED`/`EXPIRED`).
 *
 * The live wire value is the FULL protobuf enum name from
 * `model_plane/v1/orchestration.proto`'s `ApprovalState`
 * (`APPROVAL_STATE_REQUESTED`, `APPROVAL_STATE_GRANTED`,
 * `APPROVAL_STATE_DENIED`, `APPROVAL_STATE_TIMED_OUT`) — never the bare
 * `PENDING`/`GRANTED`/`DENIED`/`EXPIRED` every caller of `listApprovals`
 * (`AgentRunConsole.tsx`, `use-chat-controller.ts`,
 * `KnowledgeComposer.tsx`'s browser-workspace HITL gate) filters on. Without
 * this normalization every one of them silently found zero pending
 * approvals — `(approval.status ?? 'PENDING').toUpperCase() === 'PENDING'`
 * never matched `'APPROVAL_STATE_REQUESTED'` — so a real, correctly-fired
 * backend approval gate looked to the UI exactly like there was nothing to
 * decide. Substring-matched (not exact) so it tolerates either convention.
 */
function canonicalApprovalStatus(raw: string | undefined): string | undefined {
  if (!raw) return raw
  const upper = raw.toUpperCase()
  if (upper.includes('REQUESTED') || upper.includes('PENDING')) return 'PENDING'
  if (upper.includes('GRANTED')) return 'GRANTED'
  if (upper.includes('DENIED')) return 'DENIED'
  if (upper.includes('TIMED_OUT') || upper.includes('EXPIRED')) return 'EXPIRED'
  return upper
}

/**
 * The cause object model-gateway projects on an approval. An absent or malformed cause is
 * `undefined` (the surfaces then say the generic waiting line); a stated one is read through the
 * closed vocabulary, so a code this build does not know arrives as `unclassified_gate`.
 */
export function normalizeApprovalCause(raw: unknown): ApprovalCause | undefined {
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return undefined
  const cause = raw as Record<string, unknown>
  return {
    code: causeCodeOf(cause.code),
    tool: str(cause.tool),
    effectClass: str(cause.effect_class),
    riskCategory: str(cause.risk_category),
    ruleId: str(cause.rule_id),
    scopeOptions: Array.isArray(cause.scope_options)
      ? cause.scope_options.filter((scope): scope is string => typeof scope === 'string')
      : [],
    stepUpRequired: cause.step_up_required === true,
    // Only an explicit `true` offers "Always"; absent or anything else leaves it undefined.
    ruleOffered: cause.rule_offered === true ? true : undefined,
    assistKind: str(cause.assist_kind),
    assistOrigin: str(cause.assist_origin),
    host: str(cause.host),
  }
}

const RESUME_CLASSES: readonly ApprovalResumeClass[] = ['action_continuation', 'live_poll', 'none']
const CONTINUATION_STATES: readonly ApprovalContinuationState[] = [
  'queued',
  'running',
  'completed',
  'failed',
  'not_resumable',
  'live',
  'awaiting_continue',
]

/** The typed continuation, read through the closed vocabulary; an unknown state is no claim. */
export function normalizeApprovalContinuation(raw: unknown): ApprovalContinuation | undefined {
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return undefined
  const value = raw as Record<string, unknown>
  const state = CONTINUATION_STATES.find((candidate) => candidate === value.state)
  if (!state) return undefined
  return {
    state,
    failureCode: str(value.failure_code),
    verification: str(value.verification),
    providerReceiptId: str(value.provider_receipt_id),
  }
}

/** Normalize a model-gateway approval object (snake_case) to our camelCase shape. */
export function normalizeApproval(raw: RawApproval): Approval | null {
  const id = str(raw.id) ?? str(raw.approval_id)
  if (!id) return null
  return {
    id,
    runId: str(raw.run_id) ?? str(raw.runId),
    planId: str(raw.plan_id) ?? str(raw.planId),
    stepId: str(raw.step_id) ?? str(raw.stepId),
    kind: str(raw.kind) ?? str(raw.approval_kind),
    cause: normalizeApprovalCause(raw.cause),
    toolKey: str(raw.tool_key),
    actionLabel: str(raw.action_label),
    resumeClass: RESUME_CLASSES.find((candidate) => candidate === raw.resume_class),
    continuation: normalizeApprovalContinuation(raw.continuation),
    status: canonicalApprovalStatus(str(raw.status) ?? str(raw.state)),
    requestedBy: str(raw.requested_by) ?? str(raw.requestedBy),
    detail: str(raw.detail) ?? str(raw.summary) ?? str(raw.reason) ?? str(raw.tool),
  }
}

/** List a run's approvals; only PENDING ones are surfaced for a decision. */
export async function listApprovals(runId: string, signal?: AbortSignal): Promise<Approval[]> {
  const payload = await requestJson<{ approvals?: RawApproval[] }>(
    `/api/v1/orchestration/runs/${encodeURIComponent(runId)}/approvals`,
    { signal },
  )
  const list = Array.isArray(payload.approvals) ? payload.approvals : []
  return list.map(normalizeApproval).filter((a): a is Approval => a !== null)
}

/**
 * Approve or reject a pending approval (unblocks/blocks the gated tool step).
 *
 * `reason` is checked against the `orchestration.decide_approval` action
 * contract before anything is sent, so a caller cannot post an input the
 * contract does not describe (for example redirect text over the length cap).
 * A reject's `reason` is the redirect text channel (see the contract).
 */
export async function decideApproval(
  approvalId: string,
  decision: ApprovalDecision,
  reason?: string,
  signal?: AbortSignal,
  scope: ApprovalScope = 'once',
  promote = false,
  edit?: ApprovalEdit,
): Promise<Approval | null> {
  const checked = orchestrationDecideApprovalInput.safeParse({
    approvalId,
    decision,
    reason: reason ?? '',
    ...(edit ? { edit } : {}),
  })
  // "Always" (build plan WS13 S7) is an approval of a thread grant and nothing else. An edit
  // (EP-10-S6, decision D1) is approved for this one call only: never with a standing scope or
  // "Always". The server refuses both too; refusing here keeps the person from believing it went.
  if (
    !checked.success ||
    !APPROVAL_SCOPES.includes(scope) ||
    (promote && (decision !== 'approve' || scope !== 'thread')) ||
    (edit !== undefined && (scope !== 'once' || promote))
  ) {
    throw new ApiError('Ugyldig godkjenningsavgjørelse.', 400, 'invalid_approval_decision')
  }
  const payload = await requestJson<{ approval?: RawApproval }>(
    `/api/v1/orchestration/approvals/${encodeURIComponent(approvalId)}/decide`,
    {
      method: 'POST',
      // The scope, "Always" and an edit are sent only when asked for: the one plain decision
      // carries none of them, so an unedited approval is byte-identical to before EP-10-S6.
      body: JSON.stringify({
        decision,
        reason: checked.data.reason ?? '',
        ...(scope === 'once' ? {} : { scope }),
        ...(promote ? { promote: true } : {}),
        ...(checked.data.edit
          ? { edit: { patch: checked.data.edit.patch, baseFingerprint: checked.data.edit.baseFingerprint } }
          : {}),
      }),
      signal,
    },
  )
  return payload.approval ? normalizeApproval(payload.approval) : null
}

/**
 * "Send with my edits" (EP-10-S6): the person's changes to a drafted mail send and the fingerprint
 * of the draft they saw (`ApprovalPreview.actionFingerprint`).
 */
export type ApprovalEdit = {
  patch: MailSendPatch
  baseFingerprint: string
}

/**
 * The typed refusals of an edited approval (EP-10-S4 gateway and BFF): the draft changed since it
 * was read (409 `approval_changed`), it was edited too often (409 `revision_limit`), edits are
 * switched off or not the caller's (403), or the patch broke a limit (422, the EP-10-S3 codes).
 */
export const APPROVAL_EDIT_REFUSALS = [
  'approval_changed',
  'revision_limit',
  'approval_edit_disabled',
  'edit_owner_only',
  'edit_requires_grant',
  'edit_requires_once',
  'invalid_patch',
  'invalid_base_fingerprint',
  ...APPROVAL_EDIT_CODES,
] as const

export type ApprovalEditRefusal = (typeof APPROVAL_EDIT_REFUSALS)[number]

/** The edit refusal an error carries, or null when it is some other failure. */
export function approvalEditRefusalOf(error: unknown): ApprovalEditRefusal | null {
  if (!(error instanceof ApiError) || error.code === null) return null
  return (APPROVAL_EDIT_REFUSALS as readonly string[]).includes(error.code)
    ? (error.code as ApprovalEditRefusal)
    : null
}

/**
 * A rule an "Always" choice on an MCP tool's approval proposed (build plan WS13 S7). It is not a rule
 * until the person confirms it: confirming writes a digest-pinned user-scope allow.
 */
export type ApprovalRuleProposal = {
  id: string
  capabilityId: string
  toolName: string
  toolDigest: string
  createdAt: string
}

/**
 * A rule proposal from the server's snake_case projection (id, tool_name, tool_digest, capability_id,
 * created_at). A row without an id cannot be confirmed or dismissed, so it is dropped. The capability
 * and the digest are for showing only: a confirm never sends them.
 */
export function normalizeRuleProposal(raw: unknown): ApprovalRuleProposal | null {
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return null
  const row = raw as Record<string, unknown>
  const id = str(row.id)
  if (!id) return null
  return {
    id,
    capabilityId: str(row.capability_id) ?? '',
    toolName: str(row.tool_name) ?? '',
    toolDigest: str(row.tool_digest) ?? '',
    createdAt: str(row.created_at) ?? '',
  }
}

/** The proposal id checked against the action contract (it is the only thing a confirm or dismiss names). */
function checkedProposalId(proposal: ApprovalRuleProposal): string {
  const checked = approvalsConfirmRuleInput.safeParse({ proposalId: proposal.id })
  if (!checked.success) {
    throw new ApiError('Ugyldig regelforslag.', 400, 'invalid_proposal_id')
  }
  return checked.data.proposalId
}

/**
 * The caller's open rule proposals. The server answers empty for a zero-retention principal, and
 * keeps answering while its switch is off so open proposals stay visible.
 */
export async function listApprovalRuleProposals(signal?: AbortSignal): Promise<ApprovalRuleProposal[]> {
  // The `{data: {proposals}}` envelope is unwrapped by `requestJson`.
  const payload = await requestJson<{ proposals?: unknown[] }>('/api/v1/orchestration/rule-proposals', {
    signal,
  })
  const list = Array.isArray(payload?.proposals) ? payload.proposals : []
  return list.map(normalizeRuleProposal).filter((proposal): proposal is ApprovalRuleProposal => proposal !== null)
}

/**
 * Confirm a proposed rule (build plan WS13 S7). The body is empty on purpose: the server writes the
 * user-scope allow, pinned to the reviewed definition and lapsing after ninety days, from its own
 * record of the proposal, so the client never chooses the capability, the digest or the expiry.
 */
export async function confirmApprovalRule(proposal: ApprovalRuleProposal, signal?: AbortSignal): Promise<void> {
  const id = checkedProposalId(proposal)
  await requestJson<unknown>(`/api/v1/orchestration/rule-proposals/${encodeURIComponent(id)}/confirm`, {
    method: 'POST',
    body: '{}',
    signal,
  })
}

/** Dismiss a proposed rule. It works with the server switch off, because it only narrows authority. */
export async function dismissApprovalRule(proposal: ApprovalRuleProposal, signal?: AbortSignal): Promise<void> {
  const id = checkedProposalId(proposal)
  await requestJson<unknown>(`/api/v1/orchestration/rule-proposals/${encodeURIComponent(id)}/dismiss`, {
    method: 'POST',
    body: '{}',
    signal,
  })
}

/** The capabilities whose tool policies are the connected-app levels (EP-10 S7). */
export const PROVIDER_READ_CAPABILITY = 'cap.tool.provider.read'
export const PROVIDER_EXECUTE_CAPABILITY = 'cap.tool.provider.execute'

export type ConnectorPolicyInput = z.infer<typeof integrationsSetConnectorPolicyInput>
export type ConnectorPolicies = z.infer<typeof integrationsConnectorPoliciesOutput>
export type ConnectorReadLevel = ConnectorPolicies['read']
export type ConnectorWriteLevel = ConnectorPolicies['write']

/** The level a stored decision is: no row is the default level, `ask` is ask first, `never` is off. */
function readLevelOf(decision: string | undefined): ConnectorReadLevel {
  if (decision === 'never') return 'off'
  if (decision === 'ask') return 'ask_first'
  return 'use_without_asking'
}

function writeLevelOf(decision: string | undefined): ConnectorWriteLevel {
  return decision === 'never' ? 'off' : 'always_ask'
}

/**
 * The connected-app levels in a tool-policy list. The list holds the org's rows and the caller's own,
 * never another person's. A list without the `enforced` bit is read as not enforced, so a level is never
 * shown as honoured on a guess.
 */
export function connectorPoliciesOf(body: unknown): ConnectorPolicies {
  const record = body && typeof body === 'object' ? (body as Record<string, unknown>) : {}
  const rows = Array.isArray(record.policies) ? record.policies : []
  const decisionOf = (capabilityId: string, scopeKind: 'user' | 'org') => {
    for (const row of rows) {
      if (!row || typeof row !== 'object') continue
      const policy = row as Record<string, unknown>
      if (policy.capability_id === capabilityId && policy.scope_kind === scopeKind && typeof policy.decision === 'string') {
        return policy.decision
      }
    }
    return undefined
  }
  return {
    enforced: record.enforced === true,
    read: readLevelOf(decisionOf(PROVIDER_READ_CAPABILITY, 'user')),
    write: writeLevelOf(decisionOf(PROVIDER_EXECUTE_CAPABILITY, 'user')),
    orgRead: readLevelOf(decisionOf(PROVIDER_READ_CAPABILITY, 'org')),
    orgWrite: writeLevelOf(decisionOf(PROVIDER_EXECUTE_CAPABILITY, 'org')),
  }
}

/** The connected-app levels of the caller and their org, and whether they are enforced yet. */
export async function listConnectorPolicies(signal?: AbortSignal): Promise<ConnectorPolicies> {
  const body = await requestJson<unknown>('/api/v1/tool-policies', { signal })
  return connectorPoliciesOf(body)
}

/**
 * The request a connected-app level is: the default level revokes the person's own row, any other writes
 * `ask` or `never` on it. Never an allow and never the org's scope.
 */
export function connectorPolicyRequest(input: ConnectorPolicyInput): {
  capabilityId: string
  method: 'PUT' | 'DELETE'
  body?: Record<string, unknown>
} {
  const capabilityId = input.kind === 'read' ? PROVIDER_READ_CAPABILITY : PROVIDER_EXECUTE_CAPABILITY
  if (input.level === 'use_without_asking' || input.level === 'always_ask') {
    return { capabilityId, method: 'DELETE' }
  }
  return {
    capabilityId,
    method: 'PUT',
    body: { scope: 'user', decision: input.level === 'ask_first' ? 'ask' : 'never', reason: '' },
  }
}

/** Set the person's own connected-app level. Returning to the default when no row exists is already done. */
export async function setConnectorPolicy(input: ConnectorPolicyInput, signal?: AbortSignal): Promise<void> {
  const { capabilityId, method, body } = connectorPolicyRequest(integrationsSetConnectorPolicyInput.parse(input))
  const path = `/api/v1/tool-policies/${encodeURIComponent(capabilityId)}`
  if (method === 'DELETE') {
    try {
      await requestJson<unknown>(`${path}?scope=user`, { method, signal })
    } catch (error) {
      if (error instanceof ApiError && error.status === 404) return
      throw error
    }
    return
  }
  await requestJson<unknown>(path, { method, body: JSON.stringify(body), signal })
}

/** The kind token of an approval that holds tool output back for review (build plan WS10 S8). */
export const UNTRUSTED_CONTENT_KIND = 'APPROVAL_KIND_UNTRUSTED_CONTENT'

/** The inert preview of held tool output. Plain text only: render it as text, never as markup. */
export type QuarantinePreview = {
  text: string
  truncated: boolean
  toolKey?: string
  trustClass?: string
}

/**
 * Read the plain-text preview (at most 1 KiB) of output held for review. Answered only to the
 * person the approval is addressed to while it is pending or granted and unspent; any other case
 * is a 404, which a caller shows as "no preview available".
 */
export async function readQuarantinePreview(
  approvalId: string,
  signal?: AbortSignal,
): Promise<QuarantinePreview | null> {
  const payload = await requestJson<{ preview?: Record<string, unknown> }>(
    `/api/v1/orchestration/approvals/${encodeURIComponent(approvalId)}/quarantine-preview`,
    { signal },
  )
  const raw = payload.preview
  if (!raw || typeof raw.text !== 'string') return null
  return {
    text: raw.text,
    truncated: raw.truncated === true,
    toolKey: str(raw.tool_key),
    trustClass: str(raw.trust_class),
  }
}

/**
 * The full draft behind a pending write approval (EP-10-S1, action `approvals.preview`). A mail
 * send carries every recipient, the full subject and the full body; `other` names only the
 * operation; `none` is an approval with no retained call. Plain text only: render it as text.
 */
export type ApprovalPreview = {
  kind: 'mail_send' | 'other' | 'none'
  provider: string
  operation: string
  connectionId: string
  to: string[]
  subject: string
  bodyText: string
  hasHtml: boolean
  isReply: boolean
  rawOnly: boolean
  editableFields: string[]
  actionFingerprint: string
  revision: number
  /**
   * The gateway takes "Send with my edits" for this draft (EP-10-S6): its edit switch is on and the
   * draft has editable fields. Server-stated, never guessed; absent reads as false.
   */
  canEdit?: boolean
}

const APPROVAL_PREVIEW_KINDS: ReadonlySet<string> = new Set(['mail_send', 'other', 'none'])

const textList = (value: unknown): string[] =>
  Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []

/**
 * Read the full draft of a pending approval. Answered only to the run's owner while the approval
 * is pending; every other case (another person, a decided or expired approval, the preview
 * switched off) is a 404, returned here as `null` so the card says "preview not available". A
 * zero-retention or waived refusal and every other failure throw, typed, as `ApiError`.
 */
export async function getApprovalPreview(
  approvalId: string,
  signal?: AbortSignal,
): Promise<ApprovalPreview | null> {
  // The `{data: {preview}}` envelope is unwrapped by `requestJson`.
  let payload: { preview?: Record<string, unknown> }
  try {
    payload = await requestJson<{ preview?: Record<string, unknown> }>(
      `/api/v1/orchestration/approvals/${encodeURIComponent(approvalId)}/preview`,
      { signal },
    )
  } catch (error) {
    if (error instanceof ApiError && error.status === 404) return null
    throw error
  }
  const raw = payload.preview
  if (!raw || typeof raw.kind !== 'string' || !APPROVAL_PREVIEW_KINDS.has(raw.kind)) return null
  return {
    kind: raw.kind as ApprovalPreview['kind'],
    provider: str(raw.provider) ?? '',
    operation: str(raw.operation) ?? '',
    connectionId: str(raw.connection_id) ?? '',
    to: textList(raw.to),
    subject: typeof raw.subject === 'string' ? raw.subject : '',
    bodyText: typeof raw.body_text === 'string' ? raw.body_text : '',
    hasHtml: raw.has_html === true,
    isReply: raw.is_reply === true,
    rawOnly: raw.raw_only === true,
    editableFields: textList(raw.editable_fields),
    actionFingerprint: str(raw.action_fingerprint) ?? '',
    revision: Number.isInteger(raw.revision) && (raw.revision as number) >= 0 ? (raw.revision as number) : 0,
    canEdit: raw.can_edit === true,
  }
}

/**
 * A standing approval grant of a thread (WS13-S6, actions `approvals.list_grants` and
 * `approvals.revoke_grant`). The caller's own, in one thread; it names a tool and an effect class,
 * never an argument value. A grant is live while `revokedAt` is null and `expiresAt` is ahead.
 */
export type ApprovalGrant = {
  id: string
  threadId: string
  runId: string | null
  scope: 'thread' | 'run'
  toolName: string
  effectClass: string
  causeCode: CauseCode
  createdAt: string
  expiresAt: string
  revokedAt: string | null
  useCount: number
  lastUsedAt: string | null
}

/**
 * Read a grant row from the server (snake_case). A row without an id or with a scope this build does
 * not know is dropped, not guessed at; the cause is read through the closed vocabulary.
 */
export function normalizeApprovalGrant(raw: unknown): ApprovalGrant | null {
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return null
  const row = raw as Record<string, unknown>
  const id = str(row.id) ?? str(row.grant_id)
  if (!id) return null
  const scope = row.scope === 'thread' || row.scope === 'run' ? row.scope : null
  if (!scope) return null
  const count = row.use_count
  return {
    id,
    threadId: str(row.thread_id) ?? '',
    runId: str(row.run_id) ?? null,
    scope,
    toolName: str(row.tool_name) ?? str(row.tool) ?? '',
    effectClass: str(row.effect_class) ?? '',
    causeCode: causeCodeOf(row.cause_code),
    createdAt: str(row.created_at) ?? '',
    expiresAt: str(row.expires_at) ?? '',
    revokedAt: str(row.revoked_at) ?? null,
    useCount: typeof count === 'number' && Number.isInteger(count) && count >= 0 ? count : 0,
    lastUsedAt: str(row.last_used_at) ?? null,
  }
}

/**
 * The caller's own grants in one thread, live and past. The input is checked against the
 * `approvals.list_grants` contract before it is sent. The server answers empty for a zero-retention
 * principal, and keeps answering while the grant switch is off.
 */
export async function listApprovalGrants(
  threadId: string,
  signal?: AbortSignal,
): Promise<ApprovalGrant[]> {
  const checked = approvalsListGrantsInput.safeParse({ threadId })
  if (!checked.success) {
    throw new ApiError('Ugyldig tråd.', 400, 'invalid_thread_id')
  }
  // The `{data: {grants}}` envelope is unwrapped by `requestJson`.
  const payload = await requestJson<{ grants?: unknown[] }>(
    `/api/v1/orchestration/threads/${encodeURIComponent(checked.data.threadId)}/approval-grants`,
    { signal },
  )
  const list = Array.isArray(payload?.grants) ? payload.grants : []
  return list.map(normalizeApprovalGrant).filter((grant): grant is ApprovalGrant => grant !== null)
}

/**
 * Revoke one of the caller's own grants at once. Idempotent, and it works with the grant switch off
 * because it only narrows authority. The id is checked against the `approvals.revoke_grant`
 * contract before it is sent. Returns the grant as the server now holds it, or null when the
 * answer carries none.
 */
export async function revokeApprovalGrant(
  grantId: string,
  signal?: AbortSignal,
): Promise<ApprovalGrant | null> {
  const checked = approvalsRevokeGrantInput.safeParse({ grantId })
  if (!checked.success) {
    throw new ApiError('Ugyldig godkjenning.', 400, 'invalid_grant_id')
  }
  const payload = await requestJson<{ grant?: unknown }>(
    `/api/v1/orchestration/approval-grants/${encodeURIComponent(checked.data.grantId)}`,
    { method: 'DELETE', signal },
  )
  return normalizeApprovalGrant(payload?.grant)
}

/** Resume a run that paused for approval, after the decision is recorded. */
export async function resumeRun(runId: string, signal?: AbortSignal): Promise<void> {
  await requestJson<unknown>(`/api/v1/orchestration/runs/${encodeURIComponent(runId)}/resume`, {
    method: 'POST',
    body: '{}',
    signal,
  })
}

/**
 * Continue a run that paused for a granted loop approval (build plan WS03 S3).
 * The gateway relays the continuation as SSE: the approved call runs once, then
 * the same run finishes. Only the approval id and its claim token go on the wire;
 * the tools and authority come from the pause's snapshot upstream, never here.
 *
 * Rejects when the connection or the gateway refuses the continuation, so the
 * caller sees the typed `ApiError` code rather than a stalled stream. Read that
 * rejection with `chatRunContinueHttpRefusalOf` (the eight listed refusals) and
 * then `chatRunContinueTransportFailureOf` (everything else: a 502
 * `upstream_unavailable`, a 504, a dropped connection, an unlisted body), and the
 * frames with `chatRunContinueSpentConnected`, `chatRunContinueFrameRefusalOf` and
 * `chatRunContinueUnflaggedRefusalOf`.
 *
 * After an ambiguous failure (an effect whose `approvalSpent` is `unknown`) do not
 * assume either way: follow the run by its id with the existing run read; the run
 * read is the arbiter. Sending the request again with the same claim token is
 * answered either with the continuation or with a refusal. Only 409
 * `continuation_already_consumed` means the click was spent; `not_resumable`,
 * `approval_not_found` and `continue_disabled` leave the spend unknown, because a
 * spent approval whose run has ended gets them too. Following the run does not promise
 * that it is live: after `continue_unavailable` the run may still be recovered
 * inside the recovery window, about 15 minutes from the consume.
 */
export async function continueRun(
  runId: string,
  approvalId: string,
  claimToken: string,
  onEvent: (event: SseEvent) => void,
  signal?: AbortSignal,
): Promise<void> {
  let connError: unknown

  await readSseStream(
    `/api/v1/orchestration/runs/${encodeURIComponent(runId)}/continue`,
    {
      method: 'POST',
      // Snake_case on the wire: the BFF body rejects unknown fields outright.
      body: JSON.stringify({ approval_id: approvalId, claim_token: claimToken }),
      signal,
    },
    onEvent,
    (err) => {
      connError = err
    },
  )

  if (connError) throw connError
}

/**
 * Resume a run the executor PARKED (WS03 S8-A): there is no approval to spend, so the body is
 * the park form `{ approval_id: '', claim_token }`, the id present and exactly empty. The
 * gateway relays the resume as SSE, and a failure is a frame marked `park_continue: true` (or
 * the bare `agent_dispatch_unavailable` of the stream). It has no caller: there is no button,
 * no flow and no controller that resumes a parked run yet, so it exists only so the contract
 * and the wire shape are pinned before one is wired.
 *
 * Rejects as `continueRun` does when the gateway refuses before a stream opens (for example
 * 409 `not_resumable`). Read the frames with `chatRunContinueParkFailureOf` (pass
 * `{ parkStream: true }` for the bare frame): a failed park continue spends nothing, so the
 * reader says to follow the run by its id, and whether the run stays parked (`runStaysParked`);
 * the run read is the arbiter.
 */
export async function continueParkedRun(
  runId: string,
  claimToken: string,
  onEvent: (event: SseEvent) => void,
  signal?: AbortSignal,
): Promise<void> {
  let connError: unknown

  await readSseStream(
    `/api/v1/orchestration/runs/${encodeURIComponent(runId)}/continue`,
    {
      method: 'POST',
      // Snake_case on the wire, and the approval id is present and exactly empty: the BFF body
      // has no absent-key form.
      body: JSON.stringify({ approval_id: '', claim_token: claimToken }),
      signal,
    },
    onEvent,
    (err) => {
      connError = err
    },
  )

  if (connError) throw connError
}

/**
 * Continue a run whose browser help pause the person DENIED ("Skip this site", EP-1 S2, D3-647).
 * The approval named is the denial, so nothing is spent: the body is the committed one plus
 * `skipped: true`, and the gateway relays the continuation as SSE like `continueRun`. The executor
 * releases the page the run held for the person and continues the run with the paused call answered
 * as skipped, so the model looks elsewhere. The claim token is checked against the `chat.run.continue`
 * action contract first, as every continue's is.
 *
 * Rejects as `continueRun` does when the gateway refuses before a stream opens (for example 409
 * `approval_not_resumable` for an approval that is not a denied help pause, or 401 when no browser
 * credential could be minted). Nothing was spent in any of those cases, so the caller may fall back
 * to the plain resume; read the frames with `chatRunContinueFrameRefusalOf` and
 * `chatRunContinueUnflaggedRefusalOf` as for `continueRun`.
 */
export async function continueSkippedRun(
  runId: string,
  approvalId: string,
  claimToken: string,
  onEvent: (event: SseEvent) => void,
  signal?: AbortSignal,
): Promise<void> {
  const checked = getActionDescriptor('chat.run.continue')?.inputSchema.safeParse({
    runId,
    approvalId,
    claimToken,
    skipped: true,
  })
  if (!checked?.success) {
    throw new ApiError('Ugyldig fortsettelse.', 400, 'invalid_run_continue')
  }
  let connError: unknown

  await readSseStream(
    `/api/v1/orchestration/runs/${encodeURIComponent(runId)}/continue`,
    {
      method: 'POST',
      // Snake_case on the wire: the BFF body rejects unknown fields outright.
      body: JSON.stringify({ approval_id: approvalId, claim_token: claimToken, skipped: true }),
      signal,
    },
    onEvent,
    (err) => {
      connError = err
    },
  )

  if (connError) throw connError
}

/** Durable result of an authorized run cancellation. */
export type RunCancellation = {
  runId?: string
  cancelled: boolean
  status?: string
  /** Session Core RUN_CANCELLED event id, when a receipt exists. */
  receiptId?: string
}

/** Cancel a run (e.g. the user rejects and wants to stop the whole run). */
export async function cancelRun(runId: string, signal?: AbortSignal): Promise<RunCancellation> {
  const payload = await requestJson<Record<string, unknown>>(
    `/api/v1/orchestration/runs/${encodeURIComponent(runId)}/cancel`,
    {
      method: 'POST',
      body: '{}',
      signal,
    },
  )
  return {
    runId: str(payload.run_id) ?? str(payload.runId),
    cancelled: payload.cancelled === true || str(payload.status)?.toLowerCase() === 'cancelled',
    status: str(payload.status),
    receiptId: str(payload.receipt_id) ?? str(payload.receiptId),
  }
}

// ── Run console reads ─────────────────────────────────────────────────────────
// Point-in-time snapshots for the agentic run console (plans/todos/lineage).
// The live `step_update`/`*_transitioned` stream comes from run-console-client;
// these GETs hydrate the initial state and reconcile after a resume.

type RawPlan = Record<string, unknown>
type RawTodo = Record<string, unknown>

/** A run's plan as reported by model-gateway's orchestration API. */
export type Plan = {
  id: string
  runId?: string
  threadId?: string
  author?: string
  /** Lifecycle state (uppercased provider enum name), rendered as-is. */
  state?: string
  /** Free-form human-friendly summary of the plan, when present. */
  summary?: string
  /** Ordered steps emitted by the orchestration service. */
  steps?: PlanStep[]
  /** Previous plan id when this plan supersedes an earlier revision. */
  supersedes?: string
}

export type PlanStep = {
  id: string
  title: string
  operation?: string
  state?: string
  /**
   * One line on how the step went — its failure reason, or a summary of its
   * output. Absent when the step recorded neither.
   */
  detail?: string
}

/** A thread's todo as reported by model-gateway's orchestration API. */
export type Todo = {
  id: string
  threadId?: string
  runId?: string
  /** Lifecycle state (uppercased provider enum name), rendered as-is. */
  state?: string
  /** Human-friendly todo title/description, when present. */
  title?: string
  description?: string
  priority?: string
  blockedBy?: string[]
}

/**
 * One user edit to a proposed plan's step list (plan-gate §2.2), sent to
 * `POST /api/v1/agents/runs/{run_id}/plan-approval`'s `edited_steps`. Exported
 * so `chat-client.ts`'s `approvePlan`/`rejectPlan` (the only callers today)
 * share this exact wire shape instead of drifting.
 */
export type PlanStepEdit = {
  /** Existing step id being edited. Omit/empty for a NEW step. */
  stepId?: string
  title: string
  operation?: string
  /** True when the user removed this step (marked skipped server-side). */
  removed?: boolean
}

/** `PlanStepEdit` → the exact snake_case shape the plan-approval route expects. */
export function planStepEditToWire(edit: PlanStepEdit): Record<string, unknown> {
  return {
    step_id: edit.stepId ?? '',
    title: edit.title,
    operation: edit.operation ?? '',
    removed: edit.removed === true,
  }
}

/**
 * Normalize a model-gateway plan object (snake_case) to our camelCase shape.
 * Exported so `chat-client.ts`'s `approvePlan`/`rejectPlan` responses (which
 * carry the same `plan_value` wire shape as `listPlans`) parse identically
 * instead of duplicating this logic.
 */
export function normalizePlan(raw: RawPlan): Plan | null {
  const id = str(raw.id) ?? str(raw.plan_id) ?? str(raw.planId)
  if (!id) return null
  const plan: Plan = {
    id,
    runId: str(raw.run_id) ?? str(raw.runId),
    state: str(raw.state) ?? str(raw.status),
    summary: str(raw.summary) ?? str(raw.detail) ?? str(raw.description),
  }
  const threadId = str(raw.thread_id) ?? str(raw.threadId)
  if (threadId) plan.threadId = threadId
  const author = str(raw.author)
  if (author) plan.author = author
  const rawSteps = Array.isArray(raw.steps) ? raw.steps : undefined
  if (rawSteps) {
    plan.steps = rawSteps
      .map((item): PlanStep | null => {
        if (!item || typeof item !== 'object' || Array.isArray(item)) return null
        const step = item as Record<string, unknown>
        const stepId = str(step.id) ?? str(step.step_id) ?? str(step.stepId)
        const title = str(step.title) ?? str(step.name) ?? str(step.operation)
        if (!stepId || !title) return null
        return {
          id: stepId,
          title,
          operation: str(step.operation),
          state: str(step.state) ?? str(step.status),
          detail: str(step.detail),
        }
      })
      .filter((step): step is PlanStep => step !== null)
  }
  const supersedes = str(raw.supersedes) ?? str(raw.supersedes_id) ?? str(raw.supersedesId)
  if (supersedes) plan.supersedes = supersedes
  return plan
}

function normalizeTodo(raw: RawTodo): Todo | null {
  const id = str(raw.id) ?? str(raw.todo_id) ?? str(raw.todoId)
  if (!id) return null
  const todo: Todo = {
    id,
    threadId: str(raw.thread_id) ?? str(raw.threadId),
    state: str(raw.state) ?? str(raw.status),
    title: str(raw.title) ?? str(raw.summary) ?? str(raw.detail) ?? str(raw.description),
  }
  const runId = str(raw.run_id) ?? str(raw.runId)
  if (runId) todo.runId = runId
  const description = str(raw.description)
  if (description) todo.description = description
  const priority = str(raw.priority)
  if (priority) todo.priority = priority
  const blockedBy = Array.isArray(raw.blocked_by)
    ? raw.blocked_by
    : Array.isArray(raw.blockedBy)
      ? raw.blockedBy
      : undefined
  if (blockedBy) {
    todo.blockedBy = blockedBy.filter(
      (item): item is string => typeof item === 'string' && item.trim().length > 0,
    )
  }
  return todo
}

/** List a run's plans (initial hydration for the run console). */
export async function listPlans(runId: string, signal?: AbortSignal): Promise<Plan[]> {
  const payload = await requestJson<{ plans?: RawPlan[] }>(
    `/api/v1/orchestration/runs/${encodeURIComponent(runId)}/plans`,
    { signal },
  )
  const list = Array.isArray(payload.plans) ? payload.plans : []
  return list.map(normalizePlan).filter((p): p is Plan => p !== null)
}

function isAbortSignal(value: AbortSignal | { runId?: string; signal?: AbortSignal }): value is AbortSignal {
  return Boolean(value && typeof value === 'object' && 'aborted' in value && 'addEventListener' in value)
}

/** List a thread's todos (initial hydration for the run console). */
export async function listTodos(
  threadId: string,
  options: AbortSignal | { runId?: string; signal?: AbortSignal } = {},
): Promise<Todo[]> {
  const normalized = isAbortSignal(options) ? { signal: options } : options
  const query = normalized.runId ? `?run_id=${encodeURIComponent(normalized.runId)}` : ''
  const payload = await requestJson<{ todos?: RawTodo[] }>(
    `/api/v1/orchestration/threads/${encodeURIComponent(threadId)}/todos${query}`,
    { signal: normalized.signal },
  )
  const list = Array.isArray(payload.todos) ? payload.todos : []
  return list.map(normalizeTodo).filter((t): t is Todo => t !== null)
}

/**
 * Fetch a thread's sub-agent lineage tree. The shape is provider-defined and
 * rendered by the console as-is, so we return the raw payload unparsed.
 */
export async function getLineage(threadId: string, signal?: AbortSignal): Promise<unknown> {
  return requestJson<unknown>(
    `/api/v1/orchestration/threads/${encodeURIComponent(threadId)}/lineage`,
    { signal },
  )
}

/**
 * One lineage edge as the wide-research grid reads it back after a reload (EP-2 S9). `goalSummary`
 * and `batchId` are what the fan-out stored when the item started; `status` is the edge's own
 * (`attached`/`detached`), not how the item ended, which is the child run's status.
 */
export type LineageEdgeSummary = {
  parentRunId: string
  childRunId: string
  goalSummary: string
  batchId: string
  status: string
}

function lineageText(value: unknown): string {
  return typeof value === 'string' ? value : ''
}

/**
 * The edges of a lineage payload (`{ lineage: { edges } }`, or the inner object). An edge without a
 * child run id is dropped; a missing goal, batch or status reads as empty, as an edge stored before
 * those fields existed comes back.
 */
export function normalizeLineageEdges(payload: unknown): LineageEdgeSummary[] {
  if (!payload || typeof payload !== 'object') return []
  const root = payload as Record<string, unknown>
  const lineage = root.lineage && typeof root.lineage === 'object'
    ? root.lineage as Record<string, unknown>
    : root
  if (!Array.isArray(lineage.edges)) return []
  return lineage.edges.flatMap((edge): LineageEdgeSummary[] => {
    if (!edge || typeof edge !== 'object' || Array.isArray(edge)) return []
    const raw = edge as Record<string, unknown>
    const childRunId = lineageText(raw.child_run_id)
    if (!childRunId) return []
    return [{
      parentRunId: lineageText(raw.parent_run_id),
      childRunId,
      goalSummary: lineageText(raw.goal_summary),
      batchId: lineageText(raw.batch_id),
      status: lineageText(raw.status),
    }]
  })
}

/** A thread's lineage edges, parsed ({@link normalizeLineageEdges}). */
export async function getLineageEdges(
  threadId: string,
  signal?: AbortSignal,
): Promise<LineageEdgeSummary[]> {
  return normalizeLineageEdges(await getLineage(threadId, signal))
}

/**
 * The child runs a parent run started, read from its thread's lineage (EP-2): every edge whose parent is
 * `parentRunId`, in the order the lineage holds them. Read-only. An edge's `status` is the edge's own
 * (`attached`/`detached`); the child run's own status is read with the run, not here.
 */
export async function listChildRuns(
  threadId: string,
  parentRunId: string,
  signal?: AbortSignal,
): Promise<LineageEdgeSummary[]> {
  const edges = normalizeLineageEdges(await getLineage(threadId, signal))
  return edges.filter((edge) => edge.parentRunId === parentRunId)
}

// ── Run questions (build plan WS14 S3) ───────────────────────────────────────
// A question a run put to the person. It is its own store (`run_questions`), never an
// approval: an answer is information and grants nothing. Dark until a later slice creates
// questions, so nothing calls these yet.

export type RunQuestionState = 'requested' | 'answered' | 'skipped' | 'expired' | 'cancelled'

/** The closed item kinds. There is no kind for a secret or a credential. */
export type QuestionItemKind = 'single' | 'multi' | 'text'

export type QuestionOption = { id: string; label: string; description?: string }

export type QuestionItem = {
  id: string
  kind: QuestionItemKind
  prompt: string
  /** Two to six for `single` and `multi`; empty for `text`. */
  options: QuestionOption[]
  /** A `single` or `multi` item also accepts a free-text answer. */
  allowOther: boolean
}

/** The person's answer to one item. */
export type QuestionAnswer = { itemId: string; optionIds: string[]; text: string }

export type RunQuestion = {
  id: string
  runId: string
  threadId: string
  stepId?: string
  state: RunQuestionState
  /** `next_turn`: the answer is the next message. `resume`: it continues the same run. */
  delivery: 'next_turn' | 'resume'
  items: QuestionItem[]
  requestedAt?: string
  expiresAt?: string
  answeredAt?: string
  answers: QuestionAnswer[]
  answeredMessageId?: string
}

const RUN_QUESTION_STATES: readonly RunQuestionState[] = [
  'requested',
  'answered',
  'skipped',
  'expired',
  'cancelled',
]
const QUESTION_ITEM_KINDS: readonly QuestionItemKind[] = ['single', 'multi', 'text']

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
}

function stringList(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string' && item.length > 0)
    : []
}

function normalizeQuestionItem(raw: unknown): QuestionItem | null {
  if (!isRecord(raw)) return null
  const id = str(raw.id)
  const prompt = str(raw.prompt)
  const kind = QUESTION_ITEM_KINDS.find((candidate) => candidate === raw.kind)
  if (!id || !prompt || !kind) return null
  const options: QuestionOption[] = []
  for (const rawOption of Array.isArray(raw.options) ? raw.options : []) {
    if (!isRecord(rawOption)) return null
    const optionId = str(rawOption.id)
    const label = str(rawOption.label)
    if (!optionId || !label) return null
    const description = str(rawOption.description)
    options.push(description ? { id: optionId, label, description } : { id: optionId, label })
  }
  return { id, kind, prompt, options, allowOther: raw.allow_other === true }
}

/**
 * Normalize a model-gateway question (snake_case) to our camelCase shape. A question this
 * client cannot render whole (an unknown state, an item kind it does not know, a malformed
 * option) is dropped rather than shown half: the person is never asked something the card
 * cannot present.
 */
export function normalizeRunQuestion(raw: unknown): RunQuestion | null {
  if (!isRecord(raw)) return null
  const id = str(raw.id)
  const runId = str(raw.run_id)
  const threadId = str(raw.thread_id)
  const state = RUN_QUESTION_STATES.find((candidate) => candidate === raw.state)
  const delivery = raw.delivery === 'next_turn' || raw.delivery === 'resume' ? raw.delivery : null
  if (!id || !runId || !threadId || !state || !delivery || !Array.isArray(raw.items)) return null
  const items: QuestionItem[] = []
  for (const rawItem of raw.items) {
    const item = normalizeQuestionItem(rawItem)
    if (!item) return null
    items.push(item)
  }
  if (items.length === 0) return null
  const answers: QuestionAnswer[] = []
  for (const rawAnswer of Array.isArray(raw.answers) ? raw.answers : []) {
    if (!isRecord(rawAnswer)) return null
    const itemId = str(rawAnswer.item_id)
    if (!itemId) return null
    answers.push({
      itemId,
      optionIds: stringList(rawAnswer.option_ids),
      text: typeof rawAnswer.text === 'string' ? rawAnswer.text : '',
    })
  }
  const question: RunQuestion = { id, runId, threadId, state, delivery, items, answers }
  const stepId = str(raw.step_id)
  if (stepId) question.stepId = stepId
  const requestedAt = str(raw.requested_at)
  if (requestedAt) question.requestedAt = requestedAt
  const expiresAt = str(raw.expires_at)
  if (expiresAt) question.expiresAt = expiresAt
  const answeredAt = str(raw.answered_at)
  if (answeredAt) question.answeredAt = answeredAt
  const answeredMessageId = str(raw.answered_message_id)
  if (answeredMessageId) question.answeredMessageId = answeredMessageId
  return question
}

/**
 * List the signed-in person's own questions for a run, oldest first. Another person's
 * questions are never returned. Rejects with the typed `ApiError`: a zero-retention or waived
 * credential is refused (`zdr_durable_mutation_forbidden`, `zdr_waiver_route_mismatch`).
 */
export async function listRunQuestions(runId: string, signal?: AbortSignal): Promise<RunQuestion[]> {
  const payload = await requestJson<{ questions?: unknown[] }>(
    `/api/v1/orchestration/runs/${encodeURIComponent(runId)}/questions`,
    { signal },
  )
  const list = Array.isArray(payload.questions) ? payload.questions : []
  return list.map(normalizeRunQuestion).filter((question): question is RunQuestion => question !== null)
}

/** What the person gave: an answer for every item, or a skip. */
export type RunQuestionResponse = { answers: QuestionAnswer[] } | { skip: true }

/**
 * The closed set of codes an answer can be refused with, each under its own status: 404
 * `question_not_found` (another person's question answers the same), 410 `question_expired`,
 * 409 `question_already_answered`, 422 `invalid_answer`. The transport error carries the code
 * and message only, so the refused field and reason stay in the response body. A zero-retention
 * refusal is read with the existing ZDR refusal guards.
 */
export const RUN_QUESTION_ERRORS = {
  question_not_found: 404,
  question_expired: 410,
  question_already_answered: 409,
  invalid_answer: 422,
} as const

export type RunQuestionErrorCode = keyof typeof RUN_QUESTION_ERRORS

/** The typed question error a rejection carries, or null for any other failure. */
export function runQuestionErrorCodeOf(error: unknown): RunQuestionErrorCode | null {
  if (!(error instanceof ApiError)) return null
  const { code, status } = error
  if (code === null || !Object.hasOwn(RUN_QUESTION_ERRORS, code)) return null
  const known = code as RunQuestionErrorCode
  return RUN_QUESTION_ERRORS[known] === status ? known : null
}

/**
 * Answer, or skip, one question. An identical retry is accepted again and returns the stored
 * question; a different second answer is refused (see `RUN_QUESTION_ERRORS`). Only the answers
 * or the skip go on the wire: the org, the owner and the message that carries the answer are
 * the server's.
 */
export async function answerQuestion(
  questionId: string,
  response: RunQuestionResponse,
  signal?: AbortSignal,
): Promise<RunQuestion | null> {
  const body =
    'skip' in response
      ? { answers: [], skip: true }
      : {
          answers: response.answers.map((answer) => ({
            item_id: answer.itemId,
            option_ids: answer.optionIds,
            text: answer.text,
          })),
          skip: false,
        }
  const payload = await requestJson<{ question?: unknown }>(
    `/api/v1/orchestration/questions/${encodeURIComponent(questionId)}/answer`,
    { method: 'POST', body: JSON.stringify(body), signal },
  )
  return normalizeRunQuestion(payload.question)
}
