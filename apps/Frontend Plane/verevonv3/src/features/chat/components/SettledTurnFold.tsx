/**
 * The settled-turn fold (EP-14 S1): one row that says what a settled turn did,
 * over a body that holds its process trail.
 *
 * The row is a native button, so Enter and Space act as a click. While folded the
 * body is `hidden="until-found"`: where the browser supports it, find in the page
 * still reaches the hidden text and opens the body, and says so with `beforematch`,
 * which the row follows; elsewhere `hidden` is `display: none`. The folded state
 * lives here, per view, and is never written to the thread.
 */

import { createMemo, createSignal, createUniqueId, onSettled, Show, untrack } from 'solid-js'
import type { JSX } from '@solidjs/web'
import type { ConversationNodeEntry } from '@/shared/chat-nodes'
import { ChevronDown } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { settledFoldSummary, splitForFold, type SettledFold } from '../lib/settled-fold'
import './chat-fold.css'

/**
 * A turn's node entries with the fold drawn around the process trail. With no fold,
 * or nothing for the fold to hold, every entry renders in place, as before.
 *
 * `renderEntries` is the transcript's own keyed list, so a row keeps its DOM when
 * the turn re-derives, whichever side of the split it lands on.
 */
export function SettledTurnContent(props: {
  fold?: SettledFold
  entries: ConversationNodeEntry[]
  renderEntries: (entries: () => ConversationNodeEntry[]) => JSX.Element
}) {
  const split = createMemo(() => splitForFold(props.entries))
  const folds = createMemo(() => Boolean(props.fold) && split().folded.length > 0)
  return (
    <>
      <Show when={folds() ? props.fold : undefined}>
        {(fold) => (
          <SettledTurnFold fold={fold()}>{props.renderEntries(() => split().folded)}</SettledTurnFold>
        )}
      </Show>
      {props.renderEntries(() => (folds() ? split().visible : props.entries))}
    </>
  )
}

/**
 * The fold row and its body. It starts folded or open as the plan says, and the
 * reader's choice holds from then on, so a re-derived plan does not close a turn
 * under them.
 */
export function SettledTurnFold(props: { fold: SettledFold; children: JSX.Element }) {
  const i18n = useI18n()
  const bodyId = createUniqueId()
  const [folded, setFolded] = createSignal(untrack(() => props.fold.defaultFolded))
  let body: HTMLDivElement | undefined
  onSettled(() => {
    const element = body
    if (!element) return
    const reveal = () => setFolded(false)
    element.addEventListener('beforematch', reveal)
    return () => element.removeEventListener('beforematch', reveal)
  })
  return (
    <div class="verevon-chat-settled-fold">
      <button
        type="button"
        class="verevon-chat-settled-fold__toggle"
        aria-expanded={folded() ? 'false' : 'true'}
        aria-controls={bodyId}
        onClick={() => setFolded((value) => !value)}
      >
        <span>{settledFoldSummary(props.fold, i18n)}</span>
        <ChevronDown size={14} aria-hidden="true" />
      </button>
      <div
        id={bodyId}
        class="verevon-chat-settled-fold__body"
        hidden={folded() ? 'until-found' : undefined}
        ref={(element) => {
          body = element
        }}
      >
        {props.children}
      </div>
    </div>
  )
}
