import { createMemo, createSignal, untrack, type Accessor } from 'solid-js'
import { z } from 'zod'
import { readChatRunPanelCollapsed } from '@/features/chat/lib/chat-run-watch'
import { readClientJson, writeClientJson } from '@/shared/session/client-storage'
import { getSession } from '@/shared/session/session-store'

// Browser-only memory of where the browser dock sits beside a chat (WS9.G). It is presentation
// only: it never grants access, changes what Quarry returns and never reaches the server.
//
// The record holds exactly four fields. It never holds a page's URL or title, a browser session id
// or a run id: those belong to the session and the run, and a layout has no use for them. Writes are
// strict, so an extra field refuses the whole write. Reads are forgiving, field by field, and drop
// any extra field they find.
//
// Keyed by workspace and user, like the Home layout record, so one account's layout never shows for
// another account on the same device. Nothing is read or written until both ids are known.

export const BROWSER_DOCK_POSITIONS = ['right', 'bottom', 'maximised', 'hidden'] as const
export type BrowserDockPosition = (typeof BROWSER_DOCK_POSITIONS)[number]

/** A layout the dock opens in. `hidden` is the collapsed rail, not a layout to open. */
export type BrowserDockVisiblePosition = Exclude<BrowserDockPosition, 'hidden'>

export const BROWSER_DOCK_CHAT_PER_TAB = ['keep', 'new'] as const
export type BrowserDockChatPerTab = (typeof BROWSER_DOCK_CHAT_PER_TAB)[number]

/** The share of the page the bottom dock takes. The right layout keeps the rail's own width. */
export const BROWSER_DOCK_SIZE_MIN = 0.25
export const BROWSER_DOCK_SIZE_MAX = 0.7

const fields = {
  position: z.enum(BROWSER_DOCK_POSITIONS),
  size: z.number().min(BROWSER_DOCK_SIZE_MIN).max(BROWSER_DOCK_SIZE_MAX),
  chatPerTab: z.enum(BROWSER_DOCK_CHAT_PER_TAB),
  composerHidden: z.boolean(),
}

/** The stored record. Strict: it accepts these four fields and nothing else. */
export const browserDockSchema = z.object(fields).strict()
export type BrowserDockState = z.infer<typeof browserDockSchema>

/** A partial update. Strict as well, so a patch that carries anything else is refused whole. */
export const browserDockPatchSchema = z.object(fields).partial().strict()
export type BrowserDockPatch = z.infer<typeof browserDockPatchSchema>

export const BROWSER_DOCK_DEFAULTS: Readonly<BrowserDockState> = {
  position: 'right',
  size: 0.4,
  chatPerTab: 'keep',
  composerHidden: false,
}

/**
 * The storage key for one account in one workspace, or null while either id is unknown. Both ids are
 * encoded, so an id that contains a colon cannot collide with another account's key.
 */
export function browserDockStorageKey(userId: string | undefined, orgId: string | undefined): string | null {
  const user = userId?.trim()
  const org = orgId?.trim()
  return user && org ? `verevon.chat.browser-dock.v1:${encodeURIComponent(org)}:${encodeURIComponent(user)}` : null
}

/**
 * What a missing or unreadable record means. Before the dock existed, the chat's live panel only
 * remembered whether it was collapsed (`verevon.chat.runPanel.collapsed.v1`, read here and never
 * written). A collapsed rail maps to `hidden`; anything else to the default. Reading the old flag
 * never writes the new record, so a read has no side effect.
 */
function migratedDefaults(): BrowserDockState {
  return {
    ...BROWSER_DOCK_DEFAULTS,
    position: readChatRunPanelCollapsed() ? 'hidden' : BROWSER_DOCK_DEFAULTS.position,
  }
}

