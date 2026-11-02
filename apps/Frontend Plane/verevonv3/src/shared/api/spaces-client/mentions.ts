// WS08-S6: structured mentions on a human post, and the wake that follows.
//
// A mention is an id, never text the server has to parse: the composer sends the Control
// `subject_id` of each person and agent it picked, and the gateway checks every id against the
// room's roster and bindings. The post never fails because of a mention; a ref the server could
// not honour comes back under `rejected` with a code, and an agent it did record comes back as
// `wake: 'expected'` (the poster's browser then wakes it) or `wake: 'refused'` with the reason.
//
// Wire (plan WS08, API paragraph):
//   request  mentions: { people: [{ subject_id, label, source? }], agents: [{ subject_id, source? }] }
//   response data.mentions: { recorded: [{ subject_id, kind, wake?, reason?, ordinal? }],
//                             rejected: [{ subject_id, code }] }
// `source` is `typed` (the @name is in the text) or `locked` (the address bar held the agent).
// Nothing here authorizes anything: the gateway, Control and Session Core decide every wake.

/** Caps the gateway enforces (`invalid_mentions` above them); the composer never offers more. */
export const SPACE_MENTION_MAX_PEOPLE = 20
export const SPACE_MENTION_MAX_AGENTS = 3

export type SpaceMentionSource = 'typed' | 'locked'

export type SpaceMentionsInput = {
  readonly people: readonly { readonly subjectId: string; readonly label: string; readonly source?: SpaceMentionSource }[]
  readonly agents: readonly { readonly subjectId: string; readonly source?: SpaceMentionSource }[]
}

/** Why the server recorded an agent mention without waking it (the migration's closed set). */
export type SpaceMentionRefusalReason =
  | 'respond_to_denied'
  | 'mention_not_enabled'
  | 'invocation_blocked'
  | 'owner_only_thread'

export type SpaceMentionRecorded = {
  readonly subjectId: string
  readonly kind: 'user' | 'agent'
  /** Agents only. `expected`: this browser should wake it. `refused`: recorded, no run. */
  readonly wake?: 'expected' | 'refused'
  readonly reason?: string
  readonly ordinal?: number
}

export type SpaceMentionRejected = {
  readonly subjectId: string
  /** `person_not_in_room`, `agent_not_bound`, `mention_text_missing`, or a code this client does not know. */
  readonly code: string
}

export type SpaceMentionsReceipt = {
  readonly recorded: readonly SpaceMentionRecorded[]
  readonly rejected: readonly SpaceMentionRejected[]
}

/** The request body fragment; `undefined` when there is nothing to say, so a plain post is byte-identical to before. */
export function mentionsWire(input: SpaceMentionsInput | undefined): Record<string, unknown> | undefined {
  if (!input || (input.people.length === 0 && input.agents.length === 0)) return undefined
  return {
    people: input.people.map((person) => ({
      subject_id: person.subjectId,
      label: person.label,
      ...(person.source === 'locked' ? { source: 'locked' } : {}),
    })),
    agents: input.agents.map((agent) => ({
      subject_id: agent.subjectId,
      ...(agent.source === 'locked' ? { source: 'locked' } : {}),
    })),
  }
}

/** Same mentions, same wire: used to tell "the same send" from a different body under one key. */
export function sameMentions(a: SpaceMentionsInput | undefined, b: SpaceMentionsInput | undefined): boolean {
  return JSON.stringify(mentionsWire(a) ?? null) === JSON.stringify(mentionsWire(b) ?? null)
}

/** Reads `data.mentions`; anything that is not the documented shape reads as "no mention data". */
export function parseMentionsReceipt(raw: unknown): SpaceMentionsReceipt | undefined {
  if (!raw || typeof raw !== 'object') return undefined
  const object = raw as { recorded?: unknown; rejected?: unknown }
  const recorded: SpaceMentionRecorded[] = []
  for (const item of Array.isArray(object.recorded) ? object.recorded : []) {
    if (!item || typeof item !== 'object') continue
    const entry = item as Record<string, unknown>
    const subjectId = typeof entry.subject_id === 'string' ? entry.subject_id : ''
    const kind = entry.kind === 'agent' ? 'agent' : entry.kind === 'user' ? 'user' : undefined
    if (!subjectId || !kind) continue
    recorded.push({
      subjectId,
      kind,
      ...(entry.wake === 'expected' || entry.wake === 'refused' ? { wake: entry.wake } : {}),
      ...(typeof entry.reason === 'string' && entry.reason ? { reason: entry.reason } : {}),
      ...(typeof entry.ordinal === 'number' && Number.isFinite(entry.ordinal) ? { ordinal: entry.ordinal } : {}),
    })
  }
  const rejected: SpaceMentionRejected[] = []
  for (const item of Array.isArray(object.rejected) ? object.rejected : []) {
    if (!item || typeof item !== 'object') continue
    const entry = item as Record<string, unknown>
    if (typeof entry.subject_id === 'string' && typeof entry.code === 'string') {
      rejected.push({ subjectId: entry.subject_id, code: entry.code })
    }
  }
  return { recorded, rejected }
}

/** What a wake needs, from the chat client's side: the stored post and this wake's own retry identity. */
export type SpaceWakeRef = {
  readonly messageId: string
  /** `^[A-Za-z0-9_-]{16,64}$`; one id per (post, agent), reused by every retry. */
  readonly clientWakeId: string
}
