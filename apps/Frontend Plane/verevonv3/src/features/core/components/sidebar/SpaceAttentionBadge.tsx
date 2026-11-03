import { Show, createSignal, onSettled } from 'solid-js'

import { announce } from '@/shared/a11y/announcer'
import { markAllSpacesRead } from '@/shared/api/spaces-attention-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import {
  refreshSpaceAttention,
  retainSpaceAttention,
  spaceAttentionRoom,
  spaceAttentionStale,
  spaceAttentionTotals,
} from '@/features/spaces/lib/space-attention'
import type { AttentionBadge } from '@/shared/api/spaces-attention-client'
import './space-attention-badge.css'

/**
 * Join the shared attention poller for as long as the calling component lives (WS11-S4). One poller
 * serves the whole browser however many of these are mounted.
 */
export function useSpaceAttentionPoller(): void {
  const i18n = useI18n()
  // Joined once settled, not in the component body: starting the poller writes signals (its clock and
  // its first state), and Solid 2 refuses a write inside a component body (REACTIVE_WRITE_IN_OWNED_SCOPE).
  // The returned release is the cleanup.
  onSettled(() => retainSpaceAttention(i18n.tr))
}

function Mark(props: { badge: AttentionBadge; strong: boolean; stale: boolean; label: string }) {
  return (
    <Show when={props.badge.kind !== 'none'}>
      <span
        class="space-attention-badge"
        data-kind={props.badge.kind}
        data-strong={props.strong ? 'true' : undefined}
        data-stale={props.stale ? 'true' : undefined}
        role="img"
        aria-label={props.label}
        title={props.stale ? props.label : undefined}
      >
        <Show when={props.badge.kind === 'count'}>
          <span aria-hidden="true">{props.badge.n > 99 ? '99+' : props.badge.n}</span>
        </Show>
      </span>
    </Show>
  )
}

/**
 * A room row's badge, from the server's `badge` for that room (never recomputed). Rendered only for
 * a room the open page is NOT already narrating: the open room's own live work stays on its row.
 * `strong` is the styling difference between "something needs you" and "a quiet unread count".
 */
export function SpaceRoomAttentionBadge(props: { spaceRef: string }) {
  const i18n = useI18n()
  const room = () => spaceAttentionRoom(props.spaceRef)
  const label = () => {
    const current = room()
    if (!current) return ''
    const stale = spaceAttentionStale() ? i18n.tr(' (kan være utdatert)', ' (may be out of date)') : ''
    if (current.badge.kind === 'dot') return i18n.tr('Ny aktivitet', 'New activity') + stale
    if (current.mentions + current.needsYou > 0) {
      return i18n.tr(`${current.badge.n} trenger deg`, `${current.badge.n} need you`) + stale
    }
    return i18n.tr(`${current.badge.n} uleste`, `${current.badge.n} unread`) + stale
  }
  return (
    <Show when={room()}>
      {(current) => (
        <Mark
          badge={current().badge}
          strong={current().mentions + current().needsYou > 0}
          stale={spaceAttentionStale()}
          label={label()}
        />
      )}
    </Show>
  )
}

/** The sum across rooms for a navigation item (`SidebarPanelItem.badge === 'attention'`). */
export function SpaceTotalAttentionBadge() {
  const i18n = useI18n()
  useSpaceAttentionPoller()
  const total = () => spaceAttentionTotals()?.badge ?? 0
  return (
    <Show when={total() > 0}>
      <Mark
        badge={{ kind: 'count', n: total() }}
        strong
        stale={spaceAttentionStale()}
        label={i18n.tr(`${total()} trenger deg i rom`, `${total()} need you in rooms`)}
      />
    </Show>
  )
}

/**
 * "Mark all read" and the freshness note, under the Spaces search field. Shown only when the server
 * counted something (`unread_rooms` or `badge`); it advances existing markers and resolves read-driven
 * items and never touches "needs you" (that is the server's rule). Joins the shared poller.
 */
export function SpaceAttentionReadAll() {
  const i18n = useI18n()
  useSpaceAttentionPoller()
  const [busy, setBusy] = createSignal(false)
  const [failure, setFailure] = createSignal('')
  const anything = () => {
    const totals = spaceAttentionTotals()
    return totals !== undefined && (totals.unreadRooms > 0 || totals.badge > 0)
  }
  async function markAll(): Promise<void> {
    if (busy()) return
    setBusy(true)
    setFailure('')
    try {
      await markAllSpacesRead()
      announce(i18n.tr('Alle rom er merket som lest.', 'All rooms marked as read.'), { key: 'space-attention-read-all' })
      refreshSpaceAttention()
    } catch (error) {
      setFailure(translateApiError(error, i18n.tr))
    } finally {
      setBusy(false)
    }
  }
  return (
    <Show when={anything() || failure() || spaceAttentionStale()}>
      <div class="space-attention-readall">
        <Show when={anything()}>
          <button type="button" disabled={busy()} onClick={() => { void markAll() }}>
            {i18n.tr('Merk alle som lest', 'Mark all read')}
          </button>
        </Show>
        <Show when={failure()}>
          <span role="alert">{failure()}</span>
        </Show>
        <Show when={spaceAttentionStale()}>
          <span class="space-attention-readall__stale">{i18n.tr('Tellingen kan være utdatert', 'Counts may be out of date')}</span>
        </Show>
      </div>
    </Show>
  )
}
