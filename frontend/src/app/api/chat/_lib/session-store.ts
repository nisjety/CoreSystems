import { randomUUID } from 'crypto'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'fs'
import { tmpdir } from 'os'
import path from 'path'

export interface ChatMessageMetadata {
  source: 'reasoning-plane'
  citations?: string[]
}

export interface StoredChatMessage {
  id: string
  content: string
  role: 'user' | 'assistant' | 'system'
  timestamp: string
  sessionId: string
  isThinking?: boolean
  error?: string
  metadata?: ChatMessageMetadata
}

export interface StoredChatSession {
  id: string
  title: string
  messages: StoredChatMessage[]
  createdAt: string
  updatedAt: string
  userId?: string
}

type SessionStore = Map<string, StoredChatSession>

const CHAT_SESSION_STORE_PATH =
  process.env.AQUATIQ_CHAT_SESSION_STORE_PATH ||
  path.join(tmpdir(), 'aquatiq-frontend-chat-sessions.json')

function toSessionListItem(session: StoredChatSession) {
  const { messages, ...rest } = session

  return {
    ...rest,
    messageCount: messages.length,
  }
}

function readSessionStore(): SessionStore {
  if (!existsSync(CHAT_SESSION_STORE_PATH)) {
    return new Map<string, StoredChatSession>()
  }

  try {
    const raw = readFileSync(CHAT_SESSION_STORE_PATH, 'utf8')
    if (!raw.trim()) {
      return new Map<string, StoredChatSession>()
    }

    const parsed = JSON.parse(raw) as Record<string, StoredChatSession>
    return new Map<string, StoredChatSession>(Object.entries(parsed))
  } catch {
    return new Map<string, StoredChatSession>()
  }
}

function writeSessionStore(store: SessionStore) {
  mkdirSync(path.dirname(CHAT_SESSION_STORE_PATH), { recursive: true })
  writeFileSync(
    CHAT_SESSION_STORE_PATH,
    JSON.stringify(Object.fromEntries(store), null, 2),
    'utf8',
  )
}

function removeEmptySessions(store: SessionStore) {
  let didChange = false

  for (const [sessionId, session] of store.entries()) {
    if (session.messages.length === 0) {
      store.delete(sessionId)
      didChange = true
    }
  }

  if (didChange) {
    writeSessionStore(store)
  }
}

export function buildChatTitle(content: string): string {
  const normalized = content.trim().replace(/\s+/g, ' ')

  if (!normalized) {
    return 'Ny samtale'
  }

  return normalized.length > 60 ? `${normalized.slice(0, 57)}...` : normalized
}

export function listChatSessions() {
  const sessionStore = readSessionStore()
  removeEmptySessions(sessionStore)

  return Array.from(sessionStore.values())
    .sort((left, right) => right.updatedAt.localeCompare(left.updatedAt))
    .map((session) => toSessionListItem(session))
}

export function getChatSession(sessionId: string): StoredChatSession | null {
  const sessionStore = readSessionStore()
  return sessionStore.get(sessionId) ?? null
}

export function createChatSession(title = 'Ny samtale', userId?: string): StoredChatSession {
  const sessionStore = readSessionStore()
  const now = new Date().toISOString()
  const session: StoredChatSession = {
    id: randomUUID(),
    title,
    messages: [],
    createdAt: now,
    updatedAt: now,
    userId,
  }

  sessionStore.set(session.id, session)
  writeSessionStore(sessionStore)

  return session
}

export function ensureChatSession(options: {
  sessionId?: string
  title?: string
  userId?: string
}): StoredChatSession {
  const sessionStore = readSessionStore()
  const { sessionId, title, userId } = options

  if (!sessionId) {
    const now = new Date().toISOString()
    const session: StoredChatSession = {
      id: randomUUID(),
      title: title ?? 'Ny samtale',
      messages: [],
      createdAt: now,
      updatedAt: now,
      userId,
    }

    sessionStore.set(session.id, session)
    writeSessionStore(sessionStore)

    return session
  }

  const existing = sessionStore.get(sessionId)
  if (existing) {
    return existing
  }

  const now = new Date().toISOString()
  const session: StoredChatSession = {
    id: sessionId,
    title: title ?? 'Ny samtale',
    messages: [],
    createdAt: now,
    updatedAt: now,
    userId,
  }

  sessionStore.set(sessionId, session)
  writeSessionStore(sessionStore)

  return session
}

export function updateChatSessionTitle(sessionId: string, title: string): StoredChatSession {
  const sessionStore = readSessionStore()
  const existing = sessionStore.get(sessionId) ?? ensureChatSession({ sessionId, title })
  const nextSession: StoredChatSession = {
    ...existing,
    title,
    updatedAt: new Date().toISOString(),
  }

  sessionStore.set(sessionId, nextSession)
  writeSessionStore(sessionStore)

  return nextSession
}

export function appendMessagesToSession(options: {
  sessionId: string
  title?: string
  userId?: string
  messages: StoredChatMessage[]
}): StoredChatSession {
  const sessionStore = readSessionStore()
  const existing = sessionStore.get(options.sessionId) ?? ensureChatSession({
    sessionId: options.sessionId,
    title: options.title,
    userId: options.userId,
  })

  const nextSession: StoredChatSession = {
    ...existing,
    title: existing.title || options.title || 'Ny samtale',
    userId: existing.userId ?? options.userId,
    updatedAt: new Date().toISOString(),
    messages: [...existing.messages, ...options.messages],
  }

  sessionStore.set(options.sessionId, nextSession)
  writeSessionStore(sessionStore)

  return nextSession
}

export function deleteChatSession(sessionId: string): boolean {
  const sessionStore = readSessionStore()
  const didDelete = sessionStore.delete(sessionId)
  writeSessionStore(sessionStore)

  return didDelete
}
