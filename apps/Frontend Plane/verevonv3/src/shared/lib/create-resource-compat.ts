import { createEffect, createMemo, createSignal, type Accessor } from 'solid-js'

/**
 * Solid 2 removes createResource entirely (replaced by async createMemo read
 * through Loading/Errored boundaries). This app has zero Suspense/ErrorBoundary
 * usage anywhere — every call site reads `.loading`/`.error` off the resource
 * directly instead. Rather than rewrite ~70 call sites' surrounding render
 * logic against the new boundary-based model (and risk each one suspending
 * uncaught with no boundary to catch it), this shim reproduces the old
 * createResource ergonomics on top of v2's confirmed primitives — createSignal
 * and two-phase createEffect — so call sites need only an import swap.
 */

export type ResourceInfo<T> = { value: T | undefined }
export type ResourceAccessor<T> = Accessor<T | undefined> & {
  readonly loading: boolean
  readonly error: unknown
  readonly latest: T | undefined
}
export type ResourceActions<T> = {
  refetch: () => Promise<T | undefined>
  mutate: (next: T | undefined | ((prev: T | undefined) => T | undefined)) => void
}

export function createResource<T>(
  fetcher: (info: ResourceInfo<T>) => Promise<T>,
): [ResourceAccessor<T>, ResourceActions<T>]
// Generic order (T, S) matches Solid 1's createResource<T, S>(source, fetcher)
// convention, since call sites migrated from v1 specify generics that way.
export function createResource<T, S = true>(
  source: Accessor<S | false | null | undefined>,
  fetcher: (source: S, info: ResourceInfo<T>) => Promise<T>,
): [ResourceAccessor<T>, ResourceActions<T>]
export function createResource(
  sourceOrFetcher: Accessor<unknown> | ((info: ResourceInfo<unknown>) => Promise<unknown>),
  maybeFetcher?: (source: unknown, info: ResourceInfo<unknown>) => Promise<unknown>,
): [ResourceAccessor<unknown>, ResourceActions<unknown>] {
  const hasSource = maybeFetcher !== undefined
  const source = (hasSource ? (sourceOrFetcher as Accessor<unknown>) : () => true)
  const fetcher = (hasSource ? maybeFetcher! : (sourceOrFetcher as (info: ResourceInfo<unknown>) => Promise<unknown>))

  const [value, setValue] = createSignal<unknown>(undefined)
  const [loading, setLoading] = createSignal(false)
  const [error, setError] = createSignal<unknown>(undefined)
  let latestValue: unknown
  let lastSourceValue: unknown
  let callId = 0

  function run(sourceValue: unknown): Promise<unknown> {
    if (hasSource && (sourceValue === false || sourceValue === null || sourceValue === undefined)) {
      setLoading(false)
      return Promise.resolve(undefined)
    }
    const id = ++callId
    setLoading(true)
    setError(undefined)
    const result$ = hasSource
      ? (fetcher as (s: unknown, info: ResourceInfo<unknown>) => Promise<unknown>)(sourceValue, { value: latestValue })
      : (fetcher as (info: ResourceInfo<unknown>) => Promise<unknown>)({ value: latestValue })

    return Promise.resolve(result$)
      .then((result) => {
        if (id !== callId) return result
        latestValue = result
        setValue(() => result)
        setLoading(false)
        return result
      })
      .catch((err: unknown) => {
        if (id !== callId) return undefined
        setError(err)
        setLoading(false)
        throw err
      })
  }

  // Memoized, so the fetcher re-runs when the source *value* changes rather
  // than whenever any signal the accessor happens to touch is written.
  //
  // A source accessor is tracked, not compared. Without this, a source like
  // `() => context()?.space_ref` refetched every time `context` resolved a
  // fresh-but-equivalent object — a periodic authority recheck, say — even
  // though the ref string never changed. Callers had no way to opt out short
  // of memoizing at each call site.
  //
  // This only ever removes redundant fetches. Equality is `===`, so the
  // deliberate re-keying patterns in this codebase are untouched: sources that
  // return an object or array literal (`() => [orgId, refreshKey] as const`,
  // `() => ({ runId, tick: planTick() })`) allocate a fresh value per read and
  // still refetch, and sources that fold a revision into a string key still
  // refetch exactly when that key changes — which is what those call sites
  // already document themselves as doing.
  const trackedSource = createMemo(() => source())

  createEffect(
    () => trackedSource(),
    (sourceValue) => {
      lastSourceValue = sourceValue
      // `run` rethrows so that an awaited `refetch()` can observe the failure,
      // but this source-driven fetch has no caller to catch it. The rejection is
      // already recorded in the `error` signal every call site renders, so drop
      // it here instead of leaving a floating promise — otherwise each failed
      // load escapes as an unhandled rejection and can fail a test run after
      // teardown, long after the component handled the error.
      void run(sourceValue).catch(() => {})
    },
  )

  const accessor = (() => value()) as ResourceAccessor<unknown>
  Object.defineProperty(accessor, 'loading', { get: () => loading() })
  Object.defineProperty(accessor, 'error', { get: () => error() })
  Object.defineProperty(accessor, 'latest', { get: () => (loading() ? latestValue : value()) })

  const actions: ResourceActions<unknown> = {
    refetch: () => run(lastSourceValue),
    mutate: (next) => {
      setValue((prev) => (typeof next === 'function' ? (next as (p: unknown) => unknown)(prev) : next))
      latestValue = value()
    },
  }

  return [accessor, actions]
}
