import type { ChatArtifact } from '@/features/chat/components/chat-types'
import { roomArtifactsFromMetadata } from '@/features/spaces/lib/room-artifacts'
import {
  spaceRetrievalDeclineReason,
  type SpaceRetrievalDeclineReason,
} from '@/features/spaces/lib/space-retrieval-decline'
import { ApiError, requestJson } from '../http'
import { setDmListingEnabled, type SpaceDmParticipant } from './dm'
import { spacePinsFrom, type SpaceRoomPin } from './pins'
import { spaceRunWorkFrom, type SpaceRunWork } from './run-work'
import { readSpaceDenial } from '../space-denials'

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
  /** The room's one-line topic (WS04-S2). Absent or null when none is set. A label, not authority. */
  topic?: string | null
  /** True when Control's index says the room is archived (WS04-S7). `lifecycle` stays `active`
   * for an archived room, so read this instead. Absent on older gateways and the fallback listing. */
  archived?: boolean
  /** WS12-S3: Control's derived agent scope for the kind (`personal`, `room` or `none`). Absent on older gateways. */
  agent_scope?: string
  /** WS12-S3: a DM's fixed roster with display names; present for `dm` and `group_dm` only. */
  participants?: readonly SpaceDmParticipant[]
  /** WS12-S3: the caller has hidden this DM from their sidebar. Absent or false otherwise. */
  hidden?: boolean
}

/** A server-composed lifecycle + current-membership read. It contains no
 * signed decision, recipient audience, or owner-resource authority. */
export type SpaceContext = {
  space: SpaceSummary
  membership: SpaceMembership
  /**
   * Server rollout flags (WS01-S5) the UI reads to show or hide a control. Absent on an older
   * gateway, which means "off". Presentation only, never an authorization input.
   */
  features?: {
    human_posts?: boolean
    thread_pane?: boolean
    send_idempotency?: boolean
    connectivity_banner?: boolean
    /** WS04-S2: the owner or manager may edit the room's name and topic. */
    room_details?: boolean
    /** WS04-S3: the member picker searches the people directory instead of listing the org. */
    people_search?: boolean
    /** WS12-S3: direct messages are offered. */
    dm?: boolean
    /** WS10-S5b: the Agent tab shows the per-tool policy editor. */
    tool_policy_editor?: boolean
    /** WS14: the room file library's rollout, `{turn_attachments, durable, files_tab}`. */
    room_files?: { turn_attachments?: boolean; durable?: boolean; files_tab?: boolean }
  }
  /** Server-stated bounds (WS01-S5). Absent on an older gateway: use the documented defaults. */
  limits?: {
    post_max_chars?: number
    /** WS14: `{max_files, max_file_bytes, max_request_bytes, allowed_extensions, turn_inline_chars, turn_total_chars}`. */
    room_files?: unknown
  }
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
  /**
   * Pinned in the room. Server-owned and relayed from Session Core's listing,
   * which already orders pinned posts first. Absent on an older gateway.
   */
  pinned?: boolean
  /** Replies after the first message (WS01-S1). Absent until Session Core returns it. */
  reply_count?: number
  /** Who wrote the latest message (WS01-S1). Absent means unknown, never "you". */
  last_author_subject_id?: string
  /** Readable turns in the post (WS15-S5 summary, via the timeline window). Absent on the legacy listing. */
  turn_count?: number
  /** Session Core's tail sequence of the post, the keyset the timeline pages by (WS15-S5). */
  last_message_seq?: number
  /** Up to five newest distinct authors of the post (WS15-S5). Ids only. */
  recent_author_subject_ids?: readonly string[]
}

/**
 * Where the reader last caught up with this room (item 4b). `last_read_at` is
 * epoch milliseconds, `null` when they have never had the room open — which
 * is "nothing is new yet", not "everything is new". An absent marker means it
 * could not be read; the listing names that as a gap.
 */
export type SpaceReadMarker = {
  readonly last_read_at: number | null
  /**
   * WS11-S4: the reader moved the marker back by hand ("mark unread"), so the room is unread from
   * `last_read_at` until they come back in (`reason: 'enter'`). Absent from an older gateway.
   */
  readonly manual_unread?: boolean
}

