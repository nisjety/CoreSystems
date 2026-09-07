import { requestJson } from './http'

export type SpaceMembership = {
  space_ref: string
  org_id: string
  subject_id: string
  kind: 'personal' | 'shared' | string
  role: string
  revisions: {
    authority: number
    membership: number
    privacy: number
    recipient_audience: number
    entitlement: number
  }
}

export type SpaceSummary = {
  space_ref: string
  name: string
  kind: string
  lifecycle: string
  /** True for the organization's own shared room (its org-wide channel).
   * Absent on older gateway responses and on the Control-outage fallback
   * listing, so absence means "unknown", not "no". Callers that gate an
   * ACTION on this must test `=== false`, never `!value`: the room whose
   * membership is derived is the one where an editor would silently fail. */
  is_organization_room?: boolean
}

/** A server-composed lifecycle + current-membership read. It contains no
 * signed decision, recipient audience, or owner-resource authority. */
export type SpaceContext = {
  space: SpaceSummary
  membership: SpaceMembership
}

/** Owner-bound conversation index for the selected Space. The BFF has already
 * rechecked lifecycle and membership; this remains a read model, not a grant. */
export type SpaceThread = {
  thread_id: string
  space_id: string
  /**
   * Who started this post. Present once Model Plane returns it; absent on an
   * older gateway. Absence means "unknown author", never "you".
   */
  owner_subject_id?: string
  title?: string
  preview?: string
  updated_at?: string
  latest_run_id?: string
  latest_run_status?: string
  latest_run_updated_at?: string
}

export type SpaceThreads = {
  space: SpaceSummary
  membership: SpaceMembership
  threads: readonly SpaceThread[]
}

/** Actor-filtered owner contracts available in a Space. This is an availability
 * view only: each owner plane reauthorizes the exact resource at execution. */
export type SpaceActions = {
  space_ref: string
  actor_type: 'human'
  catalog: {
    catalogVersion: string
    actorType: 'human'
    actions: readonly unknown[]
  }
}

export type SpaceDeletionRequest = {
  requestId: string
  idempotencyKey: string
  spaceRef: string
  externalOrgId: string
  ownerExternalAuthId: string
  state: 'pending_authorization' | 'authorized' | 'blocked_legal_hold' | 'rejected'
  createdAt: number
  updatedAt: number
}

export type SpaceDeletionOwnerReceipt = {
  ownerPlane: 'application' | 'control' | 'data' | 'ingestion' | 'model' | 'infra'
  status: 'pending' | 'blocked_legal_hold' | 'succeeded' | 'partial' | 'failed' | 'unknown'
  receiptRef?: string
  detail?: string
  updatedAt: number
}

/** Authorization and purge progress stay separate. `authorized` does not mean
 * deleted; `purgeStatus` is successful only after every owner reports it. */
export type SpaceDeletionReceipt = {
  request: SpaceDeletionRequest
  receipts: readonly SpaceDeletionOwnerReceipt[]
  purgeStatus: SpaceDeletionOwnerReceipt['status']
}

/** Current Control membership only; it deliberately contains no bearer,
 * recipient audience, or resource decision that a browser could replay. */
export function getSpaceMembership(spaceRef: string): Promise<SpaceMembership> {
  return requestJson(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/membership`)
}

export async function listSpaces(): Promise<readonly SpaceSummary[]> {
  const response = await requestJson<{ spaces: readonly SpaceSummary[] }>('/api/v1/spaces')
  return response.spaces
}

/**
 * ADR-0003's Space layer of the authored-instruction hierarchy. Any active
 * Space member (viewer included) may read it — it shapes every turn a viewer
 * takes part in too, not only an editor's; writing is gated at the gateway to
 * `editor`/`manager`/`owner` (`spaces.rs::SPACE_INSTRUCTIONS_WRITE_ROLES`).
 */
export async function getSpaceInstructions(spaceRef: string): Promise<string> {
  const response = await requestJson<{ instructions: string | null }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/instructions`,
  )
  return response.instructions ?? ''
}

