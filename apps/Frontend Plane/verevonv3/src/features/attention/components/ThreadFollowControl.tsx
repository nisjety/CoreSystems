import { Show, createSignal } from 'solid-js'

import { spaceRoomFeature } from '@/features/spaces/lib/space-room-features'
import {
  getThreadFollow,
  setThreadFollow,
  type ThreadFollowState,
} from '@/shared/api/spaces-attention-items-client'
import { useI18n } from '@/shared/i18n'
import { translateApiError } from '@/shared/i18n/errors'
import { createResource } from '@/shared/lib/create-resource-compat'

import '@/styles/attention.css'

export interface ThreadFollowControlProps {
  readonly spaceRef: string
  readonly threadId: string
}

/**
 * Follow or mute one post (WS11-S7). Following lets replies reach the viewer's attention inbox;
 * muting keeps them out. Drawn only while the room's `features.attention` is on. The state is the
 * viewer's own and is read from and written to the gateway, never kept in the browser. A refusal
 * (a Zero Data Retention organization cannot keep a follow) is said in words and nothing changes.
 */
export function ThreadFollowControl(props: ThreadFollowControlProps) {
  const i18n = useI18n()
  const [read, { mutate }] = createResource(
    () => (spaceRoomFeature('attention') ? `${props.spaceRef}\u0000${props.threadId}` : false),
    async (key): Promise<ThreadFollowState | 'unknown'> => {
      const [spaceRef = '', threadId = ''] = key.split('\u0000')
      try {
        return await getThreadFollow(spaceRef, threadId)
      } catch {
        // Unknown is not "not following": the buttons stay out rather than guess.
        return 'unknown'
      }
    },
  )
  const [busy, setBusy] = createSignal(false)
  const [message, setMessage] = createSignal('')
  const current = (): ThreadFollowState | undefined => {
    const value = read()
    return value === undefined || value === 'unknown' ? undefined : value
  }

  async function choose(next: ThreadFollowState) {
    if (busy()) return
    setBusy(true)
    setMessage('')
    try {
      mutate(await setThreadFollow(props.spaceRef, props.threadId, next))
    } catch (error) {
      setMessage(translateApiError(error, i18n.tr))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Show when={spaceRoomFeature('attention') && current() !== undefined}>
      <div class="verevon-thread-follow" role="group" aria-label={i18n.tr('Varsler for denne tråden', 'Notifications for this thread')}>
        <button
          type="button"
          class="verevon-attention-action"
          aria-pressed={current() === 'following'}
          disabled={busy()}
          onClick={() => void choose(current() === 'following' ? 'none' : 'following')}
        >
          {current() === 'following' ? i18n.tr('Følger', 'Following') : i18n.tr('Følg', 'Follow')}
        </button>
        <button
          type="button"
          class="verevon-attention-action"
          aria-pressed={current() === 'muted'}
          disabled={busy()}
          onClick={() => void choose(current() === 'muted' ? 'none' : 'muted')}
        >
          {current() === 'muted' ? i18n.tr('Dempet', 'Muted') : i18n.tr('Demp', 'Mute')}
        </button>
        <Show when={message()}>
          <span class="verevon-thread-follow__message" role="status">{message()}</span>
        </Show>
      </div>
    </Show>
  )
}
