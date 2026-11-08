/**
 * Palette recents (WS13-S6, D9, D14).
 *
 * Kept in memory always. Written to `localStorage` (`verevon.palette.recent.v1`)
 * only while the workspace allows a local copy, and removed the moment the
 * server's retention verdict turns it off (ZDR). The stored entry is a pick
 * (kind, id, label, href), never a typed query.
 */

import { isLocalRetentionAllowed, subscribeLocalRetention } from '@/features/chat/lib/chat-retention'

export const PALETTE_RECENT_KEY = 'verevon.palette.recent.v1'
const MAX_RECENTS = 8

export interface PaletteRecent {
  readonly kind: 'room' | 'thread' | 'agent' | 'person' | 'chat' | 'knowledge'
  readonly id: string
  readonly label: string
  readonly href: string
}

let memory: PaletteRecent[] = []
let loaded = false
let unsubscribe: (() => void) | undefined

function isRecent(value: unknown): value is PaletteRecent {
  if (!value || typeof value !== 'object') return false
  const v = value as Record<string, unknown>
  return typeof v.kind === 'string' && typeof v.id === 'string' && typeof v.label === 'string' && typeof v.href === 'string'
}

function removeStored(): void {
  try {
    window.localStorage.removeItem(PALETTE_RECENT_KEY)
  } catch {
    /* storage unavailable: nothing was kept */
  }
}

function ensureLoaded(): void {
  if (!unsubscribe) {
    unsubscribe = subscribeLocalRetention((allowed) => {
      if (!allowed) {
        memory = []
        removeStored()
      }
    })
  }
  if (loaded) return
  loaded = true
  if (!isLocalRetentionAllowed()) {
    removeStored()
    return
  }
  try {
    const raw = window.localStorage.getItem(PALETTE_RECENT_KEY)
    const parsed: unknown = raw ? JSON.parse(raw) : []
    if (Array.isArray(parsed)) memory = parsed.filter(isRecent).slice(0, MAX_RECENTS)
  } catch {
    memory = []
  }
}

export function readPaletteRecents(): readonly PaletteRecent[] {
  ensureLoaded()
  return memory
}

export function recordPaletteRecent(entry: PaletteRecent): void {
  ensureLoaded()
  memory = [entry, ...memory.filter((item) => !(item.kind === entry.kind && item.id === entry.id))].slice(0, MAX_RECENTS)
  if (!isLocalRetentionAllowed()) return
  try {
    window.localStorage.setItem(PALETTE_RECENT_KEY, JSON.stringify(memory))
  } catch {
    /* quota or blocked storage: recents stay in memory only */
  }
}

export function __resetPaletteRecentsForTests(): void {
  memory = []
  loaded = false
  unsubscribe?.()
  unsubscribe = undefined
}