export async function updateSpaceInstructions(spaceRef: string, instructions: string): Promise<string> {
  const response = await requestJson<{ instructions: string | null }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/instructions`,
    {
      method: 'PATCH',
      body: JSON.stringify({ instructions }),
    },
  )
  return response.instructions ?? ''
}

export function getSpaceContext(spaceRef: string): Promise<SpaceContext> {
  return requestJson(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/context`)
}

export function getSpaceThreads(spaceRef: string): Promise<SpaceThreads> {
  return requestJson(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/threads`)
}

/** One evidence section this response deliberately does not claim, with why. */
export type SpaceWorkGap = {
  readonly section: 'runs' | 'schedules' | string
  /**
   * Stable identifier for the reason, so the UI can say it in the reader's
   * language. Absent on an older gateway, which is why `reason` is still sent
   * and still rendered when the code is unrecognised — a gap stated in the
   * wrong language beats a gap not stated.
   */
  readonly code?: string
  readonly reason: string
}

/**
 * What a room has running and scheduled.
 *
 * Composed by the gateway from two upstreams with two different authorities:
 * runs come through the same shared-read decision the transcript uses, so Work
 * reaches exactly as far as Chat; schedules are filtered by Space over a
 * listing that was already org-readable.
 *
 * `unavailable` is always present and may be non-empty alongside real data —
 * either upstream can fail on its own. A caller must render the gap rather
 * than treating a short list as the whole answer: the Work tab's entire job is
 * "what needs me", and a room with pending work that looks idle is the one
 * failure mode worth designing against.
 */
export type SpaceWork = {
  readonly space: SpaceSummary
  readonly membership: SpaceMembership
  readonly runs: readonly Record<string, unknown>[]
  readonly schedules: readonly Record<string, unknown>[]
  readonly unavailable: readonly SpaceWorkGap[]
}

function workGaps(value: unknown): SpaceWorkGap[] {
  if (!Array.isArray(value)) return []
  const gaps: SpaceWorkGap[] = []
  for (const entry of value) {
    if (!entry || typeof entry !== 'object') continue
    const record = entry as Record<string, unknown>
    const section = typeof record.section === 'string' ? record.section : ''
    const reason = typeof record.reason === 'string' ? record.reason : ''
    const code = typeof record.code === 'string' && record.code.trim() ? record.code.trim() : undefined
    if (section && reason) gaps.push({ section, reason, ...(code ? { code } : {}) })
  }
  return gaps
}

function workRows(value: unknown): Record<string, unknown>[] {
  return Array.isArray(value)
    ? value.filter((row): row is Record<string, unknown> => Boolean(row) && typeof row === 'object')
    : []
}

export async function getSpaceWork(spaceRef: string): Promise<SpaceWork> {
  const response = await requestJson<{
    space: SpaceSummary
    membership: SpaceMembership
    runs?: unknown
    schedules?: unknown
    unavailable?: unknown
  }>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/work`)
  return {
    space: response.space,
    membership: response.membership,
    runs: workRows(response.runs),
    schedules: workRows(response.schedules),
    unavailable: workGaps(response.unavailable),
  }
}

/**
 * One owner-plane effect performed under this Space's authority.
 *
 * Correlated through the grant, not a Space column: the operation ledger binds
 * each effect to the exact grant that authorized it, and the grant carries the
 * `space_ref` Control decided. So an effect appears in a room's record only if
 * it was genuinely authorized for that room.
 *
 * `status` includes `unknown`, which is a real terminal answer and NOT a
 * synonym for failure — the effect may have landed and the receipt may not have
 * come back. `ticket_id` is present only on `completed`; its absence means the
 * effect is not claimed to have landed.
 */
export type SpaceOperationReceipt = {
  readonly operation_id: string
  readonly action_id: string
  readonly status: 'pending_control_commit' | 'reserved' | 'completed' | 'cancelled' | 'unknown' | string
  readonly subject_id?: string
  readonly granted_by_user_id?: string
  readonly conversation_id?: string
  readonly ticket_id?: string
  readonly audit_event_id?: string
  readonly terminal_reason?: string
  readonly created_at?: string
  readonly updated_at?: string
}

