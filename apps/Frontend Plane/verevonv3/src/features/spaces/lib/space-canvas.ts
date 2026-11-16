/**
 * The room's Canvas tab as the SPA presents it (WS19-S5): the tab's identity, its rollout gate, the refresh
 * interval and the line that says who saved a version. Presentation only. Who may read or write the canvas is
 * decided behind the gateway, and a refusal arrives as a typed error (`canvasErrorMessage` in shared/i18n/errors.ts).
 */
import type { SpaceCanvasVersion, SpaceRosterMember } from '@/shared/api/spaces-client'

import { spaceRoomFeature } from './space-room-features'

/** The tab's id in the registry and the URL hash (`#lerret`). `#canvas` is its alias. */
export const CANVAS_TAB_ID = 'lerret'

/** The rollout flag the room context states in its `features` block (`features.canvas`, WS19-S4). */
export const CANVAS_FEATURE = 'canvas'

/** The conditional refresh, while the tab is active and the document is visible. */
export const CANVAS_REFRESH_MS = 15_000

type Translate = (noText: string, enText: string) => string

/** Lists the tab while the gateway says the canvas is on. Off until the room context has been read. */
export function canvasTabEnabled(): boolean {
  return spaceRoomFeature(CANVAS_FEATURE)
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
