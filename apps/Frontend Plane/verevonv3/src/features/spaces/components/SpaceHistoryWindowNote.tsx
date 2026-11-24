import { EyeOff } from '@/shared/icons'
import { Show } from 'solid-js'

import type { SpaceOwnHistoryWindow } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { useFormat } from '@/shared/preferences/use-format'

import { ownWindowDetail, ownWindowHidden } from '../lib/space-history-window'
import './space-history-window.css'

/**
 * D18: the quiet line above a timeline or a thread whose earlier messages are hidden from this member. It is drawn
 * only on Control's own `hidden: true` for the caller (`membership.history_window`), and says no more than
 * Control did: that earlier messages are hidden and, for a date or a join, from when the member reads.
 *
 * Presentation only. It grants and reveals nothing, and nothing here asks for the hidden messages: Control signs
 * the floor and Session Core applies it on every read, so the page never receives what it is not to show. A
 * `note`, not a `status`: it is background information, and a polite live region would announce it on every poll.
 */
export function SpaceHistoryWindowNote(props: { readonly window: () => SpaceOwnHistoryWindow | undefined }) {
  const i18n = useI18n()
  const fmt = useFormat()
  return (
    <Show when={ownWindowHidden(props.window()) ? props.window() : undefined}>
      {(own) => (
        <p class="verevon-space-history-window" role="note" data-mode={own().mode}>
          <EyeOff size={14} aria-hidden="true" />
          <span class="verevon-space-history-window__title">
            {i18n.tr('Tidligere meldinger er skjult', 'Earlier messages are hidden')}
          </span>
          <Show when={ownWindowDetail(own(), i18n, fmt)}>
            {(detail) => <span class="verevon-space-history-window__detail">{detail()}</span>}
          </Show>
        </p>
      )}
    </Show>
  )
}