/**
 * One owner-side grant of an effect in this Space.
 *
 * The resource half of `effective_access`: Control decides whether a subject
 * may act in the Space at all, and this records that the owner plane also
 * admitted it to one conversation and action. A revoked grant is the more
 * useful of the two — "this agent could write tickets here until Tuesday" is
 * what explains a refusal after the fact.
 */
export type SpaceAuthorityEvent = {
  readonly grant_id: string
  readonly action_id: string
  readonly subject_id?: string
  readonly conversation_id?: string
  readonly created_by_user_id?: string
  readonly created_at?: string
  readonly revoked_at?: string
  readonly revoked_by_user_id?: string
}

/**
 * What has happened in this Space, from every plane that can currently say.
 *
 * Activity and Work read overlapping evidence to answer different questions:
 * Work asks what is in flight, Activity asks what happened, on whose authority,
 * and with what outcome. So `runs` here keeps terminal runs and carries the
 * token/step counts a run has always recorded and Work never used.
 *
 * `unavailable` always includes the evidence classes that have no source yet
 * (durable delivery, watches), because the tab has promised since the cockpit
 * shipped that they join when their projection lands — a reader deserves to
 * know which are still out rather than reading their absence as calm.
 */
export type SpaceActivity = {
  readonly space: SpaceSummary
  readonly membership: SpaceMembership
  readonly runs: readonly Record<string, unknown>[]
  readonly approvals: readonly Record<string, unknown>[]
  readonly operations: readonly SpaceOperationReceipt[]
  readonly authority: readonly SpaceAuthorityEvent[]
  readonly unavailable: readonly SpaceWorkGap[]
}

function operationReceipts(value: unknown): SpaceOperationReceipt[] {
  return workRows(value)
    .filter((row) => typeof row.operation_id === 'string' && row.operation_id.trim())
    .map((row) => ({
      operation_id: String(row.operation_id),
      action_id: typeof row.action_id === 'string' ? row.action_id : '',
      status: typeof row.status === 'string' ? row.status : 'unknown',
      subject_id: typeof row.subject_id === 'string' ? row.subject_id : undefined,
      granted_by_user_id:
        typeof row.granted_by_user_id === 'string' ? row.granted_by_user_id : undefined,
      conversation_id: typeof row.conversation_id === 'string' ? row.conversation_id : undefined,
      ticket_id: typeof row.ticket_id === 'string' && row.ticket_id ? row.ticket_id : undefined,
      audit_event_id:
        typeof row.audit_event_id === 'string' && row.audit_event_id ? row.audit_event_id : undefined,
      terminal_reason:
        typeof row.terminal_reason === 'string' && row.terminal_reason ? row.terminal_reason : undefined,
      created_at: typeof row.created_at === 'string' ? row.created_at : undefined,
      updated_at: typeof row.updated_at === 'string' ? row.updated_at : undefined,
    }))
}

function authorityEvents(value: unknown): SpaceAuthorityEvent[] {
  return workRows(value)
    .filter((row) => typeof row.grant_id === 'string' && row.grant_id.trim())
    .map((row) => ({
      grant_id: String(row.grant_id),
      action_id: typeof row.action_id === 'string' ? row.action_id : '',
      subject_id: typeof row.subject_id === 'string' ? row.subject_id : undefined,
      conversation_id: typeof row.conversation_id === 'string' ? row.conversation_id : undefined,
      created_by_user_id:
        typeof row.created_by_user_id === 'string' ? row.created_by_user_id : undefined,
      created_at: typeof row.created_at === 'string' ? row.created_at : undefined,
      revoked_at: typeof row.revoked_at === 'string' && row.revoked_at ? row.revoked_at : undefined,
      revoked_by_user_id:
        typeof row.revoked_by_user_id === 'string' && row.revoked_by_user_id
          ? row.revoked_by_user_id
          : undefined,
    }))
}

