/**
 * Keyed projection channel: a guard for async writes into shared UI state.
 *
 * # The bug class
 *
 * A chat stream's handlers fire minutes after they were opened and write into
 * store state shared by every thread. Two ways that goes wrong:
 *
 * 1. **Wrong key.** The user opened another thread. Turn-scoped writes are
 *    accidentally safe (they match on turn id and find nothing), but a global
 *    write — a status flip, a transcript snapshot, a generated title — lands on
 *    whatever is now on screen. The chat controller had hand-written
 *    `ownsMachine()` checks at most of those sites, and a missing one on
 *    `onTitle` could lock one thread's AI-generated title onto another
 *    permanently.
 *
 * 2. **Wrong generation.** The SAME key, an older connection. Sending a second
 *    message does not abort the first stream, so both are live on one thread and
 *    a key check passes for both. The first stream's late `done` then flips the
 *    status to idle while the second is still answering.
 *
 * A key check alone cannot see (2), which is why generation is part of the
 * channel and not an afterthought.
 *
 * # Why drops are counted
 *
 * A dropped write is normal (that is the point) but a *surprising number* of
 * them means something is wrong — a stream that never learned it was superseded,
 * or a key that changes more often than the user switches threads. Silent
 * dropping would make the guard indistinguishable from a handler that never
 * fired.
 */

export type ProjectionDropReason = 'key' | 'generation'

export type ProjectionChannel = {
  /** This channel's connection generation. */
  readonly generation: number
  /**
   * Whether a write from this channel is still valid — and the only guard.
   *
   * Deliberately a predicate rather than a `commit(write)` wrapper. A wrapper
   * reads better, but it means every guarded site passes a closure that reads
   * store state, which Solid's reactivity lint flags (correctly in general: it
   * cannot know `commit` invokes synchronously). A predicate keeps the guarded
   * writes inline, exactly as the hand-written checks were.
   *
   * It counts and reports rejections as a side effect, so observability comes
   * with the guard instead of needing a second call.
   */
  accepts: () => boolean
  /** How many writes this channel has rejected, by reason. */
  drops: () => Readonly<Record<ProjectionDropReason, number>>
}

export function openProjectionChannel(options: {
  /** True while this channel's key is the one on screen. */
  ownsKey: () => boolean
  /** This connection's generation, taken when it opened. */
  generation: number
  /** The newest generation opened for this key. */
  activeGeneration: () => number
  /** Observability hook; called once per dropped write. */
  onDrop?: (reason: ProjectionDropReason, generation: number) => void
}): ProjectionChannel {
  const dropped: Record<ProjectionDropReason, number> = { key: 0, generation: 0 }

  const reason = (): ProjectionDropReason | null => {
    if (!options.ownsKey()) return 'key'
    // Strictly greater: an equal generation IS this channel. Using `!==` would
    // also reject the current one if the counter were ever reset.
    if (options.activeGeneration() > options.generation) return 'generation'
    return null
  }

  return {
    generation: options.generation,
    accepts: () => {
      const why = reason()
      if (!why) return true
      dropped[why] += 1
      options.onDrop?.(why, options.generation)
      return false
    },
    drops: () => ({ ...dropped }),
  }
}
