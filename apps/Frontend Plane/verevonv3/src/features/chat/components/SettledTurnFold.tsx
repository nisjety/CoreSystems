/**
 * The settled-turn fold (EP-14 S1): one row that says what a settled turn did,
 * over a body that holds its process trail.
 *
 * The row is a native button, so Enter and Space act as a click. While folded the
 * body is `hidden="until-found"`: where the browser supports it, find in the page
 * still reaches the hidden text and opens the body, and says so with `beforematch`,
 * which the row follows; elsewhere `hidden` is `display: none`. The folded state
 * belongs to the view (see `settled-fold-state.ts`), not to the row, so a row that
 * mounts again keeps the reader's choice. It is never written to the thread.
 */

import { createMemo, createUniqueId, onSettled, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'
import type { ConversationNodeEntry } from '@/shared/chat-nodes'
import { ChevronDown } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { settledFoldSummary, splitForFold, type SettledFold } from '../lib/settled-fold'
import { createSettledFoldState, type SettledFoldState } from '../lib/settled-fold-state'
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
  /** The turn the fold belongs to. Its choice is kept under this id in `foldState`. */
  turnId: string
  /** The view's fold choices. Absent, the choice is kept by this row alone. */
  foldState?: SettledFoldState
  entries: ConversationNodeEntry[]
  renderEntries: (entries: () => ConversationNodeEntry[]) => JSX.Element
}) {
  const split = createMemo(() => splitForFold(props.entries))
  const folds = createMemo(() => Boolean(props.fold) && split().folded.length > 0)
  const ownState = createSettledFoldState()
  const foldState = () => props.foldState ?? ownState
  return (
    <>
      <Show when={folds() ? props.fold : undefined}>
        {(fold) => (
          <SettledTurnFold
            fold={fold()}
            folded={foldState().choice(props.turnId) ?? fold().defaultFolded}
            onFoldedChange={(folded) => foldState().choose(props.turnId, folded)}
          >
            {props.renderEntries(() => split().folded)}
          </SettledTurnFold>
        )}
      </Show>
      {props.renderEntries(() => (folds() ? split().visible : props.entries))}
    </>
  )
}

/**
 * The fold row and its body. `folded` decides whether the body is shut. The row asks
 * its owner to change it, on a click or when find in the page reveals the body, and
 * keeps no state of its own.
 */
export function SettledTurnFold(props: {
  fold: SettledFold
  folded: boolean
  onFoldedChange: (folded: boolean) => void
  children: JSX.Element
}) {
  const i18n = useI18n()
  const bodyId = createUniqueId()
  let body: HTMLDivElement | undefined
  onSettled(() => {
    const element = body
    if (!element) return
    const reveal = () => props.onFoldedChange(false)
    element.addEventListener('beforematch', reveal)
    return () => element.removeEventListener('beforematch', reveal)
  })
  return (
    <div class="verevon-chat-settled-fold">
      <button
        type="button"
        class="verevon-chat-settled-fold__toggle"
        aria-expanded={props.folded ? 'false' : 'true'}
        aria-controls={bodyId}
        onClick={() => props.onFoldedChange(!props.folded)}
      >
        <span>{settledFoldSummary(props.fold, i18n)}</span>
        <ChevronDown size={14} aria-hidden="true" />
      </button>
      <div
        id={bodyId}
        class="verevon-chat-settled-fold__body"
        hidden={props.folded ? 'until-found' : undefined}
        ref={(element) => {
          body = element
        }}
      >
        {props.children}
      </div>
    </div>
  )
}