export async function getSpaceActivity(spaceRef: string): Promise<SpaceActivity> {
  const response = await requestJson<{
    space: SpaceSummary
    membership: SpaceMembership
    runs?: unknown
    approvals?: unknown
    operations?: unknown
    authority?: unknown
    unavailable?: unknown
  }>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/activity`)
  return {
    space: response.space,
    membership: response.membership,
    runs: workRows(response.runs),
    approvals: workRows(response.approvals),
    operations: operationReceipts(response.operations),
    authority: authorityEvents(response.authority),
    unavailable: workGaps(response.unavailable),
  }
}

/** One document in this Space's knowledge scope. */
export type SpaceKnowledgeDocument = {
  readonly document_id: string
  readonly title: string
  readonly source?: string
  readonly type?: string
  readonly status?: string
  readonly zdr_classification?: string
  readonly updated_at?: string
}

/** One published wiki page in the workspace this Space is bound to. */
export type SpaceKnowledgeWikiPage = {
  readonly page_id: string
  readonly title: string
  readonly path?: string
  readonly updated_at?: string
}

/**
 * The Data target this Space resolves to.
 *
 * Shown, not hidden: which archive a room reads from is the single most
 * consequential fact about its answers, and a room whose binding names a
 * workspace nobody expected is a configuration error you can only see if the
 * binding is visible. `null` when the read was not authorized at all.
 */
export type SpaceKnowledgeBinding = {
  readonly workspace_id?: string | null
  readonly collection_id?: string | null
  readonly owner_resource_ref?: string
}

/**
 * What a room knows about.
 *
 * Documents resolve through `documents.space_ref` — the edge Data records when
 * a document is imported under a verified Control Space decision. Wiki pages
 * resolve through the workspace the Space's `space_retrieval_bindings` row
 * names. Two mechanisms, two ways to be missing, hence a per-section
 * `unavailable`.
 *
 * `documents_truncated` says the list is a page rather than the room: a reader
 * cannot otherwise tell a capped listing from a complete archive, and quietly
 * showing the first fifty of five hundred is how someone concludes a document
 * is not in the room.
 */
export type SpaceKnowledge = {
  readonly space: SpaceSummary
  readonly membership: SpaceMembership
  readonly binding: SpaceKnowledgeBinding | null
  readonly documents: readonly SpaceKnowledgeDocument[]
  readonly documents_truncated: boolean
  readonly wiki_pages: readonly SpaceKnowledgeWikiPage[]
  readonly unavailable: readonly SpaceWorkGap[]
}

function knowledgeDocuments(value: unknown): SpaceKnowledgeDocument[] {
  return workRows(value)
    .filter((row) => typeof row.document_id === 'string' && row.document_id.trim())
    .map((row) => ({
      document_id: String(row.document_id),
      // A document with no title is a real row, so it still belongs in the
      // list; the panel names it rather than dropping it.
      title: typeof row.title === 'string' ? row.title : '',
      source: typeof row.source === 'string' ? row.source : undefined,
      type: typeof row.type === 'string' ? row.type : undefined,
      status: typeof row.status === 'string' ? row.status : undefined,
      zdr_classification:
        typeof row.zdr_classification === 'string' ? row.zdr_classification : undefined,
      updated_at: typeof row.updated_at === 'string' ? row.updated_at : undefined,
    }))
}

function knowledgeWikiPages(value: unknown): SpaceKnowledgeWikiPage[] {
  return workRows(value)
    .filter((row) => typeof row.page_id === 'string' && row.page_id.trim())
    .map((row) => ({
      page_id: String(row.page_id),
      title: typeof row.title === 'string' ? row.title : '',
      path: typeof row.path === 'string' ? row.path : undefined,
      updated_at: typeof row.updated_at === 'string' ? row.updated_at : undefined,
    }))
}

export async function getSpaceKnowledge(spaceRef: string): Promise<SpaceKnowledge> {
  const response = await requestJson<{
    space: SpaceSummary
    membership: SpaceMembership
    binding?: unknown
    documents?: unknown
    documents_truncated?: unknown
    wiki_pages?: unknown
    unavailable?: unknown
  }>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/knowledge`)
  const binding =
    response.binding && typeof response.binding === 'object'
      ? (response.binding as SpaceKnowledgeBinding)
      : null
  return {
    space: response.space,
    membership: response.membership,
    binding,
    documents: knowledgeDocuments(response.documents),
    documents_truncated: response.documents_truncated === true,
    wiki_pages: knowledgeWikiPages(response.wiki_pages),
    unavailable: workGaps(response.unavailable),
  }
}

