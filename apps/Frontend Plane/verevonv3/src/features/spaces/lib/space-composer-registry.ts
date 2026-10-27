import type { JSX } from '@solidjs/web'

import type { AiModelSelection } from '@/shared/ai/model-selection'
import type { Skill } from '@/shared/api/skills-client'
import type { TurnPrivacyTierResolver } from '@/features/dashboard/home/submit-privacy-tier'

/**
 * The room composer's extension seams (P-SPA2). The composer renders whatever
 * is listed here, so a Space slice that adds a send mode, a chip row or a
 * toolbar control adds ONE entry to a registry and does not edit the composer
 * body:
 *
 * - modes: how a message leaves the box (`SPACE_COMPOSER_MODES`; the send
 *   pipeline in `space-send-pipeline.ts` resolves one strategy per mode id);
 * - chip slot: the row of chosen things above the textarea (skills today;
 *   attachments, context, mentions later);
 * - toolbar slot: controls beside the box (the model picker today; Browse,
 *   markdown toolbar, counter later).
 *
 * The slot entries themselves live in `components/composer/SpaceComposerSlots.tsx`
 * because they render JSX; this file holds the vocabulary only.
 *
 * `enabled` is a presentation switch only (a feature flag that keeps an
 * unfinished control out of the box). It is never an authorisation input: what a
 * member may send is decided by the owning plane, not by this list.
 */

export interface SpaceComposerModeDefinition {
  readonly id: string
  readonly labelNo: string
  readonly labelEn: string
  /** Hide the mode while false. Omitted means always offered. */
  readonly enabled?: () => boolean
}

export const SPACE_COMPOSER_MODES = [
  // WS01-S6: a human post, stored as the member's own message with no model run. The composer
  // offers it only while the room's `features.human_posts` is on and the role may post; with it
  // off the box is the single `ask` mode it always was.
  {
    id: 'post',
    labelNo: 'Send til rommet',
    labelEn: 'Post to the room',
  },
  {
    id: 'ask',
    labelNo: 'Spør Verevon',
    labelEn: 'Ask Verevon',
  },
] as const satisfies readonly SpaceComposerModeDefinition[]

export type SpaceComposerModeId = (typeof SPACE_COMPOSER_MODES)[number]['id']

/** The modes that stream a model turn through the send pipeline; `post` has its own transport (`space-post-sender.ts`). */
export type SpaceStreamedModeId = Exclude<SpaceComposerModeId, 'post'>

export type SpaceComposerModeEntry = SpaceComposerModeDefinition & { readonly id: SpaceComposerModeId }

/** Today's one behaviour: the message streams a model turn into the room. */
export const DEFAULT_SPACE_COMPOSER_MODE: SpaceComposerModeId = 'ask'

/** The modes to offer now: the registry minus any whose `enabled` says no. */
export function visibleSpaceComposerModes(): readonly SpaceComposerModeEntry[] {
  return (SPACE_COMPOSER_MODES as readonly SpaceComposerModeEntry[]).filter((mode) => mode.enabled?.() !== false)
}

/** A registered, visible mode id, or the default for anything else. */
export function resolveSpaceComposerMode(id: string | undefined): SpaceComposerModeId {
  const visible = visibleSpaceComposerModes()
  return visible.some((mode) => mode.id === id) ? (id as SpaceComposerModeId) : DEFAULT_SPACE_COMPOSER_MODE
}

/**
 * What a slot entry may read from the composer. Read-only accessors plus the
 * few setters an entry legitimately needs; the composer's own state (picker,
 * mention ref, exchange) stays inside the composer and the send pipeline.
 */
export interface SpaceComposerSlotContext {
  readonly spaceRef: string
  readonly orgId: string
  readonly mode: () => SpaceComposerModeId
  readonly setMode: (id: SpaceComposerModeId) => void
  /** The modes the composer offers right now (a subset of the registry; one means no switch is shown). */
  readonly modes: () => readonly SpaceComposerModeEntry[]
  /** A send is in flight, or the thread is gated; entries disable themselves. */
  readonly disabled: () => boolean
  readonly text: () => string
  readonly setText: (value: string) => void
  readonly focus: () => void
  readonly model: {
    readonly catalogue: TurnPrivacyTierResolver['catalogue']
    readonly onChange: (selection: AiModelSelection) => void
  }
  readonly skills: {
    readonly picked: () => readonly Skill[]
    readonly remove: (id: string) => void
  }
}

export interface SpaceComposerSlotEntry {
  readonly id: string
  /** Hide the entry while this returns false. Omitted means always shown. */
  readonly enabled?: (ctx: SpaceComposerSlotContext) => boolean
  /** Called inside the composer's reactive scope; may use `useI18n()`. */
  readonly render: (ctx: SpaceComposerSlotContext) => JSX.Element
}
