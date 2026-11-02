/**
 * What a Space kind means for the page (WS12-S3). One place answers "is this a DM, and what does a
 * DM not have", so the sidebar, header, tab strip and composer cannot disagree. Presentation only,
 * never an authorization input: Control's fixed roster and Application's guards refuse the writes
 * these traits hide.
 */
import type { SpaceDmParticipant } from '@/shared/api/spaces-client/dm'

export type SpaceKindTraits = {
  /** `dm` or `group_dm`. */
  readonly dm: boolean
  /** The roster is fixed at registration: no add or remove member. */
  readonly fixedRoster: boolean
  /** The Members tab is offered. */
  readonly showMembers: boolean
  /** The Agent tab and agent controls are offered. */
  readonly showAgents: boolean
  /** The composer offers "Ask Verevon" (a model run). A DM is people only. */
  readonly canAsk: boolean
}

const DM_KINDS = new Set(['dm', 'group_dm'])

export function isDmKind(kind: string | undefined | null): boolean {
  return typeof kind === 'string' && DM_KINDS.has(kind)
}

export function spaceKindTraits(kind: string | undefined | null): SpaceKindTraits {
  const dm = isDmKind(kind)
  return { dm, fixedRoster: dm, showMembers: !dm, showAgents: !dm, canAsk: !dm }
}

/**
 * The room-features key the page publishes for a DM, so module-level readers (the tab strip, the
 * composer) can follow the kind through the same store as every other rollout flag.
 */
export const DM_SPACE_FEATURE = 'space_dm'

/** A context's `features` plus the kind trait. Pass the result to the features store. */
export function withSpaceKindTraits(
  context: { readonly features?: Readonly<Record<string, unknown>> | null; readonly space?: { readonly kind?: string } } | null | undefined,
): Readonly<Record<string, unknown>> | undefined {
  if (!context) return undefined
  return { ...(context.features ?? {}), [DM_SPACE_FEATURE]: isDmKind(context.space?.kind) }
}

/**
 * A DM's name: the other people, by name. `viewerSubjectId` leaves the viewer out; with no one
 * else (or no names) the generic label is returned.
 */
export function dmParticipantsLabel(
  participants: readonly SpaceDmParticipant[] | undefined,
  viewerSubjectId: string | undefined,
  tr: (no: string, no_en: string) => string,
  fallbackName?: string,
): string {
  const others = (participants ?? [])
    .filter((participant) => !viewerSubjectId || participant.subject_id !== viewerSubjectId)
    .map((participant) => participant.display_name?.trim() || '')
    .filter((name) => name !== '')
  if (others.length === 0) return fallbackName?.trim() || tr('Direktemelding', 'Direct message')
  return others.join(', ')
}
