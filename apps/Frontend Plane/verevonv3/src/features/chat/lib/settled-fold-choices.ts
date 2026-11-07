/**
 * The reader's own fold choices, held per view (EP-14 S1).
 *
 * A settled turn's row folds or opens by the plan until the reader acts on it, and from then on the choice holds.
 * The view holds the choices and the thread never does: nothing is stored or sent, and a choice ends with the view.
 * Because the view keeps it, a row that virtualisation scrolls out of the window and mounts again keeps the reader's
 * choice.
 */

import { createSignal } from 'solid-js'

export type SettledFoldChoices = {
  /** Whether a turn's body is folded: the reader's choice when there is one, otherwise the plan's default. */
  isFolded: (turnId: string, defaultFolded: boolean) => boolean
  /** Records the reader's choice for a turn. */
  choose: (turnId: string, folded: boolean) => void
}

/** A fresh set of choices, for one view. */
export function createSettledFoldChoices(): SettledFoldChoices {
  const [choices, setChoices] = createSignal<ReadonlyMap<string, boolean>>(new Map())
  return {
    isFolded: (turnId, defaultFolded) => choices().get(turnId) ?? defaultFolded,
    choose: (turnId, folded) => setChoices((current) => new Map(current).set(turnId, folded)),
  }
}
