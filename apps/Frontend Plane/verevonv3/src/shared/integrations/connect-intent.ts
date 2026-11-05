/**
 * The connect intent of a connector suggestion (EP-10 S9): the person's own question, kept in this tab's
 * sessionStorage while they connect an account in a popup, so a reload during the sign-in does not lose the
 * place. It is a browser convenience and never goes to a server (EP-10 D7). Every access is guarded: a blocked,
 * full or unreadable store degrades to a plain "connected" state, never to an error.
 */

/** A saved connect expires after 15 minutes: an older question may no longer be the one being asked. */
export const CONNECT_INTENT_TTL_MS = 15 * 60 * 1000

/** How long a connect waits for its new connection to appear in the connection list. */
export const CONNECT_INTENT_WAIT_MS = 30_000

/** Pause between two reads of the connection list while the wait runs. */
const CONNECT_POLL_INTERVAL_MS = 2_000

const KEY_PREFIX = 'verevon-connect-intent:'

export interface ConnectIntent {
  threadId: string
  suggestionId: string
  provider: string
  /** The question the suggestion answers, resent as it was asked. */
  prompt: string
  /** Epoch milliseconds when the connect started. */
  createdAt: number
}

/** The fields of an integration connection that the connect flow reads. */
export interface ConnectionRef {
  id: string
  providerKey?: string
  status: string
}

/**
 * How a connect started from a suggestion card ended. `pending` means connected or waiting but not resumed: the
 * saved connect stays, so "Finish connecting" can resume it after a reload.
 */
export type ConnectIntentOutcome =
  | { status: 'resumed'; connectionId: string }
  | { status: 'pending' }
  | { status: 'failed'; error: unknown }

/** How a saved connect was continued on a click. `not_connected` keeps the record for another try. */
export type SavedConnectOutcome = 'resumed' | 'pending' | 'not_connected' | 'none'

function store(): Storage | null {
  try {
    return window.sessionStorage
  } catch {
    // Reading the property throws where storage is blocked (sandboxed frames, some privacy modes).
    return null
  }
}

function keyFor(threadId: string): string {
  return `${KEY_PREFIX}${threadId}`
}

function isConnectIntent(value: unknown, threadId: string): value is ConnectIntent {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return record.threadId === threadId
    && typeof record.suggestionId === 'string'
    && typeof record.provider === 'string'
    && typeof record.prompt === 'string'
    && typeof record.createdAt === 'number'
    && Number.isFinite(record.createdAt)
}

// A record dated after `now` (the clock moved back) cannot be placed in time, so it is dropped like an expired one.
function isWithinTtl(createdAt: number, now: number): boolean {
  const age = now - createdAt
  return age >= 0 && age < CONNECT_INTENT_TTL_MS
}

function readRaw(threadId: string): string | null {
  try {
    return store()?.getItem(keyFor(threadId)) ?? null
  } catch {
    return null
  }
}

function parseRecord(raw: string, threadId: string): ConnectIntent | null {
  try {
    const value: unknown = JSON.parse(raw)
    return isConnectIntent(value, threadId) ? value : null
  } catch {
    return null
  }
}

/** Saves the thread's intent. One per thread: a new connect replaces the earlier one. */
export function saveIntent(intent: ConnectIntent): void {
  try {
    store()?.setItem(keyFor(intent.threadId), JSON.stringify(intent))
  } catch {
    // A full or blocked store loses only the reload safety net; the connect itself still runs.
  }
}

/** The thread's intent while it is unexpired. An expired or unreadable record is removed and reads as absent. */
export function readIntent(threadId: string, now: number = Date.now()): ConnectIntent | null {
  const raw = readRaw(threadId)
  if (raw === null) return null
  const intent = parseRecord(raw, threadId)
  if (intent && isWithinTtl(intent.createdAt, now)) return intent
  clearIntent(threadId)
  return null
}

/** Removes the thread's intent. */
export function clearIntent(threadId: string): void {
  try {
    store()?.removeItem(keyFor(threadId))
  } catch {
    // Nothing to remove where storage is blocked.
  }
}

/** Reads and removes the thread's intent in one step, so only the first caller gets it. */
export function takeIntent(threadId: string, now: number = Date.now()): ConnectIntent | null {
  const intent = readIntent(threadId, now)
  clearIntent(threadId)
  return intent
}

/**
 * The id of the first active connection of `provider` that is not in `knownIds`, or null. Only an active
 * connection counts, as it does in the connector picker.
 */
export function findNewConnection(
  connections: readonly ConnectionRef[],
  provider: string,
  knownIds: ReadonlySet<string>,
): string | null {
  const match = connections.find((connection) => (
    connection.providerKey === provider && connection.status === 'active' && !knownIds.has(connection.id)
  ))
  return match?.id ?? null
}

