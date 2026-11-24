/**
 * DEPTH-WS04-S2-1: the Spaces listings (the sidebar panel and the Spaces index) read each room's name once per mount,
 * so a rename or a new topic made on the room's page left the old name in the sidebar until a reload. The room page
 * announces the change here, and each listing reads the listing again. The signal carries no room data, and nothing
 * in authorization reads it.
 */
type SpacesListChangedListener = () => void

const listeners = new Set<SpacesListChangedListener>()

/** Tells every mounted Spaces listing that a room's name or topic changed, so each one reads the listing again. */
export function notifySpacesListChanged(): void {
  for (const listener of [...listeners]) {
    try {
      listener()
    } catch {
      // One listing that throws must not stop the others from reading the listing again.
    }
  }
}

/** Subscribes a Spaces listing to those changes. The returned function unsubscribes, for `onCleanup`. */
export function onSpacesListChanged(listener: SpacesListChangedListener): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}