export type SpaceThreads = {
  space: SpaceSummary
  membership: SpaceMembership
  threads: readonly SpaceThread[]
  read_marker?: SpaceReadMarker
  /** The room's pins (WS13-S8), newest first. Empty when none, when the flag is off or when the section was unavailable. */
  pins?: readonly SpaceRoomPin[]
  /**
   * Whether this caller may read the other members' conversations in this room
   * (Control's shared-read decision, relayed by the BFF). `getSpaceThreads`
   * always fills it: an older gateway that does not say defaults to
   * `{ available: true }` so nothing changes there. `available: false` is an
   * answer, not an error.
   */
  shared_read?: SpaceSharedRead
  /**
   * What the room's agent runs offer this caller (WS09). Absent from an older gateway, which
   * means every field is off. A rollout answer, never an authorization: the owner plane still
   * decides each steer and stop.
   */
  agent_work?: SpaceAgentWork
  unavailable?: readonly SpaceWorkGap[]
}

export type SpaceSharedRead = { readonly available: boolean }

/**
 * `data.agent_work` on the thread listing. `steer_owner`: the person who started a run may type
 * into it while it works (`space_run_owner_steer`; false in a Zero Data Retention org, where the
 * queue route refuses). `live`, `text` and `stop_manager` are the room live view, its streamed
 * text and the manager Stop (later WS09 slices); each is true only when that surface exists.
 */
export type SpaceAgentWork = {
  readonly live: boolean
  readonly text: boolean
  readonly steer_owner: boolean
  readonly stop_manager: boolean
}

/** Only an explicit `true` turns a field on; anything else, or a missing object, is off. */
export function spaceAgentWorkFrom(raw: unknown): SpaceAgentWork | undefined {
  if (!raw || typeof raw !== 'object') return undefined
  const value = raw as Record<string, unknown>
  return {
    live: value.live === true,
    text: value.text === true,
    steer_owner: value.steer_owner === true,
    stop_manager: value.stop_manager === true,
  }
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
  const response = await requestJson<{ spaces: readonly SpaceSummary[]; features?: { dm?: boolean } }>('/api/v1/spaces')
  setDmListingEnabled(response.features?.dm === true)
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

/** The body of the thread listing and of the timeline's first page (`data` of both routes). */
export type SpaceThreadsBody = {
  space: SpaceSummary
  membership: SpaceMembership
  threads: readonly SpaceThread[]
  read_marker?: unknown
  pins?: unknown
  unavailable?: unknown
  shared_read?: unknown
  agent_work?: unknown
}

export async function getSpaceThreads(spaceRef: string): Promise<SpaceThreads> {
  const response = await requestJson<SpaceThreadsBody>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/threads`)
  return spaceThreadsFromBody(response)
}

/** Reads the closed shape of a listing body; shared by the legacy listing and the timeline's first page. */
export function spaceThreadsFromBody(response: SpaceThreadsBody): SpaceThreads {
  // Only an explicit `available: false` restricts; absent or malformed keeps
  // today's behaviour (the transcript route still decides per read).
  const sharedRead = response.shared_read
  const shared_read: SpaceSharedRead = {
    available:
      !(sharedRead && typeof sharedRead === 'object' && (sharedRead as { available?: unknown }).available === false),
  }
  const marker = response.read_marker
  const lastReadAt =
    marker && typeof marker === 'object' ? (marker as { last_read_at?: unknown }).last_read_at : undefined
  // Only a real object counts as "the marker was read". `null` inside it is
  // the member who has never caught up; a missing object is the gap.
  const read_marker: SpaceReadMarker | undefined =
    marker && typeof marker === 'object'
      ? {
          last_read_at: typeof lastReadAt === 'number' && Number.isFinite(lastReadAt) ? lastReadAt : null,
          ...((marker as { manual_unread?: unknown }).manual_unread === true ? { manual_unread: true } : {}),
        }
      : undefined
  const agent_work = spaceAgentWorkFrom(response.agent_work)
  return {
    space: response.space,
    membership: response.membership,
    threads: response.threads,
    ...(read_marker ? { read_marker } : {}),
    pins: spacePinsFrom(response.pins),
    shared_read,
    ...(agent_work ? { agent_work } : {}),
    unavailable: workGaps(response.unavailable),
  }
}

/**
 * Retitle or pin a post in the room.
 *
 * Owner-bound in Session Core: only the member who started the post may. The
 * room route relays a refusal as `thread_presentation_owner_only`, and the UI
 * offers the control only to the author — so anyone else reaches this only by
 * racing a membership change, and it fails honestly then.
 */
export async function updateSpaceThreadPresentation(
  spaceRef: string,
  threadId: string,
  presentation: { readonly title?: string; readonly pinned?: boolean },
): Promise<{ thread_id: string; title?: string; pinned?: boolean }> {
  return requestJson(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/threads/${encodeURIComponent(threadId)}/presentation`,
    { method: 'PATCH', body: JSON.stringify(presentation) },
  )
}