const defaultSleep = (ms: number): Promise<void> => new Promise<void>((resolve) => {
  setTimeout(resolve, ms)
})

/**
 * Polls the connection list until a new connection of `provider` appears, or returns null when the wait runs out.
 * A failed read is retried until the deadline: the sign-in can still complete while the list is unreachable.
 */
export async function waitForNewConnection(input: {
  provider: string
  knownIds: ReadonlySet<string>
  listConnections: () => Promise<readonly ConnectionRef[]>
  timeoutMs?: number
  intervalMs?: number
  now?: () => number
  sleep?: (ms: number) => Promise<void>
}): Promise<string | null> {
  const now = input.now ?? Date.now
  const sleep = input.sleep ?? defaultSleep
  const intervalMs = input.intervalMs ?? CONNECT_POLL_INTERVAL_MS
  const deadline = now() + (input.timeoutMs ?? CONNECT_INTENT_WAIT_MS)
  for (;;) {
    try {
      const connectionId = findNewConnection(await input.listConnections(), input.provider, input.knownIds)
      if (connectionId !== null) return connectionId
    } catch {
      // Retried below until the deadline.
    }
    const remaining = deadline - now()
    if (remaining <= 0) return null
    await sleep(Math.min(intervalMs, remaining))
  }
}

/**
 * Hands the intent to `resume`, and saves it again when the resend is refused or throws, so a refused question is
 * still there to finish. The caller has removed the record before the call; that is what keeps a second resume from
 * finding it.
 */
function resumeWithRestore(
  intent: ConnectIntent,
  connectionId: string,
  resume: (intent: ConnectIntent, connectionId: string) => boolean,
): boolean {
  let resumed = false
  try {
    resumed = resume(intent, connectionId)
  } finally {
    if (!resumed) saveIntent(intent)
  }
  return resumed
}

/**
 * Runs a connect started from a suggestion card. The intent is saved before the sign-in opens, so a reload while the
 * person is in the popup keeps it. When the new connection appears, the saved question is resent on it once. A failed
 * sign-in clears the intent. A connection that does not appear in time, or a resend that is refused, keeps the intent
 * for "Finish connecting".
 */
export async function runConnectIntent(input: {
  threadId: string
  suggestionId: string
  provider: string
  prompt: string
  /** The connections the card saw before the sign-in. Only a connection outside this set resumes the question. */
  knownIds: ReadonlySet<string>
  /** Opens the provider sign-in and resolves when it completes. Rejects with the provider's message on failure. */
  openConnect: () => Promise<void>
  listConnections: () => Promise<readonly ConnectionRef[]>
  /** Resends the question on the connection. Returns false when that cannot happen now. */
  resume: (intent: ConnectIntent, connectionId: string) => boolean
  now?: () => number
  sleep?: (ms: number) => Promise<void>
  timeoutMs?: number
  intervalMs?: number
}): Promise<ConnectIntentOutcome> {
  const now = input.now ?? Date.now
  const intent: ConnectIntent = {
    threadId: input.threadId,
    suggestionId: input.suggestionId,
    provider: input.provider,
    prompt: input.prompt,
    createdAt: now(),
  }
  saveIntent(intent)
  try {
    await input.openConnect()
  } catch (error) {
    clearIntent(intent.threadId)
    return { status: 'failed', error }
  }
  const connectionId = await waitForNewConnection({
    provider: intent.provider,
    knownIds: input.knownIds,
    listConnections: input.listConnections,
    timeoutMs: input.timeoutMs,
    intervalMs: input.intervalMs,
    now,
    sleep: input.sleep,
  })
  if (connectionId === null) return { status: 'pending' }
  // This connect owns its question, so its record goes before the resend and a stale copy cannot resume it again.
  clearIntent(intent.threadId)
  return resumeWithRestore(intent, connectionId, input.resume)
    ? { status: 'resumed', connectionId }
    : { status: 'pending' }
}

/**
 * Continues a saved connect after a reload, on the person's "Continue" click: resumes the question once its connection
 * exists. Nothing is resent on load. `not_connected` keeps the record, so the person can connect and continue again.
 */
export async function continueSavedConnectIntent(input: {
  threadId: string
  listConnections: () => Promise<readonly ConnectionRef[]>
  resume: (intent: ConnectIntent, connectionId: string) => boolean
  now?: () => number
}): Promise<SavedConnectOutcome> {
  const now = input.now ?? Date.now
  const saved = readIntent(input.threadId, now())
  if (!saved) return 'none'
  const connectionId = findNewConnection(await input.listConnections(), saved.provider, new Set<string>())
  if (connectionId === null) return 'not_connected'
  // Taken, not only read, before the resend: a second click that lands while the list was being read finds nothing.
  const intent = takeIntent(input.threadId, now())
  if (!intent) return 'none'
  return resumeWithRestore(intent, connectionId, input.resume) ? 'resumed' : 'pending'
}