/** One participant of a Space as shown to another participant. */
export type SpaceRosterMember = {
  subject_type: 'user' | 'service'
  subject_id: string
  role: 'viewer' | 'editor' | 'manager' | 'owner'
  revision: number
  /** May be empty: a membership can exist before its user projection does. */
  display_name: string
}

/**
 * Who is in this Space. Control gates it on your own membership and answers 404
 * when you are not in the room — that is "you cannot see this", not "the room
 * is empty", and the two must stay distinguishable in the UI.
 */
export async function getSpaceRoster(spaceRef: string): Promise<readonly SpaceRosterMember[]> {
  const response = await requestJson<{ members: readonly SpaceRosterMember[] }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/roster`,
  )
  return response.members
}

/** One turn of a Space thread, as the room reads it. */
export type SpaceThreadTurn = {
  readonly role: 'user' | 'assistant' | 'system' | 'tool'
  readonly content: string
  /** The persona an assistant turn answered as, recorded at the time. */
  readonly agentName?: string
  /**
   * Which member wrote this turn, from Model Plane's at-the-time record.
   *
   * Absent on assistant turns (use `agentName`), on system/tool turns, and on
   * rows written before authorship was recorded. Absent must render as an
   * unnamed author — never as the reader, which is what the room did while
   * every transcript was owner-bound and one name was always right by accident.
   */
  readonly authorSubjectId?: string
}

export type SpaceThreadTranscript = {
  readonly threadId: string
  readonly turns: readonly SpaceThreadTurn[]
}

function normalizeSpaceThreadTurns(value: unknown): readonly SpaceThreadTurn[] {
  if (!Array.isArray(value)) return []
  const turns: SpaceThreadTurn[] = []
  for (const entry of value) {
    if (!entry || typeof entry !== 'object') continue
    const record = entry as Record<string, unknown>
    const role = record.role
    const content = typeof record.content === 'string' ? record.content.trim() : ''
    if (role !== 'user' && role !== 'assistant' && role !== 'system' && role !== 'tool') continue
    if (!content) continue
    const agentName = typeof record.agent_name === 'string' && record.agent_name.trim()
      ? record.agent_name.trim()
      : typeof record.agentName === 'string' && record.agentName.trim()
        ? record.agentName.trim()
        : undefined
    const authorSubjectId =
      typeof record.author_subject_id === 'string' && record.author_subject_id.trim()
        ? record.author_subject_id.trim()
        : typeof record.authorSubjectId === 'string' && record.authorSubjectId.trim()
          ? record.authorSubjectId.trim()
          : undefined
    turns.push({
      role,
      content,
      ...(agentName ? { agentName } : {}),
      ...(authorSubjectId ? { authorSubjectId } : {}),
    })
  }
  return turns
}

/**
 * One Space thread's transcript, read through the room's own authority.
 *
 * Distinct from Chat's `getChatThreadTranscript`, which resolves a thread only
 * inside the caller's own durable list — correct for a personal chat history,
 * and the reason a colleague's post in a shared room could previously render
 * as nothing but its preview. This route asks Control whether the caller is a
 * current recipient of the Space instead, so the room can show what the room
 * actually said.
 *
 * A 403 here is a real answer, not a glitch: the organization has not enabled
 * shared reads for this Space (deny-by-default), so the caller may take part in
 * the room without being admitted to other members' turns.
 */
export async function getSpaceThreadTranscript(
  spaceRef: string,
  threadId: string,
): Promise<SpaceThreadTranscript> {
  const response = await requestJson<{ transcript?: { threadId?: string; turns?: unknown } }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/threads/${encodeURIComponent(threadId)}/transcript`,
  )
  return {
    threadId: response.transcript?.threadId ?? threadId,
    turns: normalizeSpaceThreadTurns(response.transcript?.turns),
  }
}

/** A channel a bound agent's work can reach, beyond the room itself. */
export type SpaceAgentDeliveryTarget = {
  channel: 'teams' | 'messenger' | 'embed'
  /** Operator-facing destination label. Never a token or credential. */
  label: string
  status: 'active' | 'pending' | 'failed'
}

