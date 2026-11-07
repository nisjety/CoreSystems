/**
 * The reader's choice for each settled turn's fold, held for one view (EP-14 S1).
 *
 * Behind `VITE_CHAT_VIRTUALIZE` the transcript mounts only the turns on screen, so a
 * turn that scrolls out of the window loses its fold and mounts again with the
 * default. The choice is kept here instead, by the view that owns the transcript, so
 * it survives that remount. It lives in memory only: it is never written to the
 * thread, the device or the server, and the view forgets it when the thread changes.
 */

import { createSignal } from 'solid-js'

export type SettledFoldState = {
  /** The reader's choice for a turn's fold, or undefined while they have made none. */
  choice: (turnId: string) => boolean | undefined
  /** Records the reader's choice: true folds the turn, false opens it. */
  choose: (turnId: string, folded: boolean) => void
  /** Forgets every choice, as when the view moves to another thread. */
  reset: () => void
}

/**
 * A fresh, empty set of choices. Each view makes its own, so two views never share
 * one. Reads are reactive: a choice recorded or forgotten updates every reader.
 */
export function createSettledFoldState(): SettledFoldState {
  const choices = new Map<string, boolean>()
  const [revision, setRevision] = createSignal(0)
  const changed = () => setRevision((value) => value + 1)
  return {
    choice(turnId) {
      revision()
      return choices.get(turnId)
    },
    choose(turnId, folded) {
      if (choices.get(turnId) === folded) return
      choices.set(turnId, folded)
      changed()
    },
    reset() {
      if (choices.size === 0) return
      choices.clear()
      changed()
    },
  }
}
