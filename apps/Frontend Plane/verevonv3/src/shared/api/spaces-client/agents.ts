// WS17-S5: personal agents from the SPA. The gateway (apps/gateway/src/domains/spaces/personal_agents.rs)
// relays Control's registry and Application's definitions:
//   POST   /api/v1/me/agents                               201 {data:{agent_ref, subject_id, visibility, add_policy, revision}}
//   GET    /api/v1/me/agents                               200 {data:{agents:[...], primary_subject_id}}
//   GET    /api/v1/agents/{agent_ref}/access               200 {data:{agent_ref, subject_id, role, visibility, add_policy, revision, owner_subject_id, entries}}
//   PUT    /api/v1/agents/{agent_ref}/access               200 {data:{agent_ref, subject_id, role, visibility, add_policy, revision, entries}}
//   PUT    /api/v1/me/primary-agent                        200 {data:{primary_subject_id}}
//   DELETE /api/v1/agents/{agent_ref}/rooms/{space_ref}    200 {data:{agent_ref, space_ref, state, changed}}
// Errors arrive as an ApiError carrying the typed code: 409 `agent_state_changed` (a save based on an
// older revision; `details.current_revision`), 403 `agent_not_addable` (`details.reason`, one of
// AGENT_ADD_REFUSAL_REASONS), 404 `feature_disabled` with the gateway flag off. The UI gate is
// `features.personal_agents` on GET /api/v1/spaces/features (space_personal_agents_api &&
// space_personal_agents_ui). Presentation only: Control decides on every call. The action contracts
// are shared/actions/domains/spaces/ws17.ts.
import { hasErrorCode, type ApiError } from '../api-error'
import { requestJson } from '../http'

/** The closed Control reasons an add can be refused for (WS17 D3). Each has its sentence in errors.ts. */
export const AGENT_ADD_REFUSAL_REASONS = [
  'add_policy_nobody',
  'add_policy_owner_only',
  'agent_not_visible',
  'agent_suspended',
  'owner_not_in_room',
  'personal_agents_not_entitled',
  'personal_space_not_supported',
  'room_role_insufficient',
  'space_not_active',
] as const

export type AgentAddRefusalReason = (typeof AGENT_ADD_REFUSAL_REASONS)[number]

export function isAgentAddRefusalReason(value: unknown): value is AgentAddRefusalReason {
  return typeof value === 'string' && (AGENT_ADD_REFUSAL_REASONS as readonly string[]).includes(value)
}

export type PersonalAgentVisibility = 'owner' | 'org'
export type PersonalAgentAddPolicy = 'owner_only' | 'nobody' | 'anyone'
export type PersonalAgentAccessRole = 'owner' | 'manager' | 'collaborator'
export type PersonalAgentEntryRole = 'manager' | 'collaborator'
export type PersonalAgentRoomState = 'pending' | 'active' | 'paused'

export type PersonalAgentRoom = {
  readonly spaceRef: string
  readonly state: PersonalAgentRoomState
}

/** One of the caller's own agents, from GET /api/v1/me/agents. */
export type PersonalAgent = {
  readonly agentRef: string
  readonly subjectId: string
  readonly name: string
  readonly status: 'active' | 'draft'
  readonly role: PersonalAgentAccessRole
  readonly visibility: PersonalAgentVisibility
  readonly addPolicy: PersonalAgentAddPolicy
  readonly revision: number
  readonly rooms: readonly PersonalAgentRoom[]
}

export type PersonalAgentList = {
  readonly agents: readonly PersonalAgent[]
  readonly primarySubjectId: string | null
}

export type PersonalAgentEntry = {
  readonly principalId: string
  readonly role: PersonalAgentEntryRole
}

/** The access of one agent. Principal labels are not sent, so the id is the only name for a person. */
export type PersonalAgentAccess = {
  readonly agentRef: string
  readonly subjectId: string
  readonly role: PersonalAgentAccessRole
  readonly visibility: PersonalAgentVisibility
  readonly addPolicy: PersonalAgentAddPolicy
  readonly revision: number
  /** Null when no owner is named (a save reply does not carry it; the read does). */
  readonly ownerSubjectId: string | null
  readonly entries: readonly PersonalAgentEntry[]
}

