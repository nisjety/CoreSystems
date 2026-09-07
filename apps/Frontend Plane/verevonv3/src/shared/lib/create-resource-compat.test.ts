import { createRoot, createSignal, flush } from 'solid-js'
import { describe, expect, it, vi } from 'vitest'

import { createResource } from './create-resource-compat'

/** Lets a test await the fetcher's microtasks without depending on real timers. */
async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

describe('createResource compat shim', () => {
  it('fetches once per distinct source value, not once per dependency write', async () => {
    // The shape that caused ten identical `GET .../instructions` per Space: a
    // resource keyed off a field of a periodically re-resolved context object.
    // The field never changes; only the object wrapping it does.
    const [context, setContext] = createSignal({ spaceRef: 'space_1' })
    const fetcher = vi.fn(async (ref: string) => `instructions for ${ref}`)

    const dispose = createRoot((disposeRoot) => {
      createResource(() => context().spaceRef, fetcher)
      return disposeRoot
    })
    flush()
    await settle()

    expect(fetcher).toHaveBeenCalledTimes(1)

    // A fresh object carrying the same ref — what a membership recheck resolves.
    setContext({ spaceRef: 'space_1' })
    flush()
    await settle()

    expect(fetcher).toHaveBeenCalledTimes(1)

    // A genuinely different Space must still refetch.
    setContext({ spaceRef: 'space_2' })
    flush()
    await settle()

    expect(fetcher).toHaveBeenCalledTimes(2)
    expect(fetcher).toHaveBeenLastCalledWith('space_2', expect.anything())

    dispose()
  })

  it('still refetches for the re-keying sources this codebase relies on', async () => {
    // Several call sites invalidate deliberately by returning a fresh tuple or
    // object (`() => [orgId, refreshKey] as const`, `() => ({ runId, tick })`).
    // Equality is `===`, so a new allocation must always refetch — otherwise
    // memoizing the source would silently break those refresh buttons.
    const [refreshKey, setRefreshKey] = createSignal(0)
    const fetcher = vi.fn(async (_key: readonly [string, number]) => 'rows')

    const dispose = createRoot((disposeRoot) => {
      createResource(() => ['org_1', refreshKey()] as const, fetcher)
      return disposeRoot
    })
    flush()
    await settle()

    expect(fetcher).toHaveBeenCalledTimes(1)

    setRefreshKey(1)
    flush()
    await settle()

    expect(fetcher).toHaveBeenCalledTimes(2)

    dispose()
  })

  it('refetches when a revision folded into a string key changes', async () => {
    // `SpaceRoomTimeline` keys a transcript on `${threadId}\0${updatedAt}` so a
    // settled reply refetches "exactly the post that changed". Value equality
    // is what makes that claim true.
    const [updatedAt, setUpdatedAt] = createSignal('2026-01-01T00:00:00Z')
    const fetcher = vi.fn(async (_key: string) => ['turn'])

    const dispose = createRoot((disposeRoot) => {
      createResource(() => `thread_1\0${updatedAt()}`, fetcher)
      return disposeRoot
    })
    flush()
    await settle()

    expect(fetcher).toHaveBeenCalledTimes(1)

    // Re-resolving the thread list without a new revision must not refetch
    // every post's transcript.
    setUpdatedAt('2026-01-01T00:00:00Z')
    flush()
    await settle()

    expect(fetcher).toHaveBeenCalledTimes(1)

    setUpdatedAt('2026-01-02T00:00:00Z')
    flush()
    await settle()

    expect(fetcher).toHaveBeenCalledTimes(2)

    dispose()
  })

  it('refetches on an explicit refetch() even when the source is unchanged', async () => {
    const fetcher = vi.fn(async (ref: string) => `v:${ref}`)
    let refetch!: () => Promise<unknown>

    const dispose = createRoot((disposeRoot) => {
      const [, actions] = createResource(() => 'space_1', fetcher)
      refetch = actions.refetch
      return disposeRoot
    })
    flush()
    await settle()

    expect(fetcher).toHaveBeenCalledTimes(1)

    await refetch()
    expect(fetcher).toHaveBeenCalledTimes(2)
    expect(fetcher).toHaveBeenLastCalledWith('space_1', expect.anything())

    dispose()
  })

  it('does not fetch while the source is falsy, then fetches when it resolves', async () => {
    const [ref, setRef] = createSignal<string | undefined>(undefined)
    const fetcher = vi.fn(async (value: string) => value)

    const dispose = createRoot((disposeRoot) => {
      createResource(ref, fetcher)
      return disposeRoot
    })
    flush()
    await settle()

    expect(fetcher).not.toHaveBeenCalled()

    setRef('space_1')
    flush()
    await settle()

    expect(fetcher).toHaveBeenCalledTimes(1)

    dispose()
  })
})
