export type SupportChatThreadScope = {
  userId: string
  orgId: string
  conversationId: string
}

const supportThreadBindings = new Map<string, string>()
const supportReadOnlyThreadIds = new Set<string>()
const SUPPORT_THREAD_BINDINGS_STORAGE_KEY = 'verevon.chat.supportThreadBindings.v1'
const SUPPORT_READ_ONLY_STORAGE_KEY = 'verevon.chat.supportReadOnlyThreads.v1'
let supportThreadBindingsHydrated = false

/** Returns a shared Chat thread only when it was created for this exact user,
 * organization, and conversation during the current authenticated browser
 * session. The binding is scoped by all three identities so a reload can
 * rehydrate the right answer without making it visible to another tenant or
 * conversation. */
export function readSupportChatThread(scope: SupportChatThreadScope): string | null {
  hydrateSupportThreadBindings()
  return supportThreadBindings.get(scopeKey(scope)) ?? null
}

export function bindSupportChatThread(scope: SupportChatThreadScope, threadId: string): void {
  const normalizedThreadId = threadId.trim()
  if (!normalizedThreadId) return
  hydrateSupportThreadBindings()
  supportThreadBindings.set(scopeKey(scope), normalizedThreadId)
  persistSupportThreadBindings()
  markSupportThreadReadOnly(normalizedThreadId)
}

export function clearSupportChatThreads(): void {
  supportThreadBindings.clear()
  supportReadOnlyThreadIds.clear()
  supportThreadBindingsHydrated = false
  try {
    if (typeof sessionStorage !== 'undefined') {
      sessionStorage.removeItem(SUPPORT_THREAD_BINDINGS_STORAGE_KEY)
      sessionStorage.removeItem(SUPPORT_READ_ONLY_STORAGE_KEY)
    }
  } catch {
    // The in-memory authority was already cleared; storage may be unavailable.
  }
}

/** Support-derived threads contain customer-authored transcript history. They
 * stay read-only for the authenticated browser session, including reloads. */
export function isSupportChatThread(threadId: string | undefined): boolean {
  const normalizedThreadId = threadId?.trim()
  if (!normalizedThreadId) return false
  if (normalizedThreadId.startsWith('support_')) return true
  hydrateReadOnlyThreadIds()
  return supportReadOnlyThreadIds.has(normalizedThreadId)
}

export function newSupportChatThreadId(): string {
  if (typeof globalThis.crypto?.randomUUID === 'function') {
    return `support_${globalThis.crypto.randomUUID()}`
  }
  const bytes = new Uint8Array(16)
  if (typeof globalThis.crypto?.getRandomValues === 'function') globalThis.crypto.getRandomValues(bytes)
  else for (let index = 0; index < bytes.length; index += 1) bytes[index] = Math.floor(Math.random() * 256)
  bytes[6] = (bytes[6]! & 0x0f) | 0x40
  bytes[8] = (bytes[8]! & 0x3f) | 0x80
  const hex = [...bytes].map((value) => value.toString(16).padStart(2, '0')).join('')
  return `support_${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}

function scopeKey(scope: SupportChatThreadScope): string {
  return `${normalizePart(scope.userId)}:${normalizePart(scope.orgId)}:${normalizePart(scope.conversationId)}`
}

function normalizePart(value: string): string {
  return value.trim().replaceAll(':', '%3A')
}

function markSupportThreadReadOnly(threadId: string): void {
  supportReadOnlyThreadIds.add(threadId)
  try {
    if (typeof sessionStorage !== 'undefined') {
      sessionStorage.setItem(SUPPORT_READ_ONLY_STORAGE_KEY, JSON.stringify([...supportReadOnlyThreadIds]))
    }
  } catch {
    // Keep the in-memory restriction even when browser storage is unavailable.
  }
}

function persistSupportThreadBindings(): void {
  try {
    if (typeof sessionStorage === 'undefined') return
    const bindings = Object.fromEntries([...supportThreadBindings].slice(-80))
    sessionStorage.setItem(SUPPORT_THREAD_BINDINGS_STORAGE_KEY, JSON.stringify(bindings))
  } catch {
    // The in-memory binding remains authoritative when browser storage is unavailable.
  }
}

function hydrateSupportThreadBindings(): void {
  if (supportThreadBindingsHydrated) return
  supportThreadBindingsHydrated = true
  try {
    if (typeof sessionStorage === 'undefined') return
    const stored: unknown = JSON.parse(sessionStorage.getItem(SUPPORT_THREAD_BINDINGS_STORAGE_KEY) ?? '{}')
    if (!stored || typeof stored !== 'object' || Array.isArray(stored)) return
    for (const [key, threadId] of Object.entries(stored)) {
      if (key.trim() && typeof threadId === 'string' && threadId.trim()) {
        supportThreadBindings.set(key, threadId.trim())
      }
    }
  } catch {
    try {
      sessionStorage.removeItem(SUPPORT_THREAD_BINDINGS_STORAGE_KEY)
    } catch {
      // Storage is unavailable; the in-memory map remains authoritative.
    }
  }
}

function hydrateReadOnlyThreadIds(): void {
  if (supportReadOnlyThreadIds.size > 0 || typeof sessionStorage === 'undefined') return
  try {
    const stored: unknown = JSON.parse(sessionStorage.getItem(SUPPORT_READ_ONLY_STORAGE_KEY) ?? '[]')
    if (!Array.isArray(stored)) return
    for (const threadId of stored) {
      if (typeof threadId === 'string' && threadId.trim()) supportReadOnlyThreadIds.add(threadId.trim())
    }
  } catch {
    try {
      sessionStorage.removeItem(SUPPORT_READ_ONLY_STORAGE_KEY)
    } catch {
      // Storage is unavailable; the in-memory set remains authoritative.
    }
  }
}
