import { createMemo, createSignal, untrack, type Accessor } from 'solid-js'
import { z } from 'zod'
import { homeUpdatesFeedSchema } from '@/shared/actions/home-updates'
import { newsCategoryOptions } from '@/shared/api/information-client'
import { readClientJson, writeClientJson } from '@/shared/session/client-storage'
import { getSession } from '@/shared/session/session-store'

// Browser-only memory of how this person left Home: the chat dock, the updates
// card and the recent-work filter. It is presentation only: it never grants
// access, changes what a feed returns or reaches the server. The active tab is
// not here; it lives in the URL (?tab=).
//
// The record is keyed by workspace and user, like the chat organizer. Sign-out
// leaves it in place, as it does for the other per-account keys, because the
// identity in the key is the isolation: the next account computes a different
// key and never sees this one. Nothing is read or written until both ids are
// known, and a write only ever goes to the key of the identity that is current
// at that moment.
const fields = {
  dockMinimized: z.boolean(),
  dockExpanded: z.boolean(),
  feed: homeUpdatesFeedSchema,
  // The value goes straight into the news query, so it must be a known topic.
  category: z.string().refine((value) => newsCategoryOptions.some((option) => option.value === value)),
  pinnedOnly: z.boolean(),
}

export type HomeLayoutState = { [K in keyof typeof fields]: z.infer<(typeof fields)[K]> }
export type HomeLayoutPatch = Partial<HomeLayoutState>

export const HOME_LAYOUT_DEFAULTS: Readonly<HomeLayoutState> = {
  dockMinimized: false,
  dockExpanded: false,
  feed: 'news',
  category: 'all',
  pinnedOnly: false,
}

// Reading is forgiving field by field: one stale or hand-edited value falls
// back to its default without discarding the rest.
export const homeLayoutSchema = z.object({
  dockMinimized: fields.dockMinimized.catch(HOME_LAYOUT_DEFAULTS.dockMinimized),
  dockExpanded: fields.dockExpanded.catch(HOME_LAYOUT_DEFAULTS.dockExpanded),
  feed: fields.feed.catch(HOME_LAYOUT_DEFAULTS.feed),
  category: fields.category.catch(HOME_LAYOUT_DEFAULTS.category),
  pinnedOnly: fields.pinnedOnly.catch(HOME_LAYOUT_DEFAULTS.pinnedOnly),
})
// Writing is strict: a patch with any invalid field is refused as a whole.
export const homeLayoutPatchSchema = z.object(fields).partial()

export function homeLayoutStorageKey(userId: string | undefined, orgId: string | undefined): string | null {
  const user = userId?.trim()
  const org = orgId?.trim()
  return user && org ? `verevon.home-layout.v1:${encodeURIComponent(org)}:${encodeURIComponent(user)}` : null
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

export function readHomeLayout(key: string | null): HomeLayoutState {
  if (!key) return { ...HOME_LAYOUT_DEFAULTS }
  const parsed = homeLayoutSchema.safeParse(readClientJson(key, isRecord))
  return parsed.success ? parsed.data : { ...HOME_LAYOUT_DEFAULTS }
}

/** Merges a partial update into what is stored under `key`, so components that
 * each own a few fields never overwrite one another. Returns the saved state. */
export function writeHomeLayout(key: string | null, patch: HomeLayoutPatch): HomeLayoutState {
  const checked = homeLayoutPatchSchema.safeParse(patch)
  if (!key || !checked.success) return readHomeLayout(key)
  const next = { ...readHomeLayout(key), ...checked.data }
  writeClientJson(key, next)
  return next
}

/** The remembered layout for the signed-in person and workspace, plus a partial
 * updater. The value is read as the component is created, so a known identity
 * paints its restored state on the first frame; when the person or workspace
 * changes it is read again for the new identity and the previous one is left
 * untouched. */
export function useHomeLayout(): [state: Accessor<HomeLayoutState>, update: (patch: HomeLayoutPatch) => void] {
  const session = getSession()
  const key = createMemo(() => homeLayoutStorageKey(session.user?.id, session.activeOrg?.id))
  const [state, setState] = createSignal<HomeLayoutState>(() => readHomeLayout(key()))
  const update = (patch: HomeLayoutPatch) => {
    const checked = homeLayoutPatchSchema.safeParse(patch)
    if (!checked.success) return
    setState((current) => ({ ...current, ...checked.data }))
    writeHomeLayout(untrack(key), checked.data)
  }
  return [state, update]
}
