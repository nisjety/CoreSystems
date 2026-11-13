import { requestJson } from './http'

/**
 * The owner's learning inbox: learned-skill proposals to accept or reject, and the
 * ledger of what Verevon learned and changed, with undo. Fronted by the gateway's
 * `/api/v1/learning/*` and model-gateway's `/v1/learning/*`, which forward to
 * session-core's `LearningGovernanceService`.
 *
 * Every call concerns the signed-in owner's own proposals and ledger. Whose they are
 * is taken from the session on the server, never from anything sent here.
 */

/** Who made a change: the person, Verevon itself, or nobody recorded it. */
export type LearningActorKind = 'user' | 'system' | 'service' | 'unattributed'

/** `skill` is a change to a skill; `memory` is a memory Verevon wrote from a conversation. */
export type LearningEntryKind = 'skill' | 'memory'

export interface LearningLedgerEntry {
  /** A skill revision id, or the memory's id for a memory. */
  entryId: string
  kind: LearningEntryKind
  /**
   * For a skill: created, proposed, updated, accepted, rejected, enabled, disabled,
   * edited, deleted, restored or promoted. For a memory: learned.
   */
  action: string
  skillId: string
  skillName: string
  revision: number
  actorKind: LearningActorKind
  /** The conversation the change came from; empty when there is none. */
  threadId: string
  reason: string
  /** The skill snapshot before the change, as JSON text; empty where it does not exist. */
  beforeJson: string
  /** The skill snapshot after the change, as JSON text; for a memory, `{"content": ...}`. */
  afterJson: string
  /** Whether a later change has already undone this one. */
  undone: boolean
  /** Whether undoing it would be accepted now. A memory is never undone here. */
  undoable: boolean
  createdAt: string | null
}

export interface LearningLedgerPage {
  /** Newest first. */
  entries: LearningLedgerEntry[]
  /** Empty when there is no further page. */
  nextCursor: string
  /** Why memory entries are left out, named as the memory controls name it; empty when they are not. */
  memorySkipReason: string
}

interface LedgerEntryResponse {
  entry_id?: string
  kind?: string
  action?: string
  skill_id?: string
  skill_name?: string
  revision?: number
  actor_kind?: string
  thread_id?: string
  reason?: string
  before_json?: string
  after_json?: string
  undone?: boolean
  undoable?: boolean
  created_at?: string | null
}

interface LedgerPageResponse {
  entries?: LedgerEntryResponse[]
  next_cursor?: string
  memory_skip_reason?: string
}

const ACTOR_KINDS: readonly LearningActorKind[] = ['user', 'system', 'service', 'unattributed']

function normalizeEntry(raw: LedgerEntryResponse): LearningLedgerEntry | null {
  // An entry of a kind this client does not know is dropped, never shown as a skill.
  if (!raw.entry_id || (raw.kind !== 'skill' && raw.kind !== 'memory')) return null
  const actorKind = ACTOR_KINDS.includes(raw.actor_kind as LearningActorKind)
    ? (raw.actor_kind as LearningActorKind)
    // An unrecognised actor is "nobody recorded", never a guess at who made the change.
    : 'unattributed'
  return {
    entryId: raw.entry_id,
    kind: raw.kind,
    action: raw.action ?? '',
    skillId: raw.skill_id ?? '',
    skillName: raw.skill_name ?? '',
    revision: raw.revision ?? 0,
    actorKind,
    threadId: raw.thread_id ?? '',
    reason: raw.reason ?? '',
    beforeJson: raw.before_json ?? '',
    afterJson: raw.after_json ?? '',
    undone: raw.undone ?? false,
    undoable: raw.undoable ?? false,
    createdAt: raw.created_at ?? null,
  }
}

export interface ListLedgerOptions {
  /** The previous page's `nextCursor`. */
  cursor?: string
  limit?: number
}