export type PersonalAgentAccessSave = {
  readonly visibility: PersonalAgentVisibility
  readonly addPolicy: PersonalAgentAddPolicy
  /** The whole list the person sees. The server replaces the stored list with exactly this. */
  readonly entries: readonly PersonalAgentEntry[]
  /** The revision the person read. A different stored revision refuses the save. */
  readonly expectedRevision: number
}

export type PersonalAgentCreate = {
  readonly name: string
  readonly instructions?: string
  readonly avatarColor?: string
  /** One id per attempt: a retry with the same id and content gets the same agent back. */
  readonly clientRequestId: string
}

export type PersonalAgentCreated = {
  readonly agentRef: string
  readonly subjectId: string
  readonly revision: number
}

export type SpaceAgentWithdrawal = {
  readonly agentRef: string
  readonly spaceRef: string
  readonly changed: boolean
}

// A reply that does not match the contract fails closed: the caller shows its error and writes nothing
// based on a value it could not read.
const INVALID_RESPONSE = 'invalid_personal_agent_response'

function invalid(): never {
  throw new Error(INVALID_RESPONSE)
}

function recordOf(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : invalid()
}

function textOf(value: unknown): string {
  return typeof value === 'string' && value.trim() ? value : invalid()
}

function choice<T extends string>(value: unknown, allowed: readonly T[]): T {
  return typeof value === 'string' && (allowed as readonly string[]).includes(value) ? (value as T) : invalid()
}

function revisionOf(value: unknown): number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 ? value : invalid()
}

function listOf<T>(value: unknown, read: (item: unknown) => T): T[] {
  return Array.isArray(value) ? value.map((item) => read(item)) : invalid()
}

const VISIBILITY = ['owner', 'org'] as const
const ADD_POLICY = ['owner_only', 'nobody', 'anyone'] as const
const ACCESS_ROLE = ['owner', 'manager', 'collaborator'] as const
const ENTRY_ROLE = ['manager', 'collaborator'] as const
const ROOM_STATE = ['pending', 'active', 'paused'] as const

function roomOf(item: unknown): PersonalAgentRoom {
  const row = recordOf(item)
  return { spaceRef: textOf(row.space_ref), state: choice(row.state, ROOM_STATE) }
}

function agentOf(item: unknown): PersonalAgent {
  const row = recordOf(item)
  return {
    agentRef: textOf(row.agent_ref),
    subjectId: textOf(row.subject_id),
    name: typeof row.name === 'string' ? row.name : '',
    status: choice(row.status, ['active', 'draft'] as const),
    role: choice(row.role, ACCESS_ROLE),
    visibility: choice(row.visibility, VISIBILITY),
    addPolicy: choice(row.add_policy, ADD_POLICY),
    revision: revisionOf(row.revision),
    rooms: listOf(row.rooms, roomOf),
  }
}

function entryOf(item: unknown): PersonalAgentEntry {
  const row = recordOf(item)
  return { principalId: textOf(row.principal_id), role: choice(row.role, ENTRY_ROLE) }
}

function accessOf(value: unknown): PersonalAgentAccess {
  const row = recordOf(value)
  return {
    agentRef: textOf(row.agent_ref),
    subjectId: textOf(row.subject_id),
    role: choice(row.role, ACCESS_ROLE),
    visibility: choice(row.visibility, VISIBILITY),
    addPolicy: choice(row.add_policy, ADD_POLICY),
    revision: revisionOf(row.revision),
    ownerSubjectId: typeof row.owner_subject_id === 'string' && row.owner_subject_id ? row.owner_subject_id : null,
    entries: listOf(row.entries, entryOf),
  }
}