/**
 * The reader has this room open now. Advances their read marker.
 *
 * `reason` (WS11-S4): `'enter'` is the first call of a visit and clears a manual "mark unread"
 * floor; `'while_open'` (the default, and what an older gateway assumes) is the page's routine poll
 * and leaves a floor alone, so marking something unread while the room is open sticks.
 */
export async function markSpaceRead(
  spaceRef: string,
  reason?: 'enter' | 'while_open',
): Promise<SpaceReadMarker> {
  const response = await requestJson<{ last_read_at?: unknown; manual_unread?: unknown }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/read`,
    { method: 'POST', body: JSON.stringify(reason ? { reason } : {}) },
  )
  const at = response.last_read_at
  return {
    last_read_at: typeof at === 'number' && Number.isFinite(at) ? at : null,
    ...(response.manual_unread === true ? { manual_unread: true } : {}),
  }
}

/** Someone else in the room right now. Identifiers and a status, never content. */
export type SpacePresentMember = {
  readonly subject_id: string
  readonly status: 'online' | 'typing'
  readonly last_seen_at: number
}

export type SpacePresence = {
  readonly present: readonly SpacePresentMember[]
  /** How long a heartbeat counts as present, per Application Plane. */
  readonly ttl_seconds: number | null
}

/**
 * "I am here" — and, in the same answer, "who else is?".
 *
 * One request per beat rather than a write plus a read, on a timer that
 * already runs. `offline` is the polite goodbye a room can send when the
 * reader leaves; it is never required, because a heartbeat that stops coming
 * expires on its own.
 */
export async function recordSpacePresence(
  spaceRef: string,
  status: 'online' | 'typing' | 'offline' = 'online',
): Promise<SpacePresence> {
  const response = await requestJson<{ present?: unknown; ttl_seconds?: unknown }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/presence`,
    { method: 'POST', body: JSON.stringify({ status }) },
  )
  const present = Array.isArray(response.present) ? response.present : []
  return {
    present: present.flatMap((row) => {
      if (!row || typeof row !== 'object') return []
      const member = row as Record<string, unknown>
      const subjectId = typeof member.subject_id === 'string' ? member.subject_id.trim() : ''
      if (!subjectId) return []
      return [{
        subject_id: subjectId,
        status: member.status === 'typing' ? ('typing' as const) : ('online' as const),
        last_seen_at: typeof member.last_seen_at === 'number' ? member.last_seen_at : 0,
      }]
    }),
    ttl_seconds: typeof response.ttl_seconds === 'number' ? response.ttl_seconds : null,
  }
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
  /**
   * Why Control declined, when it said (`space_denials_typed`): `reason`,
   * `resolvable_by` and the allowlisted keys. Read through `space-denials.ts`.
   */
  readonly details?: Readonly<Record<string, unknown>>
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
  /**
   * Background processes started in this Space (S4.2). Read-only here: the
   * room can see what is running, but signalling or writing to a process is a
   * separate authority question this slice does not answer.
   *
   * Absent from a gateway that predates S4.2, which `workRows` turns into the
   * empty list rather than `undefined` — a Work tab that crashes on an older
   * gateway is worse than one that shows no processes.
   */
  readonly processes: readonly Record<string, unknown>[]
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
    const details = record.details && typeof record.details === 'object' && !Array.isArray(record.details)
      ? (record.details as Readonly<Record<string, unknown>>)
      : undefined
    if (section && reason) gaps.push({ section, reason, ...(code ? { code } : {}), ...(details ? { details } : {}) })
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
    processes?: unknown
    unavailable?: unknown
  }>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/work`)
  return {
    space: response.space,
    membership: response.membership,
    runs: workRows(response.runs),
    schedules: workRows(response.schedules),
    processes: workRows(response.processes),
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

/**
 * The roster plus whether the gateway's Control member writer is on (WS04-S6). `writerApi` is
 * true only when the server said so; absence (an older gateway, the flag off) is false, so the
 * role and leave controls stay hidden rather than offering calls the server refuses.
 */
export async function getSpaceRosterView(
  spaceRef: string,
): Promise<{ members: readonly SpaceRosterMember[]; writerApi: boolean }> {
  const response = await requestJson<{ members: readonly SpaceRosterMember[]; writer_api?: boolean }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/roster`,
  )
  return { members: response.members, writerApi: response.writer_api === true }
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
  /** The server's id for this message, when it sent one. */
  readonly messageId?: string
  /**
   * When the server stored this turn, as an ISO-8601 UTC string (`2026-10-31T09:30:15.250Z`).
   * Absent when the server sent none or sent something that is not a time; a row must then
   * render without a time rather than with an invented one.
   */
  readonly createdAt?: string
  /**
   * Artifacts the turn persisted, already through the room's kind and size
   * gate (`roomArtifactsFromMetadata`). Only ever set on assistant turns.
   */
  readonly artifacts?: readonly ChatArtifact[]
  /**
   * Why this assistant turn answered without the organisation's knowledge, when Control declined
   * retrieval for the Space (D1-242): the code Model Plane stored on the answer, read back through the
   * closed set. Absent on every other turn and on a code outside the set.
   */
  readonly retrievalDeclined?: SpaceRetrievalDeclineReason
  /**
   * The content-free work summary of an assistant turn (WS09-S3 `metadata.work`): which kinds of step the
   * agent took and how many messages its owner steered in. Read through the closed shape
   * (`spaceRunWorkFrom`); absent on every other turn and when the summary was not stored.
   */
  readonly work?: SpaceRunWork
}

