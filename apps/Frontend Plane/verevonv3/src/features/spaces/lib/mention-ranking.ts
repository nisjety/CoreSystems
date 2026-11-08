import type { MentionCandidate } from './mention-model'

/**
 * Ranking for the mention picker (WS08-S6): one list, People then Agents then "Add to room".
 * Prefix match with diacritic folding first (the whole name, then the start of any word in it),
 * then people and agents the member dealt with lately, then alphabetical. Pure and synchronous.
 */

/** Case- and diacritic-insensitive form used only for matching. */
export function foldForMatch(value: string): string {
  return value.normalize('NFD').replace(/\p{M}+/gu, '').toLocaleLowerCase().trim()
}

function matchTier(name: string, query: string): number | undefined {
  if (query === '') return 0
  const folded = foldForMatch(name)
  if (folded.startsWith(query)) return 0
  if (folded.split(/[\s\-_.]+/u).some((word) => word.startsWith(query))) return 1
  return undefined
}

/**
 * `recentIds` is most recent first and holds `MentionCandidate.subjectId`s: the last mentions this
 * member sent, then the people and agents in the loaded turns. A candidate not in it ranks after
 * every candidate that is.
 */
export function rankMentionCandidates(
  candidates: readonly MentionCandidate[],
  query: string,
  recentIds: readonly string[],
): readonly MentionCandidate[] {
  const normalized = foldForMatch(query)
  const recency = new Map<string, number>()
  recentIds.forEach((id, index) => {
    if (!recency.has(id)) recency.set(id, index)
  })
  return candidates
    .map((candidate) => ({ candidate, tier: matchTier(candidate.name, normalized) }))
    .filter((entry): entry is { candidate: MentionCandidate; tier: number } => entry.tier !== undefined)
    .sort((a, b) => {
      if (a.tier !== b.tier) return a.tier - b.tier
      const ar = recency.get(a.candidate.subjectId) ?? Number.POSITIVE_INFINITY
      const br = recency.get(b.candidate.subjectId) ?? Number.POSITIVE_INFINITY
      if (ar !== br) return ar - br
      return a.candidate.name.localeCompare(b.candidate.name)
    })
    .map((entry) => entry.candidate)
}

export type MentionSectionId = 'people' | 'agents' | 'add'

/** People first, then agents (the picker's two sections for room members). */
export function sectionOf(candidate: MentionCandidate): MentionSectionId {
  return candidate.kind === 'agent' ? 'agents' : 'people'
}

/**
 * Keep the ranked order inside each section and lay the sections out as the picker shows them:
 * People, Agents. The "Add to room" rows are appended by the caller (they are not room members).
 */
export function orderBySection(ranked: readonly MentionCandidate[]): readonly MentionCandidate[] {
  return [
    ...ranked.filter((candidate) => sectionOf(candidate) === 'people'),
    ...ranked.filter((candidate) => sectionOf(candidate) === 'agents'),
  ]
}
