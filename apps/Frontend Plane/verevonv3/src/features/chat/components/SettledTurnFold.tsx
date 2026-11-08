/**
 * The settled-turn fold (EP-14 S1): one row that says what a settled turn did,
 * over a body that holds its process trail.
 *
 * The row is a native button, so Enter and Space act as a click. While folded the
 * body is `hidden="until-found"`: where the browser supports it, find in the page
 * still reaches the hidden text and opens the body, and says so with `beforematch`,
 * which the row follows; elsewhere `hidden` is `display: none`. The row is one line
 * of fixed height whatever its label says, so the virtual planner measures it the
 * same way each time. The reader's choice is the view's (see `settled-fold-choices.ts`),
 * never the thread's.
 */

import { createMemo, createUniqueId, onSettled, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'
import type { ConversationNodeEntry } from '@/shared/chat-nodes'
import { ChevronDown } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { settledFoldSummary, splitForFold, type SettledFold } from '../lib/settled-fold'
import { createSettledFoldChoices, type SettledFoldChoices } from '../lib/settled-fold-choices'
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
  /** The turn the fold belongs to: the key of the reader's choice. */
  turnId: string
  /** The view's fold choices. Absent, the fold keeps its choice for this mount only. */
  choices?: SettledFoldChoices
  entries: ConversationNodeEntry[]
  renderEntries: (entries: () => ConversationNodeEntry[]) => JSX.Element
}) {
  const split = createMemo(() => splitForFold(props.entries))
  const folds = createMemo(() => Boolean(props.fold) && split().folded.length > 0)
  return (
    <>
      <Show when={folds() ? props.fold : undefined}>
        {(fold) => (
          <SettledTurnFold fold={fold()} turnId={props.turnId} choices={props.choices}>
            {props.renderEntries(() => split().folded)}
          </SettledTurnFold>
        )}
      </Show>
      {props.renderEntries(() => (folds() ? split().visible : props.entries))}
    </>
  )
}

/**
 * The row and its body. The plan's default decides whether it starts folded or open
 * until the reader acts on it. From then on the view's choice holds, so a plan that
 * is derived again does not close a turn under them.
 */
export function SettledTurnFold(props: {
  fold: SettledFold
  turnId: string
  choices?: SettledFoldChoices
  children: JSX.Element
}) {
  const i18n = useI18n()
  const bodyId = createUniqueId()
  const ownChoices = createSettledFoldChoices()
  const choices = () => props.choices ?? ownChoices
  const folded = () => choices().isFolded(props.turnId, props.fold.defaultFolded)
  const label = () => settledFoldSummary(props.fold, i18n)
  let body: HTMLDivElement | undefined
  onSettled(() => {
    const element = body
    if (!element) return
    const reveal = () => choices().choose(props.turnId, false)
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
        onClick={() => choices().choose(props.turnId, !folded())}
      >
        <span class="verevon-chat-settled-fold__label" title={label()}>{label()}</span>
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
