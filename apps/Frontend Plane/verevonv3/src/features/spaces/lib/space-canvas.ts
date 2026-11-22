/**
 * The room's Canvas tab as the SPA presents it (WS19-S5): the tab's identity, its rollout gate, the refresh
 * interval and the line that says who saved a version. Presentation only. Who may read or write the canvas is
 * decided behind the gateway, and a refusal arrives as a typed error (`canvasErrorMessage` in shared/i18n/errors.ts).
 */
import type { SpaceCanvasVersion, SpaceRosterMember } from '@/shared/api/spaces-client'

import { DM_SPACE_FEATURE } from './space-kind'
import { spaceRoomFeature } from './space-room-features'

/** The tab's id in the registry and the URL hash (`#lerret`). `#canvas` is its alias. */
export const CANVAS_TAB_ID = 'lerret'

/** The rollout flag the room context states in its `features` block (`features.canvas`, WS19-S4). */
export const CANVAS_FEATURE = 'canvas'

/** The conditional refresh, while the tab is active and the document is visible. */
export const CANVAS_REFRESH_MS = 15_000

/**
 * D22aq: a canvas is cleared by saving one newline. wiki-store keeps no empty version (a document is 1 byte or more), so
 * this one-character version is the cleared state, and it is in the history like any other save.
 */
export const CANVAS_CLEAR_CONTENT = '\n'

/**
 * Whether the viewer is offered "Clear canvas": a room manager or owner (D22aq), the same role rule the room's other
 * manager actions use (`canOfferBrowserStop`). Presentation only: the save still goes through Control and wiki-store.
 */
export function canOfferCanvasClear(role: string | undefined): boolean {
  return role === 'manager' || role === 'owner'
}

type Translate = (noText: string, enText: string) => string

/**
 * Lists the tab while the gateway says the canvas is on. Off until the room context has been read. A direct message
 * has no canvas (Control refuses one in a `dm` or `group_dm` Space, D-8), so the tab is not offered there either:
 * the Agent and Members tabs follow the same trait.
 */
export function canvasTabEnabled(): boolean {
  return spaceRoomFeature(CANVAS_FEATURE) && !spaceRoomFeature(DM_SPACE_FEATURE)
}

/** A member's display name from the roster, or undefined when there is none. Never the subject id. */
export function canvasMemberName(roster: readonly SpaceRosterMember[], subjectId: string | null): string | undefined {
  if (!subjectId) return undefined
  const name = roster.find((member) => member.subject_id === subjectId)?.display_name?.trim()
  return name ? name : undefined
}

/**
 * Who saved a version, and when, without the text. A member: "Version 12, saved by Ola, 14:02". An agent run:
 * "Version 12, agent run 1a2b3c4d asked by Ola, 14:02", where the asker is the member the run acted for. `when`
 * is the time already formatted for the viewer.
 */
export function canvasVersionMeta(
  version: SpaceCanvasVersion,
  roster: readonly SpaceRosterMember[],
  when: string,
  tr: Translate,
): string {
  const number = String(version.versionNo)
  const timeSuffix = when ? `, ${when}` : ''
  if (version.authorKind === 'agent') {
    const run = version.runId ? ` ${version.runId.slice(0, 8)}` : ''
    const asker = canvasMemberName(roster, version.onBehalfOf)
    const who = asker
      ? tr(`agentkjøring${run} bedt om av ${asker}`, `agent run${run} asked by ${asker}`)
      : tr(`agentkjøring${run}`, `agent run${run}`)
    return tr(`Versjon ${number}, ${who}${timeSuffix}`, `Version ${number}, ${who}${timeSuffix}`)
  }
  const author = canvasMemberName(roster, version.authorId) ?? tr('et medlem', 'a member')
  return tr(`Versjon ${number}, lagret av ${author}${timeSuffix}`, `Version ${number}, saved by ${author}${timeSuffix}`)
}

/** One page of the history: the most versions the gateway lists at once (wiki-store's page limit). */
export const CANVAS_HISTORY_PAGE = 50

function decodedSegment(segment: string): string {
  try {
    return decodeURIComponent(segment)
  } catch {
    return segment
  }
}

/**
 * Whether a navigation leaves the room the panel shows, so that an open draft may be lost. A path under another
 * Space, a path outside the Spaces, and back or forward (a number, whose target this cannot name) all leave it. A
 * hash, a tab or a message link in this same Space does not: the panel and its draft stay mounted.
 */
export function canvasLeavesRoom(to: string | number, spaceRef: string): boolean {
  if (typeof to !== 'string') return true
  const match = /^\/spaces\/([^/?#]+)/.exec(to)
  if (!match) return true
  return decodedSegment(match[1] ?? '') !== spaceRef
}

/**
 * Whether two versions hold the same text, by the hash of their bytes the gateway sends with each. Two empty hashes
 * say nothing, so they never match.
 */
export function sameCanvasText(
  a: Pick<SpaceCanvasVersion, 'contentSha256'>,
  b: Pick<SpaceCanvasVersion, 'contentSha256'>,
): boolean {
  return a.contentSha256 !== '' && a.contentSha256 === b.contentSha256
}

/**
 * The compare selection after a version is picked: at most two, the older pick dropped when a third is picked, and a
 * version picked again is taken back out.
 */
export function toggleCompareSelection<T extends { readonly versionId: string }>(
  selected: readonly T[],
  version: T,
): readonly T[] {
  if (selected.some((item) => item.versionId === version.versionId)) {
    return selected.filter((item) => item.versionId !== version.versionId)
  }
  return [...selected, version].slice(-2)
}

/** Two versions in the order a comparison reads them: the older (lower number) first. */
export function canvasCompareOrder<T extends { readonly versionNo: number }>(a: T, b: T): readonly [T, T] {
  return a.versionNo <= b.versionNo ? [a, b] : [b, a]
}

/**
 * What a history line says about a restore: the number of the version it copied when that version is in the list
 * on screen, and a generic line when it is not. Null for an edit or a create.
 */
export function canvasChangeNote(
  version: SpaceCanvasVersion,
  versionNoOf: (versionId: string) => number | undefined,
  tr: Translate,
): string | null {
  if (version.changeKind !== 'restore') return null
  const from = version.restoredFromVersionId ? versionNoOf(version.restoredFromVersionId) : undefined
  return from === undefined
    ? tr('Gjenopprettet fra en tidligere versjon', 'Restored from an earlier version')
    : tr(`Gjenopprettet fra versjon ${from}`, `Restored from version ${from}`)
}
