import type { SpaceAgent, SpaceAgentRespondTo, SpaceRosterMember } from '@/shared/api/spaces-client'
import { spaceRoomFeatureWithDevOverride } from './space-room-features'

// WS08-S8: the pure half of the "who can wake it" editor. The gateway enforces `respond_to` on
// every mention and wake (domains/spaces `respond_to_allows`) and the Application plane stores it;
// everything here is presentation. A binding without the field is `members` (any editor, manager or
// owner), the same default the gateway applies, so the chip says what will actually happen.

/** `features.agent_wake_policy` from the BFF; `VITE_SPACE_AGENT_WAKE_POLICY` overrides it in dev builds only. */
export function agentWakePolicyEnabled(): boolean {
  return spaceRoomFeatureWithDevOverride('agent_wake_policy', import.meta.env.VITE_SPACE_AGENT_WAKE_POLICY)
}

export const WAKE_POLICY_MODES: readonly SpaceAgentRespondTo[] = ['members', 'managers', 'allowlist']

/** The mode in force: the stored one, or `members` when the binding predates the field or carries a retired value. */
export function respondToOf(agent: Pick<SpaceAgent, 'respond_to'>): SpaceAgentRespondTo {
  const value = agent.respond_to
  return value === 'managers' || value === 'allowlist' ? value : 'members'
}

export function respondToSubjectsOf(agent: Pick<SpaceAgent, 'respond_to' | 'respond_to_subjects'>): readonly string[] {
  return respondToOf(agent) === 'allowlist' ? (agent.respond_to_subjects ?? []) : []
}

type Tr = (no: string, en: string) => string

/** The chip text on the agent card. */
export function respondToChipLabel(mode: SpaceAgentRespondTo, count: number, tr: Tr): string {
  switch (mode) {
    case 'members':
      return tr('Kan tilkalles av alle med skrivetilgang', 'Anyone who can post can wake it')
    case 'managers':
      return tr('Kan tilkalles av eiere og ledere', 'Owners and managers can wake it')
    case 'allowlist':
      return tr(`Kan tilkalles av ${count} utvalgte`, `${count} chosen people can wake it`)
  }
}

export function respondToModeLabel(mode: SpaceAgentRespondTo, tr: Tr): string {
  switch (mode) {
    case 'members':
      return tr('Alle med skrivetilgang', 'Everyone who can post')
    case 'managers':
      return tr('Bare eiere og ledere', 'Owners and managers only')
    case 'allowlist':
      return tr('Bare utvalgte personer', 'Only chosen people')
  }
}

export function respondToModeHint(mode: SpaceAgentRespondTo, tr: Tr): string {
  switch (mode) {
    case 'members':
      return tr(
        'Redaktører, ledere og eiere kan tilkalle agenten med @.',
        'Editors, managers and owners can call the agent with @.',
      )
    case 'managers':
      return tr(
        'Bare eiere og ledere kan tilkalle agenten. Andre kan fortsatt nevne den, men den svarer ikke dem.',
        'Only owners and managers can call the agent. Others can still mention it, but it will not answer them.',
      )
    case 'allowlist':
      return tr(
        'Bare personene du velger kan tilkalle agenten (høyst 50).',
        'Only the people you choose can call the agent (at most 50).',
      )
  }
}

/**
 * The "why it did not answer" line shown under the chip, in words a member can act on. It is the
 * standing explanation, true whenever the mode is not `members`; the per-post reason (paused agent,
 * someone else's conversation) is `REFUSAL_REASON_CODE` in mention-model.ts and the WakeStatus line.
 */
export function whyNoAnswerHint(mode: SpaceAgentRespondTo, tr: Tr): string | null {
  switch (mode) {
    case 'members':
      return null
    case 'managers':
      return tr(
        'Svarer ikke hvis du bare er redaktør: en eier eller leder må tilkalle den, eller endre hvem som kan.',
        'It will not answer if you are only an editor: an owner or manager has to call it, or change who can.',
      )
    case 'allowlist':
      return tr(
        'Svarer ikke hvis du ikke står på listen: be en eier eller leder legge deg til.',
        'It will not answer if you are not on the list: ask an owner or manager to add you.',
      )
  }
}

/** People the allowlist can name: humans who can post. Viewers and agents are never wakers. */
export function allowlistCandidates(roster: readonly SpaceRosterMember[]): readonly SpaceRosterMember[] {
  return roster.filter((member) => member.subject_type === 'user' && member.role !== 'viewer')
}

export const WAKE_ALLOWLIST_MAX = 50

export type WakePolicyDraft = {
  readonly mode: SpaceAgentRespondTo
  readonly subjects: readonly string[]
}

export function draftFromAgent(agent: Pick<SpaceAgent, 'respond_to' | 'respond_to_subjects'>): WakePolicyDraft {
  return { mode: respondToOf(agent), subjects: respondToSubjectsOf(agent) }
}

export function draftsEqual(a: WakePolicyDraft, b: WakePolicyDraft): boolean {
  if (a.mode !== b.mode) return false
  if (a.mode !== 'allowlist') return true
  if (a.subjects.length !== b.subjects.length) return false
  const left = new Set(a.subjects)
  return b.subjects.every((id) => left.has(id))
}

export type WakePolicyProblem = 'allowlist_empty' | 'allowlist_too_long'

export function draftProblem(draft: WakePolicyDraft): WakePolicyProblem | null {
  if (draft.mode !== 'allowlist') return null
  if (draft.subjects.length === 0) return 'allowlist_empty'
  if (draft.subjects.length > WAKE_ALLOWLIST_MAX) return 'allowlist_too_long'
  return null
}

export function draftProblemMessage(problem: WakePolicyProblem, tr: Tr): string {
  return problem === 'allowlist_empty'
    ? tr('Velg minst én person, eller velg en annen modus.', 'Choose at least one person, or pick another mode.')
    : tr(`Listen kan ha høyst ${WAKE_ALLOWLIST_MAX} personer.`, `The list can hold at most ${WAKE_ALLOWLIST_MAX} people.`)
}
