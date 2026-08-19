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

export function getSpaceContext(spaceRef: string): Promise<SpaceContext> {
  return requestJson(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/context`)
}

export function getSpaceThreads(spaceRef: string): Promise<SpaceThreads> {
  return requestJson(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/threads`)
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

export function requestPersonalSpaceDeletion(spaceRef: string, idempotencyKey: string): Promise<SpaceDeletionRequest> {
  return requestJson(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/deletion-requests`, {
    method: 'POST',
    body: JSON.stringify({ idempotency_key: idempotencyKey }),
  })
}

export function getPersonalSpaceDeletionReceipt(requestId: string): Promise<SpaceDeletionReceipt> {
  return requestJson(`/api/v1/spaces/deletion-requests/${encodeURIComponent(requestId)}`)
}