/** `features.personal_agents` from GET /api/v1/spaces/features: true only when the gateway says so. */
export async function getPersonalAgentsFeature(): Promise<boolean> {
  try {
    const response = await requestJson<{ features?: unknown }>('/api/v1/spaces/features')
    const features = response && typeof response === 'object' ? response.features : undefined
    return !!features && typeof features === 'object' && (features as Record<string, unknown>).personal_agents === true
  } catch {
    return false
  }
}

/** Create one personal agent. The caller keeps the same `clientRequestId` for a retry of the same attempt. */
export async function createPersonalAgent(input: PersonalAgentCreate): Promise<PersonalAgentCreated> {
  const wire = recordOf(
    await requestJson<unknown>('/api/v1/me/agents', {
      method: 'POST',
      body: JSON.stringify({
        name: input.name.trim(),
        ...(input.instructions?.trim() ? { instructions: input.instructions.trim() } : {}),
        ...(input.avatarColor?.trim() ? { avatar_color: input.avatarColor.trim() } : {}),
        client_request_id: input.clientRequestId,
      }),
    }),
  )
  return {
    agentRef: textOf(wire.agent_ref),
    subjectId: textOf(wire.subject_id),
    revision: revisionOf(wire.revision),
  }
}

/** The caller's own agents and which one is primary. */
export async function listPersonalAgents(): Promise<PersonalAgentList> {
  const wire = recordOf(await requestJson<unknown>('/api/v1/me/agents'))
  return {
    agents: listOf(wire.agents, agentOf),
    primarySubjectId: wire.primary_subject_id === null ? null : textOf(wire.primary_subject_id),
  }
}

/** The access of one agent, with the revision a later save must name. */
export async function getPersonalAgentAccess(agentRef: string): Promise<PersonalAgentAccess> {
  return accessOf(await requestJson<unknown>(`/api/v1/agents/${encodeURIComponent(agentRef)}/access`))
}

/**
 * Save the access of one agent: visibility, add policy and the whole list, against the revision the
 * person read. Refused as stale with 409 `agent_state_changed`; the caller reloads and never resends.
 */
export async function updatePersonalAgentAccess(
  agentRef: string,
  save: PersonalAgentAccessSave,
): Promise<PersonalAgentAccess> {
  return accessOf(
    await requestJson<unknown>(`/api/v1/agents/${encodeURIComponent(agentRef)}/access`, {
      method: 'PUT',
      body: JSON.stringify({
        visibility: save.visibility,
        add_policy: save.addPolicy,
        entries: save.entries.map((entry) => ({ principal_id: entry.principalId, role: entry.role })),
        expected_revision: save.expectedRevision,
      }),
    }),
  )
}

/** Choose the primary agent, or clear it with `null`. Returns the primary subject id the server stored. */
export async function setPrimaryPersonalAgent(agentRef: string | null): Promise<string | null> {
  const wire = recordOf(
    await requestJson<unknown>('/api/v1/me/primary-agent', {
      method: 'PUT',
      body: JSON.stringify({ agent_ref: agentRef }),
    }),
  )
  return wire.primary_subject_id === null ? null : textOf(wire.primary_subject_id)
}

/** Take one agent out of one room (its owner or a room manager). The agent is kept and can be added again. */
export async function withdrawSpaceAgent(spaceRef: string, agentRef: string): Promise<SpaceAgentWithdrawal> {
  const wire = recordOf(
    await requestJson<unknown>(
      `/api/v1/agents/${encodeURIComponent(agentRef)}/rooms/${encodeURIComponent(spaceRef)}`,
      { method: 'DELETE' },
    ),
  )
  return { agentRef, spaceRef, changed: wire.changed === true }
}

/** The reason a bind was refused (403 `agent_not_addable`, `details.reason`), when it is one this build knows. */
export function agentNotAddableReason(err: unknown): AgentAddRefusalReason | null {
  if (!hasErrorCode(err, 'agent_not_addable')) return null
  const details = (err as ApiError).details
  const reason = details && !Array.isArray(details) ? details.reason : undefined
  return isAgentAddRefusalReason(reason) ? reason : null
}
