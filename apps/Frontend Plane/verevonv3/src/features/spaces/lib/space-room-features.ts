import { createSignal } from 'solid-js'

/**
 * The room's rollout flags as the BFF states them (`GET /spaces/{ref}/context` -> `features`).
 *
 * The four UI switches that used to be build-time (`VITE_SPACE_THREAD_PANE_UI`,
 * `VITE_SPACE_SEND_IDEMPOTENCY_API`, `VITE_SPACE_CONNECTIVITY_BANNER_UI`,
 * `VITE_SPACE_ROOM_ARTIFACTS_UI`) read from here. The page publishes the context's `features`
 * the moment a context read lands, so by the time a room's children render the answer is
 * already in; a room that has not been read yet, or an older gateway that sends no `features`,
 * means every flag is off. Presentation only, never an authorization input: every action these
 * switches reveal is re-checked by the gateway and the owning plane.
 *
 * The value is held in a plain variable and a version signal beside it. Readers get the
 * variable, so a read straight after a publish is current even before Solid applies the signal
 * write, and a reader inside a memo or JSX still re-runs when the version moves.
 */
export type SpaceRoomFeatures = Readonly<Record<string, unknown>>

let latest: SpaceRoomFeatures | undefined
// What the version signal's readers have last been told. It trails `latest` after a `prime`.
let announced: SpaceRoomFeatures | undefined
const [version, setVersion] = createSignal(0)

function sameFeatures(a: SpaceRoomFeatures | undefined, b: SpaceRoomFeatures | undefined): boolean {
  if (a === b) return true
  if (!a || !b) return false
  const keys = new Set([...Object.keys(a), ...Object.keys(b)])
  for (const key of keys) {
    const left = a[key]
    const right = b[key]
    // A flag that is a block of flags (`room_files`) changes when any member does.
    const comparable = (value: unknown) => (value !== null && typeof value === 'object' ? JSON.stringify(value) : value === true)
    if (comparable(left) !== comparable(right)) return false
  }
  return true
}

function copyOf(next: SpaceRoomFeatures | null | undefined): SpaceRoomFeatures | undefined {
  return next && typeof next === 'object' ? { ...next } : undefined
}

/**
 * Make the flags readable right now, without touching the signal. The page calls this while it
 * renders a room, so a child that reads a flag once in its body already sees the right answer.
 * A signal write is not allowed there; {@link publishSpaceRoomFeatures} follows from an effect and
 * tells the readers that are tracking.
 */
export function primeSpaceRoomFeatures(next: SpaceRoomFeatures | null | undefined): void {
  latest = copyOf(next)
}

/**
 * Publish the context's `features` to tracking readers. Does nothing when no flag changed since
 * the last publish, so the page's 30 s authority recheck never churns readers.
 */
export function publishSpaceRoomFeatures(next: SpaceRoomFeatures | null | undefined): void {
  const value = copyOf(next)
  latest = value
  if (sameFeatures(announced, value)) return
  announced = value
  setVersion((current) => current + 1)
}

/**
 * Forget the flags when the page leaves the room. Deliberately does not touch the signal: this runs
 * from a cleanup, where a signal write can land inside another computation's update, and nothing
 * that was reading the flags is alive any more. The next publish always differs from `undefined`,
 * so it notifies.
 */
export function clearSpaceRoomFeatures(): void {
  latest = undefined
  announced = undefined
}

/** One flag as the BFF last said it. Off when the room has not been read or the key is absent. */
export function spaceRoomFeature(name: string): boolean {
  version()
  return latest?.[name] === true
}

/**
 * One member of a flag block (`features.room_files.files_tab`). Off when the room has not been read,
 * the block is absent, or the member is not exactly `true`.
 */
export function spaceRoomNestedFeature(block: string, name: string): boolean {
  version()
  const value = latest?.[block]
  return value !== null && typeof value === 'object' && (value as Record<string, unknown>)[name] === true
}

/**
 * A flag with its explicit development override. `VITE_*` set to `true` or `false` wins, but
 * only in a dev build: a production bundle never reads it, so the BFF alone decides there.
 */
export function spaceRoomFeatureWithDevOverride(name: string, override: string | undefined): boolean {
  if (import.meta.env.DEV === true) {
    if (override === 'true') return true
    if (override === 'false') return false
  }
  return spaceRoomFeature(name)
}

/**
 * One flag read straight off a context object, for the flags the page passes down as props
 * (`room_details`, `room_archive`, `human_posts`). Absent features, or a key that is not exactly
 * `true`, mean off.
 */
export function contextFeatureOn(
  context: { readonly features?: SpaceRoomFeatures | null } | null | undefined,
  name: string,
): boolean {
  return context?.features?.[name] === true
}

// The context type in the API client does not carry the archive fields yet, so they are read through
// this shape off any context-like object. Every field is optional: an older gateway sends none.
type ArchiveShape = {
  readonly archived?: unknown
  readonly membership?: { readonly state?: unknown; readonly archived_at?: unknown } | null
}

/**
 * Whether the context says the room is archived. The context's own `archived` and Control's
 * membership `state` both count; either one saying so is enough, and neither being present (an
 * older gateway) means active.
 */
export function roomIsArchived(context: object | null | undefined): boolean {
  const shape = context as ArchiveShape | null | undefined
  return shape?.archived === true || shape?.membership?.state === 'archived'
}

/** When the room was archived, as the server's string, or `null` when it is not or none was sent. */
export function roomArchivedAt(context: object | null | undefined): string | null {
  const value = (context as ArchiveShape | null | undefined)?.membership?.archived_at
  return roomIsArchived(context) && typeof value === 'string' && value.trim() ? value.trim() : null
}
