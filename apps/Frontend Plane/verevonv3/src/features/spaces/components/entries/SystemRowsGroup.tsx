import { createSignal, For, Show } from 'solid-js'

import type { SpaceRosterMember, SpaceSystemEntry } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { resolveAuthorName } from '../sidepane/SpaceTurnList'
import { SystemNoticeRow } from './SystemNoticeRow'
import './space-entries.css'

/** A run of more room changes than this collapses into one "N room changes" row. */
export const SYSTEM_ROWS_COLLAPSE_AFTER = 3

/**
 * The room's system rows that fall between two posts (WS15-S11), as one list item. Up to
 * {@link SYSTEM_ROWS_COLLAPSE_AFTER} rows show; a longer run shows its newest rows behind a button
 * that reads "N earlier room changes" until expanded. Names are resolved from the roster here so the
 * row itself only ever holds ids; an id the roster does not know reads as a former member.
 */
export function SystemRowsGroup(props: {
  readonly entries: readonly SpaceSystemEntry[]
  readonly roster: () => readonly SpaceRosterMember[]
  readonly viewerSubjectId?: () => string | undefined
}) {
  const i18n = useI18n()
  const [expanded, setExpanded] = createSignal(false)
  const hidden = () => (expanded() ? 0 : Math.max(0, props.entries.length - SYSTEM_ROWS_COLLAPSE_AFTER))
  const shown = () => props.entries.slice(hidden())
  const nameOf = (id: string | undefined) =>
    id ? resolveAuthorName(id, props.roster(), props.viewerSubjectId?.(), i18n.tr) : undefined
  return (
    <li class="verevon-room-system-item" role="presentation">
      <Show when={hidden() > 0}>
        <button type="button" class="verevon-room-system-more" onClick={() => setExpanded(true)}>
          {i18n.tr(`${hidden()} tidligere romendringer`, `${hidden()} earlier room changes`)}
        </button>
      </Show>
      <For each={shown()}>
        {(entry) => (
          <SystemNoticeRow entry={entry} actorName={nameOf(entry.actorId)} subjectName={nameOf(entry.subjectId)} />
        )}
      </For>
    </li>
  )
}
