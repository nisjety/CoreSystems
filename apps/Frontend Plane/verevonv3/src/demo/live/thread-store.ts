/**
 * The demo's conversations, in memory. A page load starts from the seed (the
 * recent conversations on the dashboard) and every turn the visitor runs is
 * added here. Nothing is written to browser storage: the frame is opaque, and
 * the state is meant to vanish with the page, as on a fresh visit.
 */
import type { ThreadRow } from './shared-types'

export type StoredMessage = {
  id: string
  role: 'user' | 'assistant'
  content: string
  /** ISO 8601. */
  created_at: string
  model_used?: string
}

export type StoredThread = {
  threadId: string
  title: string
  preview: string
  /** ISO 8601. */
  updatedAt: string
  pinned: boolean
  personaEnabled: boolean
  messages: StoredMessage[]
}

export type ThreadPatch = {
  title?: string
  preview?: string
  pinned?: boolean
  updatedAt?: string
}

export type ThreadStore = {
  /** The listing the product reads: rows, newest first. */
  sessions(): ThreadRow[]
  get(threadId: string): StoredThread | undefined
  /** The thread, created with `title` when it is new. */
  ensure(threadId: string, input: { title: string; now: string }): StoredThread
  append(threadId: string, message: StoredMessage): void
  /** Adds `text` to the end of one stored message. */
  extend(threadId: string, messageId: string, text: string): void
  patch(threadId: string, changes: ThreadPatch): StoredThread | undefined
  remove(threadId: string): boolean
  /** Removes every thread, the seeded ones too. */
  clear(): void
  /** Back to the seed, as on a fresh visit. */
  reset(): void
}

function fromRow(row: ThreadRow, messages: readonly StoredMessage[]): StoredThread {
  return {
    threadId: row.threadId,
    title: row.title,
    preview: row.preview,
    updatedAt: row.updatedAt,
    pinned: row.pinned,
    personaEnabled: row.personaEnabled,
    messages: messages.map((message) => ({ ...message })),
  }
}

function toRow(thread: StoredThread): ThreadRow {
  return {
    threadId: thread.threadId,
    title: thread.title,
    preview: thread.preview,
    updatedAt: thread.updatedAt,
    pinned: thread.pinned,
    personaEnabled: thread.personaEnabled,
  }
}

/**
 * A store that starts from `seed`. `transcripts` gives a seeded thread its
 * messages (by thread id): a seeded conversation opens with what it said.
 */
export function createThreadStore(
  seed: readonly ThreadRow[],
  transcripts: Readonly<Record<string, readonly StoredMessage[]>> = {},
): ThreadStore {
  let threads = new Map<string, StoredThread>()

  const loadSeed = () => {
    threads = new Map(seed.map((row) => [row.threadId, fromRow(row, transcripts[row.threadId] ?? [])]))
  }
  loadSeed()

  const lookup = (threadId: string): StoredThread | undefined => threads.get(threadId)

  return {
    sessions() {
      return [...threads.values()]
        .map(toRow)
        .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))
    },
    get: lookup,
    ensure(threadId, input) {
      const existing = threads.get(threadId)
      if (existing) return existing
      const created: StoredThread = {
        threadId,
        title: input.title,
        preview: '',
        updatedAt: input.now,
        pinned: false,
        personaEnabled: true,
        messages: [],
      }
      threads.set(threadId, created)
      return created
    },
    append(threadId, message) {
      lookup(threadId)?.messages.push(message)
    },
    extend(threadId, messageId, text) {
      const message = lookup(threadId)?.messages.find((candidate) => candidate.id === messageId)
      if (message) message.content += text
    },
    patch(threadId, changes) {
      const thread = lookup(threadId)
      if (!thread) return undefined
      if (changes.title !== undefined && changes.title.trim()) thread.title = changes.title.trim()
      if (changes.preview !== undefined) thread.preview = changes.preview
      if (changes.pinned !== undefined) thread.pinned = changes.pinned
      if (changes.updatedAt !== undefined && !Number.isNaN(Date.parse(changes.updatedAt))) {
        thread.updatedAt = changes.updatedAt
      }
      return thread
    },
    remove(threadId) {
      return threads.delete(threadId)
    },
    clear() {
      threads = new Map()
    },
    reset() {
      loadSeed()
    },
  }
}