/**
 * One agent participating in a Space.
 *
 * Two planes fill this in, and the split is load-bearing
 * (`docs/SPACE_AGENT_SCOPE_PLAN_2026-08-14.md` §3.2): `role` and `revision` come
 * from Control, which decides who may act in the room, while the identity
 * fields come from an Application binding. `identity_published` is false when
 * Control authorizes an agent that Application has not described yet — a real
 * state that must render as a gap in presentation, never as an absent agent
 * and never under an invented name.
 */
export type SpaceAgent = {
  subject_id: string
  role: 'viewer' | 'editor' | 'manager' | 'owner'
  revision: number
  identity_published: boolean
  binding_ref?: string
  agent_ref?: string
  name?: string
  title?: string
  description?: string
  /** Binding lifecycle. Absent when no identity is published. */
  status?: 'pending' | 'active' | 'paused' | 'revoked' | 'failed'
  /** The definition's own lifecycle, which can differ from the binding's. */
  definition_status?: 'active' | 'inactive' | 'draft'
  /**
   * Binding policy, enforced by the gateway at invocation. Absent on bindings
   * created before policy fields existed — absence means the legacy behavior,
   * so the UI must not render a policy it cannot prove.
   */
  trigger_modes?: readonly ('mention' | 'group')[]
  allowed_tools?: readonly string[]
  approval_mode?: 'auto' | 'require_confirmation' | 'blocked'
  delivery_targets: readonly SpaceAgentDeliveryTarget[]
  updated_at?: number
}

/**
 * The agents bound to this Space. Control gates it on your own membership, so a
 * failure here means "your view could not be resolved" — never "this room has
 * no agents", and the panel keeps those two apart.
 */
export async function getSpaceAgents(spaceRef: string): Promise<readonly SpaceAgent[]> {
  const response = await requestJson<{ agents: readonly SpaceAgent[] }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/agents`,
  )
  return response.agents
}

export function getSpaceActions(spaceRef: string): Promise<SpaceActions> {
  return requestJson(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/actions`)
}

export type CreateSpaceAgentInput = {
  name: string
  instructions?: string
  avatarColor?: string
}

export type CreatedSpaceAgent = {
  agent_ref: string
  subject_id: string
  status: string
}

/**
 * Create a simple agent from inside the room (scope plan §UI-3b). The gateway
 * runs the governed two-step flow — Application definition + pending binding,
 * then Control roster confirmation — under the caller's verified session and
 * room role; nothing here carries identity or authority. A rejection with
 * `agent_membership_unconfirmed` means the agent exists but stays `pending`
 * in the Agent tab until Control accepts the roster.
 */
export function createSpaceAgent(spaceRef: string, input: CreateSpaceAgentInput): Promise<CreatedSpaceAgent> {
  return requestJson(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/agents`, {
    method: 'POST',
    body: JSON.stringify({
      name: input.name.trim(),
      ...(input.instructions?.trim() ? { instructions: input.instructions.trim() } : {}),
      ...(input.avatarColor?.trim() ? { avatar_color: input.avatarColor.trim() } : {}),
    }),
  })
}

/**
 * An org agent definition the caller could add to this Space (scope plan
 * §UI-3). `already_bound` is reported, never used to filter — Control's
 * membership state, not this list, is the source of truth for who can
 * currently act in the room.
 */
export type InstallableSpaceAgent = {
  agent_ref: string
  name?: string
  description?: string
  definition_status?: 'active' | 'inactive' | 'draft'
  already_bound: boolean
}

/**
 * The org's agent definitions, for an owner/manager browsing what to add to
 * this room (scope plan §UI-3). Gated the same way as creation: only a room
 * owner/manager may call this.
 */
export async function getInstallableSpaceAgents(spaceRef: string): Promise<readonly InstallableSpaceAgent[]> {
  const response = await requestJson<{ agents: readonly InstallableSpaceAgent[] }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/agents/available`,
  )
  return response.agents
}

/**
 * Bind an EXISTING agent definition to this Space (scope plan §UI-3) — the
 * same governed two-step and room-role gate as `createSpaceAgent`, except
 * step 1 reuses a definition the caller picked from
 * `getInstallableSpaceAgents` instead of authoring a new one. The bound
 * agent's own (possibly broader) Agent Studio configuration is never
 * inherited; the binding is born with the same narrow policy as a
 * freshly-created room agent.
 */
