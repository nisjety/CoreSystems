import {
  SPACE_MENTION_MAX_AGENTS,
  SPACE_MENTION_MAX_PEOPLE,
  type SpaceMentionSource,
  type SpaceMentionsInput,
} from '@/shared/api/spaces-client'

/**
 * Structured mentions in the room composer (WS08-S6): the pure part. A pick is an id plus the
 * label that was put in the text; the payload sent with a post is built from the picks that are
 * still in the words (typed) and from the agents the address bar holds (locked). Presentation
 * only: the gateway re-checks every id against the roster and the bindings.
 */

export { SPACE_MENTION_MAX_AGENTS as MAX_ADDRESSED_AGENTS, SPACE_MENTION_MAX_PEOPLE as MAX_MENTIONED_PEOPLE }

export type MentionCandidateKind = 'person' | 'agent'

/** One row the picker can offer for a room member. `label` is what goes into the text after `@`. */
export interface MentionCandidate {
  readonly id: string
  readonly kind: MentionCandidateKind
  readonly subjectId: string
  /** The plain display name, before namesake qualification. */
  readonly name: string
  /** The text inserted after `@`: the name, or the name plus a qualifier when two share it. */
  readonly label: string
  /** The member's room role (people) or `agent`; used to tell namesakes apart. */
  readonly role?: string
  readonly color?: string
}

export interface MentionedPerson {
  readonly subjectId: string
  readonly label: string
}

export interface AddressedAgent {
  readonly subjectId: string
  readonly label: string
  /** `typed`: the `@name` is in the words. `locked`: the address bar keeps it for every post. */
  readonly source: SpaceMentionSource
  readonly color?: string
}

const WORD_CHAR = /[\p{L}\p{N}_]/u

/** NFC and case-folded, the way the gateway compares a label against the text. */
export function foldLabel(value: string): string {
  return value.normalize('NFC').toLocaleLowerCase()
}

/**
 * Whether `@label` is in the text as a mention: the label follows an `@` that starts the text or
 * follows whitespace, and a letter or digit does not continue it (so `@Kari` is not found inside
 * `@Karin`).
 */
export function mentionPresent(text: string, label: string): boolean {
  const haystack = foldLabel(text)
  const needle = `@${foldLabel(label)}`
  if (needle.length <= 1) return false
  let from = 0
  for (;;) {
    const at = haystack.indexOf(needle, from)
    if (at < 0) return false
    const before = at === 0 ? '' : (haystack[at - 1] ?? '')
    const after = haystack.slice(at + needle.length, at + needle.length + 1)
    const startsOk = before === '' || /\s/u.test(before)
    const endsOk = after === '' || !WORD_CHAR.test(after)
    if (startsOk && endsOk) return true
    from = at + 1
  }
}

type RawCandidate = Omit<MentionCandidate, 'label' | 'id'>

/**
 * Give every candidate the label it is inserted under. A name nobody else shares stays as it is.
 * Namesakes (same name, case- and diacritic-insensitively, across people and agents) get the role in
 * brackets, and when the role does not separate them either, the last four characters of the
 * subject id. The qualified label is what lands in the text, so the text itself says who was meant.
 */
export function qualifyCandidates(raw: readonly RawCandidate[]): readonly MentionCandidate[] {
  const keyOf = (value: string) => foldLabel(value).normalize('NFD').replace(/\p{M}+/gu, '').trim()
  const byName = new Map<string, RawCandidate[]>()
  for (const candidate of raw) {
    const key = keyOf(candidate.name)
    byName.set(key, [...(byName.get(key) ?? []), candidate])
  }
  const roleOf = (candidate: RawCandidate) => candidate.role ?? (candidate.kind === 'agent' ? 'agent' : 'member')
  const labels = new Map<RawCandidate, string>()
  for (const group of byName.values()) {
    if (group.length === 1) {
      const only = group[0]
      if (only) labels.set(only, only.name)
      continue
    }
    const byRole = new Map<string, number>()
    for (const candidate of group) byRole.set(roleOf(candidate), (byRole.get(roleOf(candidate)) ?? 0) + 1)
    for (const candidate of group) {
      const role = roleOf(candidate)
      const tail = candidate.subjectId.slice(-4)
      labels.set(
        candidate,
        (byRole.get(role) ?? 0) > 1 ? `${candidate.name} (${role} ${tail})` : `${candidate.name} (${role})`,
      )
    }
  }
  return raw.map((candidate) => ({
    ...candidate,
    id: `${candidate.kind === 'agent' ? 'agent' : 'user'}:${candidate.subjectId}`,
    label: labels.get(candidate) ?? candidate.name,
  }))
}

export interface MentionPayloadInput {
  readonly text: string
  readonly people: readonly MentionedPerson[]
  readonly agents: readonly AddressedAgent[]
}

/**
 * The mentions to send with a post, or `undefined` for none. A person counts only while their
 * `@label` is still in the words; a typed agent likewise; a locked agent always. Duplicates by id
 * collapse, and the caps the gateway enforces are applied here so the box never promises more.
 */
export function buildMentionsPayload(input: MentionPayloadInput): SpaceMentionsInput | undefined {
  const seenPeople = new Set<string>()
  const people = input.people
    .filter((person) => mentionPresent(input.text, person.label))
    .filter((person) => (seenPeople.has(person.subjectId) ? false : (seenPeople.add(person.subjectId), true)))
    .slice(0, SPACE_MENTION_MAX_PEOPLE)
    .map((person) => ({ subjectId: person.subjectId, label: person.label }))
  const seenAgents = new Set<string>()
  const agents = input.agents
    .filter((agent) => agent.source === 'locked' || mentionPresent(input.text, agent.label))
    .filter((agent) => (seenAgents.has(agent.subjectId) ? false : (seenAgents.add(agent.subjectId), true)))
    .slice(0, SPACE_MENTION_MAX_AGENTS)
    .map((agent) => ({ subjectId: agent.subjectId, ...(agent.source === 'locked' ? { source: 'locked' as const } : {}) }))
  if (people.length === 0 && agents.length === 0) return undefined
  return { people, agents }
}

/** Drops the typed picks whose `@label` left the words; locked agents stay. */
export function pruneToText<T extends { readonly label: string }>(
  text: string,
  picks: readonly T[],
  keep: (pick: T) => boolean = () => false,
): readonly T[] {
  const next = picks.filter((pick) => keep(pick) || mentionPresent(text, pick.label))
  return next.length === picks.length ? picks : next
}

/** The word list shown for a wake that could not start, keyed by the server's refusal reason. */
export const REFUSAL_REASON_CODE: Readonly<Record<string, string>> = {
  respond_to_denied: 'agent_respond_to_denied',
  mention_not_enabled: 'agent_mention_not_enabled',
  invocation_blocked: 'agent_invocation_blocked',
  owner_only_thread: 'wake_owner_required',
}