export type SpaceThreadTranscript = {
  readonly threadId: string
  readonly turns: readonly SpaceThreadTurn[]
  /** Present only when a tail window was asked for and the gateway honoured it (WS15-S7 R2). */
  readonly window?: SpaceTranscriptWindowInfo
}

/** What a tail read left out: `omittedBefore` turns sit between the first turn and the window. */
export type SpaceTranscriptWindowInfo = {
  readonly omittedBefore: number
  readonly hasMoreBefore: boolean
}

/** `?tail=&before=&include_first=` of the transcript route (WS15-S7 R2). */
export type SpaceTranscriptWindowRequest = {
  readonly tail: number
  /** A message id: the window is the turns before it. */
  readonly before?: string
  /** Put the post's first turn ahead of the tail. Ignored with `before`. */
  readonly includeFirst?: boolean
}

/** A server time as a normalized ISO string, or undefined when it is missing or not a time. */
function normalizeTurnTime(value: unknown): string | undefined {
  if (typeof value !== 'string' || !value.trim()) return undefined
  const ms = Date.parse(value.trim())
  return Number.isFinite(ms) ? new Date(ms).toISOString() : undefined
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
    // An artifact-only assistant turn (the agent replied with the artifact and
    // no prose) is kept: it is the turn the room's chip hangs on.
    const artifacts = role === 'assistant' ? roomArtifactsFromMetadata(record.artifacts) : []
    if (!content && artifacts.length === 0) continue
    const messageId =
      typeof record.message_id === 'string' && record.message_id.trim()
        ? record.message_id.trim()
        : typeof record.messageId === 'string' && record.messageId.trim()
          ? record.messageId.trim()
          : undefined
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
    const createdAt = normalizeTurnTime(record.created_at) ?? normalizeTurnTime(record.createdAt)
    const retrievalDeclined =
      role === 'assistant' && typeof record.retrievalDeclined === 'string'
        ? spaceRetrievalDeclineReason(record.retrievalDeclined)
        : undefined
    const work = role === 'assistant' ? spaceRunWorkFrom(record.work) : undefined
    turns.push({
      role,
      content,
      ...(agentName ? { agentName } : {}),
      ...(authorSubjectId ? { authorSubjectId } : {}),
      ...(messageId ? { messageId } : {}),
      ...(createdAt ? { createdAt } : {}),
      ...(artifacts.length > 0 ? { artifacts } : {}),
      ...(retrievalDeclined ? { retrievalDeclined } : {}),
      ...(work ? { work } : {}),
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
  window?: SpaceTranscriptWindowRequest,
): Promise<SpaceThreadTranscript> {
  const params = new URLSearchParams()
  if (window) {
    params.set('tail', String(window.tail))
    if (window.before) params.set('before', window.before)
    else if (window.includeFirst) params.set('include_first', '1')
  }
  const query = params.toString()
  const response = await requestJson<{
    transcript?: { threadId?: string; turns?: unknown; window?: { omitted_before?: unknown; has_more_before?: unknown } }
  }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/threads/${encodeURIComponent(threadId)}/transcript${query ? `?${query}` : ''}`,
  )
  const raw = response.transcript?.window
  const omitted = raw && typeof raw.omitted_before === 'number' && Number.isFinite(raw.omitted_before) ? raw.omitted_before : 0
  const info: SpaceTranscriptWindowInfo | undefined = window && raw
    ? { omittedBefore: Math.max(0, Math.floor(omitted)), hasMoreBefore: raw.has_more_before === true }
    : undefined
  return {
    threadId: response.transcript?.threadId ?? threadId,
    turns: normalizeSpaceThreadTurns(response.transcript?.turns),
    ...(info ? { window: info } : {}),
  }
}

/**
 * Every way reading one Space thread can end, as a value instead of a thrown
 * error, so a caller cannot forget a case (the room used to swallow the 403
 * together with every other failure).
 *
 * - `restricted`: 403, Control declined the shared read (a real answer).
 * - `missing`: 404, the thread is not (or no longer) there for this caller.
 * - `error`: anything else (network, 5xx); worth a retry, never a silent preview.
 */
export type SpaceTranscriptLoad =
  | {
      readonly state: 'ok'
      readonly threadId: string
      readonly turns: readonly SpaceThreadTurn[]
      readonly window?: SpaceTranscriptWindowInfo
    }
  | { readonly state: 'restricted'; readonly denial?: SpaceTranscriptDenial }
  | { readonly state: 'missing' }
  | { readonly state: 'error' }

/**
 * The typed Space denial a 403 carried (`code` and `details`, as the gateway sent them). Only set
 * when `space-denials.ts` can read a reason from it; read it back through that module, which
 * accepts nothing its vocabulary does not know.
 */
export type SpaceTranscriptDenial = {
  readonly code: string | null
  readonly details: Readonly<Record<string, unknown>> | null
}

export async function loadSpaceTranscript(
  spaceRef: string,
  threadId: string,
  window?: SpaceTranscriptWindowRequest,
): Promise<SpaceTranscriptLoad> {
  try {
    const snapshot = await getSpaceThreadTranscript(spaceRef, threadId, window)
    return {
      state: 'ok',
      threadId: snapshot.threadId,
      turns: snapshot.turns,
      ...(snapshot.window ? { window: snapshot.window } : {}),
    }
  } catch (error) {
    if (error instanceof ApiError && error.status === 403) {
      return readSpaceDenial(error)
        ? { state: 'restricted', denial: { code: error.code, details: error.details } }
        : { state: 'restricted' }
    }
    if (error instanceof ApiError && error.status === 404) return { state: 'missing' }
    return { state: 'error' }
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
  /** WS10-S5b: Application's projection of Control's tool policy for this agent (display only). */
  tool_policy_revision?: number
  tool_policy_digest?: string
  /** Presentation-only `#rrggbb`; the gateway drops anything else. Never an authorization input. */
  avatar_color?: string
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
 * Where the recipient audience stands after a membership write (WS04-S6). `pending`: applied or
 * waiting for acknowledgement, so a new member may need a moment before they can read. `current`:
 * nothing to change. `unconfirmed`: the reconcile call failed; the change stands and a cron
 * heals it. Absent on the legacy path.
 */
export type SpaceAudienceState = 'pending' | 'current' | 'unconfirmed'

function audienceStateOf(audience: { state?: unknown } | undefined): SpaceAudienceState | undefined {
  const state = audience?.state
  return state === 'pending' || state === 'current' || state === 'unconfirmed' ? state : undefined
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
  role?: string,
): Promise<{ memberCount: number; changed: boolean; role?: string; audience?: SpaceAudienceState }> {
  const response = await requestJson<{
    member_count?: number
    changed?: boolean
    role?: string
    audience?: { state?: unknown }
  }>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/members`, {
    method: 'POST',
    // `role` is honoured only with the Control member writer on (WS04-S6); absent means
    // Control's default.
    body: JSON.stringify(role ? { member_id: memberId, role } : { member_id: memberId }),
  })
  return {
    memberCount: response.member_count ?? 0,
    changed: response.changed === true,
    role: response.role,
    audience: audienceStateOf(response.audience),
  }
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
): Promise<{ memberCount: number; changed: boolean; left?: boolean; audience?: SpaceAudienceState }> {
  const response = await requestJson<{
    member_count?: number
    changed?: boolean
    left?: boolean
    audience?: { state?: unknown }
  }>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/members/${encodeURIComponent(memberId)}`, {
    method: 'DELETE',
  })
  return {
    memberCount: response.member_count ?? 0,
    changed: response.changed === true,
    left: response.left,
    audience: audienceStateOf(response.audience),
  }
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