export function bindSpaceAgent(spaceRef: string, agentRef: string): Promise<CreatedSpaceAgent> {
  return requestJson(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/agents/bind`, {
    method: 'POST',
    body: JSON.stringify({ agent_ref: agentRef }),
  })
}

/**
 * Pause or resume one bound agent, in this room only.
 *
 * The dividing rule from `space-defenition.md`: pausing HERE is a room
 * decision, and pausing everywhere is an Agent page decision. This is the
 * former, so it changes one binding and nothing about the definition or its
 * other installations.
 *
 * Server-gated to owner/manager, the same roles that may add an agent —
 * governing one is the same class of decision as granting one. A paused
 * binding stays a member of the room and stops being invokable; the gateway
 * already refuses a mention of it.
 */
export async function setSpaceAgentState(
  spaceRef: string,
  bindingRef: string,
  status: 'active' | 'paused',
): Promise<{ status: string; changed: boolean }> {
  const response = await requestJson<{ status?: string; changed?: boolean }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/agents/${encodeURIComponent(bindingRef)}`,
    {
      method: 'PATCH',
      body: JSON.stringify({ status }),
    },
  )
  return { status: response.status ?? status, changed: response.changed === true }
}

/**
 * Revoke a bound agent from this room.
 *
 * Its own verb rather than a third status on the call above, because it is the
 * one option that cannot be undone by pressing the other button: reinstating
 * means binding again, through the same grant flow as the first time.
 *
 * The binding record survives for audit history — the room keeps being able to
 * say who was here — but Control drops the agent from the room's roster, which
 * is what actually stops it acting. A `agent_revocation_unconfirmed` response
 * means the binding was revoked and Control has not converged yet, so the
 * agent may still show as a member until it does.
 */
