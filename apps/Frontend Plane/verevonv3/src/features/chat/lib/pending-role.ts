import { readSessionValue, removeSessionValue, writeSessionValue } from '@/shared/session/client-storage'

// The role a person stated on the personal first-run card before the chat had a thread (EP-8-S8, D3-356).
// memory.remember needs a thread, and a thread starts on the first send, so the role waits for that send:
// the chat controller saves it once as a memory and clears it. One role is held at a time, for 15 minutes.
// A zero-retention or temporary send discards it. It lives in memory and in tab-lifetime session storage, so
// a reload inside the window keeps it. It never goes to localStorage.
const pendingRoleKey = 'verevon.personalSetup.pendingRole'
const PENDING_ROLE_TTL_MS = 15 * 60 * 1000
const MAX_PENDING_ROLE_CHARS = 200

type PendingRole = { role: string; savedAt: number }

let heldRole: PendingRole | null = null

/** Holds the role for the next first send of a personal chat. It replaces any role already held. */
export function setPendingRole(role: string, now: number = Date.now()): void {
  const text = role.trim().slice(0, MAX_PENDING_ROLE_CHARS)
  if (!text) {
    discardPendingRole()
    return
  }
  heldRole = { role: text, savedAt: now }
  writeSessionValue(pendingRoleKey, JSON.stringify(heldRole))
}

/**
 * Takes the held role, once. It returns the role while it is inside its 15-minute window, and clears it
 * either way, so a second take finds nothing.
 */
export function takePendingRole(now: number = Date.now()): string | null {
  const held = heldRole ?? readStoredRole()
  discardPendingRole()
  return held && isInWindow(held, now) ? held.role : null
}

/** Drops the held role from memory and from session storage. */
export function discardPendingRole(): void {
  heldRole = null
  removeSessionValue(pendingRoleKey)
}

function isInWindow(entry: PendingRole, now: number): boolean {
  const age = now - entry.savedAt
  return age >= 0 && age < PENDING_ROLE_TTL_MS
}

function readStoredRole(): PendingRole | null {
  try {
    const parsed: unknown = JSON.parse(readSessionValue(pendingRoleKey) ?? 'null')
    return isPendingRole(parsed) ? parsed : null
  } catch {
    return null
  }
}

function isPendingRole(value: unknown): value is PendingRole {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return typeof record.role === 'string'
    && record.role.trim() !== ''
    && typeof record.savedAt === 'number'
    && Number.isFinite(record.savedAt)
}
