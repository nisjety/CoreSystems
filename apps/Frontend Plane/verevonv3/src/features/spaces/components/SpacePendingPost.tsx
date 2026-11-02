import { For } from 'solid-js'

import { useI18n } from '@/shared/i18n'
import type { SpaceJournalEntry } from '../lib/space-send-journal'
import { PostTurnHeader } from './sidepane/SpaceTurnList'
import './space-send.css'

/**
 * The sender's own post, shown the moment they press Send (WS01-S6, F-t1-033), in the place the
 * real one will take. It is the send journal's entry made visible, not a record: it says in words
 * where the send stands (`Sender …`, `Sender når du er på nett igjen`, `Ikke sendt, prøv igjen`)
 * and it goes away once the room's own projection carries the post, so it never sits beside its
 * real twin. Plain text only, as human posts are everywhere in the room.
 */
export function SpacePendingTurn(props: { readonly entry: SpaceJournalEntry }) {
  const i18n = useI18n()
  const stateText = () => {
    switch (props.entry.stage) {
      case 'queued':
        return i18n.tr('Sendes når du er på nett igjen', 'Sends when you are back online')
      case 'sending':
        return i18n.tr('Sender …', 'Sending…')
      case 'accepted':
        return i18n.tr('Sendt', 'Sent')
      case 'failed':
        return i18n.tr('Ikke sendt, prøv igjen', 'Not sent, try again')
    }
  }
  return (
    <div
      class="verevon-room-turn verevon-room-turn--pending"
      data-role="user"
      data-stage={props.entry.stage}
      data-send-id={props.entry.sendId}
    >
      <span class="verevon-room-turn__avatar" aria-hidden="true">{i18n.tr('Du', 'You').slice(0, 1)}</span>
      <div class="verevon-room-turn__content">
        <PostTurnHeader author={i18n.tr('Du', 'You')} />
        <p class="verevon-room-turn__plain">{props.entry.content}</p>
        <p
          class="verevon-room-pending__state"
          data-stage={props.entry.stage}
          role={props.entry.stage === 'failed' ? 'alert' : 'status'}
        >
          {stateText()}
        </p>
      </div>
    </div>
  )
}

/** A new post that is not in the room's list yet: its own post card at the end of the timeline. */
export function SpacePendingNewPost(props: { readonly entry: SpaceJournalEntry }) {
  return (
    <li class="verevon-room-post verevon-room-post--pending" data-stage={props.entry.stage}>
      <SpacePendingTurn entry={props.entry} />
    </li>
  )
}

/** The pending replies to one thread, drawn after its real turns. */
export function SpacePendingReplies(props: { readonly entries: readonly SpaceJournalEntry[] }) {
  return <For each={props.entries}>{(entry) => <SpacePendingTurn entry={entry} />}</For>
}