export async function revokeSpaceAgent(
  spaceRef: string,
  bindingRef: string,
): Promise<{ status: string }> {
  const response = await requestJson<{ status?: string }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/agents/${encodeURIComponent(bindingRef)}`,
    { method: 'DELETE' },
  )
  return { status: response.status ?? 'revoked' }
}

/**
 * One agent definition and every Space it is installed in, org-wide —
 * ADR-0002 (`apps/CROSS_SPACE_AGENT_REGISTRY_ADR_2026-08-19.md`). Each
 * installation's `status` is the Application binding's own status field, the
 * same value the Space's own Agent tab reads from its binding. Unlike the
 * Agent tab's single-Space view, this list does NOT re-verify each binding
 * against Control's live per-Space roster on every read (a presence
 * registry, not an authorization surface — see the ADR) — so treat it as
 * "what's installed, where" for display, never as proof the caller may
 * invoke a listed binding.
 */
export type AgentInstallation = {
  space_ref: string
  space_name: string
  space_kind: 'personal' | 'room' | 'project' | 'case'
  status?: 'pending' | 'active' | 'paused' | 'revoked' | 'failed'
}

export type AgentDefinitionInstallations = {
  agent_ref: string
  name?: string
  description?: string
  definition_status?: 'active' | 'inactive' | 'draft'
  installations: readonly AgentInstallation[]
}

/**
 * Every agent definition with a published identity anywhere in the caller's
 * organization, grouped with its per-Space installations — ADR-0002's
 * org-wide registry (`spaceAgents:agentInstallationsForOrgForGateway`), read
 * through `GET /api/v1/agents/installations` in one call rather than the
 * narrow slice's original per-Space loop. Gated on org membership only, so
 * this can legitimately list a binding in a Space the caller cannot
 * currently act in — presence, not authority.
 */
export async function getAgentInstallations(): Promise<readonly AgentDefinitionInstallations[]> {
  const response = await requestJson<{ definitions: readonly AgentDefinitionInstallations[] }>(
    '/api/v1/agents/installations',
  )
  return response.definitions
}

/**
 * Provision the caller's own personal Space.
 *
 * Idempotent server-side — an owner has at most one personal Space, so calling
 * this twice returns the same room rather than creating a second.
 *
 * The room comes back as `pending_registration`, not active: Control registers
 * it afterwards, and Space actions stay refused until it does. Treat a
 * successful response as "the room now exists", not as "the room is ready".
 * Owner and organization are taken from the session at the gateway; nothing
 * here identifies the user.
 */
export async function createPersonalSpace(name?: string): Promise<SpaceSummary> {
  const response = await requestJson<{ space: SpaceSummary }>('/api/v1/spaces', {
    method: 'POST',
    body: JSON.stringify(name?.trim() ? { name: name.trim() } : {}),
  })
  return response.space
}

/**
 * Create a named shared room.
 *
 * Until this existed the sidebar drew a "Channels" heading over a list that
 * could only ever hold one entry, because the organization room was the only
 * room the product could make.
 *
 * Same two-step lifecycle as every other Space: the room exists when this
 * returns, and Control must register it before anything may happen in it. It
 * starts with its creator and grows by explicit grant — it is NOT an
 * organization room, so it does not inherit the organization's roster.
 */
export async function createRoom(name: string): Promise<SpaceSummary> {
  const response = await requestJson<{ space: SpaceSummary }>('/api/v1/spaces', {
    method: 'POST',
    body: JSON.stringify({ kind: 'room', name: name.trim() }),
  })
  return response.space
}

/**
 * Add one person to a named room.
 *
 * Gated to the room's owner or manager, the same floor as granting an agent:
 * deciding who may read a room's shared record is at least as consequential.
 * The person must already be in the organization — a room grant is not a way
 * into the tenant.
 *
 * A `room_membership_unconfirmed` response means the grant was recorded and
 * Control has not accepted the roster yet, so the person is not a member: the
 * room should say that rather than showing them as present.
 */
export async function addSpaceMember(
  spaceRef: string,
  memberId: string,
): Promise<{ memberCount: number; changed: boolean }> {
  const response = await requestJson<{ member_count?: number; changed?: boolean }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/members`,
    {
      method: 'POST',
      body: JSON.stringify({ member_id: memberId }),
    },
  )
  return { memberCount: response.member_count ?? 0, changed: response.changed === true }
}

/**
 * Remove one person from a named room.
 *
 * The room's registered owner cannot be removed: Control keeps them as owner
 * regardless, so dropping the grant would only make this list disagree with the
 * roster it describes.
 */
export async function removeSpaceMember(
  spaceRef: string,
  memberId: string,
): Promise<{ memberCount: number; changed: boolean }> {
  const response = await requestJson<{ member_count?: number; changed?: boolean }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/members/${encodeURIComponent(memberId)}`,
    { method: 'DELETE' },
  )
  return { memberCount: response.member_count ?? 0, changed: response.changed === true }
}

/**
 * Provision the organization's shared room — the org-wide channel every member
 * lands in.
 *
 * Idempotent server-side: an organization has at most one org room, so a
 * repeat call returns the existing one. Normally the onboarding
 * create-organization action already did this; the Spaces surface calls it
 * when the listing shows no channel, which self-heals organizations created
 * before the hook existed and retries a Convex outage during onboarding.
 *
 * Same two-step lifecycle as the personal room: the response means "the room
 * now exists", and Control must register it before it appears in the listing
 * or accepts any action.
 */
export async function ensureOrganizationRoom(name?: string): Promise<SpaceSummary> {
  const response = await requestJson<{ space: SpaceSummary }>('/api/v1/spaces/organization-room', {
    method: 'POST',
    body: JSON.stringify(name?.trim() ? { name: name.trim() } : {}),
  })
  return response.space
}

export function requestPersonalSpaceDeletion(spaceRef: string, idempotencyKey: string): Promise<SpaceDeletionRequest> {
  return requestJson(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/deletion-requests`, {
    method: 'POST',
    body: JSON.stringify({ idempotency_key: idempotencyKey }),
  })
}

export function getPersonalSpaceDeletionReceipt(requestId: string): Promise<SpaceDeletionReceipt> {
  return requestJson(`/api/v1/spaces/deletion-requests/${encodeURIComponent(requestId)}`)
}