// Reading is forgiving field by field: one stale or hand-edited value falls back to the value a
// missing record would give it, without discarding the rest.
function forgivingSchemaFor(base: BrowserDockState) {
  return z.object({
    position: fields.position.catch(base.position),
    size: fields.size.catch(base.size),
    chatPerTab: fields.chatPerTab.catch(base.chatPerTab),
    composerHidden: fields.composerHidden.catch(base.composerHidden),
  })
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

/**
 * The layout stored under `key`. An unknown identity reads the default and touches no storage. A
 * missing or unreadable record reads as the migrated default. A field that cannot be read takes the
 * value a missing record would give it, and any field beyond the four is dropped.
 */
export function readBrowserDockState(key: string | null): BrowserDockState {
  if (!key) return { ...BROWSER_DOCK_DEFAULTS }
  const base = migratedDefaults()
  const stored = readClientJson(key, isRecord)
  if (!stored) return base
  const parsed = forgivingSchemaFor(base).safeParse(stored)
  return parsed.success ? parsed.data : base
}

/**
 * Stores a complete layout under `key` and returns what was stored. Refused (null) when there is no
 * identity, or when the state carries anything beyond the four fields or a value out of range. A
 * browser that refuses the write (full or blocked storage) still gets the state back, so the view
 * keeps what the person chose for this visit.
 */
export function writeBrowserDockState(key: string | null, state: BrowserDockState): BrowserDockState | null {
  if (!key) return null
  const checked = browserDockSchema.safeParse(state)
  if (!checked.success) return null
  writeClientJson(key, checked.data)
  return checked.data
}

/**
 * The remembered layout for the signed-in person and workspace, plus a partial updater. It is read as
 * the component is created. An update merges into what is stored for the identity current at that
 * moment, so a patch never overwrites a field another control wrote. With no identity the change
 * holds for this view only, and nothing is stored.
 */
export function useBrowserDock(): [state: Accessor<BrowserDockState>, update: (patch: BrowserDockPatch) => void] {
  const session = getSession()
  const key = createMemo(() => browserDockStorageKey(session.user?.id, session.activeOrg?.id))
  const [state, setState] = createSignal<BrowserDockState>(() => readBrowserDockState(key()))
  const update = (patch: BrowserDockPatch) => {
    const checked = browserDockPatchSchema.safeParse(patch)
    if (!checked.success) return
    const id = untrack(key)
    const merged = browserDockSchema.safeParse({ ...readBrowserDockState(id), ...checked.data })
    if (!merged.success) return
    const saved = writeBrowserDockState(id, merged.data) ?? merged.data
    setState(() => saved)
  }
  return [state, update]
}

/** The next layout in the order the cycle shortcut walks: right, bottom, maximised, hidden, right. */
export function nextBrowserDockPosition(position: BrowserDockPosition): BrowserDockPosition {
  const index = BROWSER_DOCK_POSITIONS.indexOf(position)
  return BROWSER_DOCK_POSITIONS[(index + 1) % BROWSER_DOCK_POSITIONS.length] ?? BROWSER_DOCK_DEFAULTS.position
}

/**
 * Hides an open dock, or opens a hidden one in `restore`: the layout it had before it was hidden, or
 * the right side when the caller does not know it.
 */
export function toggleBrowserDockPosition(
  position: BrowserDockPosition,
  restore: BrowserDockVisiblePosition = 'right',
): BrowserDockPosition {
  return position === 'hidden' ? restore : 'hidden'
}

/** The class the chat page takes for a layout, or null when the dock is hidden (the collapsed rail carries that). */
export function browserDockPageClass(position: BrowserDockPosition): string | null {
  return position === 'hidden' ? null : `verevon-chat-page--dock-${position}`
}

/** The `--browser-dock-size` value for the bottom layout, from the stored share of the page height. */
export function browserDockSizeCss(state: Pick<BrowserDockState, 'size'>): string {
  return `${Math.round(state.size * 100)}%`
}

/**
 * Whether the browser dock is on for this build. `VITE_CHAT_BROWSER_DOCK` counts only as "1" or
 * "true", so an unset variable is off: a build stays off until its environment turns the dock on.
 */
export function isChatBrowserDockEnabled(): boolean {
  const value = import.meta.env.VITE_CHAT_BROWSER_DOCK
  return value === '1' || value === 'true'
}