/** One page of the owner's ledger, newest first. */
export async function listLearningLedger(options: ListLedgerOptions = {}): Promise<LearningLedgerPage> {
  const params = new URLSearchParams()
  if (options.cursor) params.set('cursor', options.cursor)
  if (options.limit) params.set('limit', String(options.limit))
  const query = params.toString()
  const response = await requestJson<LedgerPageResponse>(
    query ? `/api/v1/learning/ledger?${query}` : '/api/v1/learning/ledger',
  )
  return {
    entries: (response.entries ?? [])
      .map(normalizeEntry)
      .filter((entry): entry is LearningLedgerEntry => entry !== null),
    nextCursor: response.next_cursor ?? '',
    memorySkipReason: response.memory_skip_reason ?? '',
  }
}

/** The owner's choice on one learned-skill proposal. */
export type ProposalDecision = 'accept' | 'reject'

export interface ProposalDecisionResult {
  /** The skill's review state after the decision: `accepted` or `rejected`. */
  reviewState: string
  /** Whether the skill is used in chats now. */
  enabled: boolean
  revision: number
}

interface ProposalDecisionResponse {
  review_state?: string
  enabled?: boolean
  revision?: number
}

/** Accepts or rejects one proposal. Only the proposal's owner can decide it. */
export async function decideLearningProposal(
  skillId: string,
  decision: ProposalDecision,
): Promise<ProposalDecisionResult> {
  const response = await requestJson<ProposalDecisionResponse>(
    `/api/v1/learning/proposals/${encodeURIComponent(skillId)}/decision`,
    {
      method: 'POST',
      body: JSON.stringify({ decision }),
    },
  )
  return {
    reviewState: response.review_state ?? '',
    enabled: response.enabled ?? false,
    revision: response.revision ?? 0,
  }
}

export interface UndoLearningResult {
  /** `restored` when the snapshot before the change is back; `deleted` when the change had created the skill. */
  action: string
  skillId: string
  revision: number
}

interface UndoLearningResponse {
  action?: string
  skill_id?: string
  revision?: number
}

/**
 * Undoes the newest change to one of the owner's learned skills. A memory is erased
 * through `deleteMemory` instead; the server refuses to undo it here.
 */
export async function undoLearningEntry(entryId: string): Promise<UndoLearningResult> {
  const response = await requestJson<UndoLearningResponse>(
    `/api/v1/learning/ledger/${encodeURIComponent(entryId)}/undo`,
    { method: 'POST' },
  )
  return {
    action: response.action ?? '',
    skillId: response.skill_id ?? '',
    revision: response.revision ?? 0,
  }
}

/** The skill proposal a taught run filed: its id, and the review state it starts in (`proposed`). */
export interface TaughtSkillProposal {
  skillId: string
  reviewState: string
}

export interface TeachSkillOptions {
  /** The skill's name: lowercase letters and digits joined by single hyphens, at most 64 characters. */
  name: string
  /** An optional hint for the draft, at most 500 characters. */
  goalHint?: string
}

interface TeachSkillResponse {
  skill_id?: string
  review_state?: string
}

/**
 * Turns one run the person drove or took over into a disabled skill proposal in their learning
 * inbox (F-t3-081). Nothing is used until the owner accepts it there. The owner is the signed-in
 * session; a zero-retention turn is refused by the server.
 */
export async function teachSkillFromRun(
  runId: string,
  options: TeachSkillOptions,
): Promise<TaughtSkillProposal> {
  const response = await requestJson<TeachSkillResponse>('/api/v1/learning/teach', {
    method: 'POST',
    body: JSON.stringify({ run_id: runId, name: options.name, goal_hint: options.goalHint ?? '' }),
  })
  return {
    skillId: response.skill_id ?? '',
    reviewState: response.review_state ?? '',
  }
}

/**
 * The `content` of one snapshot, or `null` when there is no snapshot or it cannot be
 * read. A malformed snapshot is shown as absent rather than guessed at.
 */
export function snapshotContent(json: string): string | null {
  if (!json) return null
  try {
    const parsed: unknown = JSON.parse(json)
    if (parsed && typeof parsed === 'object' && 'content' in parsed && typeof parsed.content === 'string') {
      return parsed.content
    }
    return null
  } catch {
    return null
  }
}
