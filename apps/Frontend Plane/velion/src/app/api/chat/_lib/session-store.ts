import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'fs'
import { tmpdir } from 'os'
import path from 'path'

import { getServerSession } from '@/components/auth/lib/auth-server'
import {
  convexMutation,
  convexQuery,
  CONVEX_INTERNAL_SERVICE_KEY,
} from '@/app/api/_lib/convex-client'

const INTERNAL_API_KEY =
  process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET

export interface ChatMessageMetadata {
  source: 'reasoning-plane'
  citations?: string[]
}

export interface StoredChatMessage {
  id: string
  clientId?: string
  content: string
  role: 'user' | 'assistant' | 'system'
  timestamp: string
  sessionId: string
  isThinking?: boolean
  error?: string
  metadata?: ChatMessageMetadata
}

interface StoredChatSession {
  id: string
  title: string
  messages: StoredChatMessage[]
  createdAt: string
  updatedAt: string
  userId?: string
  orgId?: string
}

type SessionListItem = Omit<StoredChatSession, 'messages'> & {
  messageCount: number
}

type ConvexOrganization = {
  _id: string
  externalOrgId: string
  name: string
  slug: string
  settings?: {
    defaultModel?: string
    maxTokens?: number
    allowedModels?: string[]
  }
}

type ConvexUser = {
  _id: string
  externalAuthId: string
  email: string
  name?: string
  orgId: string
  role: 'admin' | 'member' | 'viewer'
  lastSeenAt?: number
  lastSyncedAt?: number
  createdAt?: number
  syncStatus?: string
}

type ConvexMessage = {
  _id: string
  conversationId: string
  role: 'user' | 'assistant' | 'system'
  content: string
  clientId?: string
  userId?: string
  isStreaming?: boolean
  metadata?: {
    source?: string
    citations?: string[]
    model?: string
    tokens?: number
    latencyMs?: number
    error?: string
  }
  createdAt: number
  updatedAt: number
}

type ConvexConversation = {
  _id: string
  orgId: string
  userId: string
  title: string
  status: 'active' | 'archived' | 'deleted'
  createdAt: number
  updatedAt: number
  lastMessageAt?: number
  messageCount?: number
  messages?: ConvexMessage[]
}

type UserSessionContext = {
  userId: string
  orgId?: string
  role?: string
  onboardingStatus?: string
}

type OrganizationSummary = {
  id: string
  name: string
  slug?: string
}

type OrganizationDetails = OrganizationSummary & {
  created_at?: string
  createdAt?: string
}

type LegacySessionStore = Record<
  string,
  {
    id: string
    title: string
    messages: StoredChatMessage[]
    createdAt: string
    updatedAt: string
    userId?: string
  }
>

type MigrationState = Record<string, true>

interface ChatActorContext {
  userId: string
  userName: string
  userEmail: string
  orgId: string
  role: 'admin' | 'member' | 'viewer'
  convexOrgId: string
  convexUserId: string
}

export class ChatStoreError extends Error {
  statusCode: number

  constructor(message: string, statusCode = 500) {
    super(message)
    this.name = 'ChatStoreError'
    this.statusCode = statusCode
  }
}

const USER_SERVICE_URL = process.env.USER_SERVICE_URL || 'http://localhost:3012'
const ORG_SERVICE_URL = process.env.ORG_SERVICE_URL || 'http://localhost:8080'

const LEGACY_CHAT_SESSION_STORE_PATH =
  process.env.AQUATIQ_CHAT_SESSION_STORE_PATH ||
  path.join(tmpdir(), 'aquatiq-frontend-chat-sessions.json')

const LEGACY_MIGRATION_STATE_PATH = path.join(
  tmpdir(),
  'aquatiq-convex-chat-migration-state.json',
)

function slugify(value: string) {
  const normalized = value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')

  return normalized || `org-${Date.now()}`
}

function toConversationTimestamp(value: number | undefined, fallback: number) {
  return new Date(value ?? fallback).toISOString()
}

function toStoredMessage(sessionId: string, message: ConvexMessage): StoredChatMessage {
  return {
    id: message._id,
    clientId: message.clientId,
    content: message.content,
    role: message.role,
    timestamp: new Date(message.createdAt).toISOString(),
    sessionId,
    isThinking: message.isStreaming ?? false,
    error: message.metadata?.error,
    metadata: message.metadata?.source === 'reasoning-plane'
      ? {
          source: 'reasoning-plane',
          citations: message.metadata.citations,
        }
      : undefined,
  }
}

function toStoredSession(
  conversation: ConvexConversation & { messages?: ConvexMessage[] },
  actor: Pick<ChatActorContext, 'userId' | 'orgId'>,
): StoredChatSession {
  const createdAt = toConversationTimestamp(conversation.createdAt, Date.now())
  const updatedAt = toConversationTimestamp(conversation.updatedAt, conversation.createdAt)

  return {
    id: conversation._id,
    title: conversation.title,
    createdAt,
    updatedAt,
    userId: actor.userId,
    orgId: actor.orgId,
    messages: (conversation.messages ?? []).map((message) =>
      toStoredMessage(conversation._id, message),
    ),
  }
}

function toSessionListItem(
  conversation: ConvexConversation,
  actor: Pick<ChatActorContext, 'userId' | 'orgId'>,
): SessionListItem {
  return {
    id: conversation._id,
    title: conversation.title,
    createdAt: toConversationTimestamp(conversation.createdAt, Date.now()),
    updatedAt: toConversationTimestamp(conversation.updatedAt, conversation.createdAt),
    userId: actor.userId,
    orgId: actor.orgId,
    messageCount: conversation.messageCount ?? 0,
  }
}

function normalizeRole(role?: string): 'admin' | 'member' | 'viewer' {
  if (role === 'admin' || role === 'owner') {
    return 'admin'
  }

  if (role === 'viewer') {
    return 'viewer'
  }

  return 'member'
}

function buildInternalHeaders(actor: {
  userId: string
  userEmail?: string
  userName?: string
}) {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    'X-User-Id': actor.userId,
  }
  if (INTERNAL_API_KEY) {
    headers['X-Internal-Api-Key'] = INTERNAL_API_KEY
  }

  if (actor.userEmail) {
    headers['X-User-Email'] = actor.userEmail
  }

  if (actor.userName) {
    headers['X-User-Name'] = actor.userName
  }

  return headers
}

async function fetchInternalJson<T>(url: string, init: RequestInit): Promise<T | null> {
  try {
    const response = await fetch(url, {
      ...init,
      cache: 'no-store',
      signal: AbortSignal.timeout(5_000),
    })

    if (!response.ok) {
      return null
    }

    return (await response.json()) as T
  } catch {
    return null
  }
}

async function getSessionContext(actor: {
  userId: string
  userEmail: string
  userName: string
}) {
  return await fetchInternalJson<UserSessionContext>(
    `${USER_SERVICE_URL}/api/v1/me/session-context`,
    {
      method: 'GET',
      headers: buildInternalHeaders(actor),
    },
  )
}

async function getOrganizationsForUser(actor: {
  userId: string
  userEmail: string
  userName: string
}) {
  const organizations = await fetchInternalJson<OrganizationSummary[]>(
    `${ORG_SERVICE_URL}/orgs/me`,
    {
      method: 'GET',
      headers: buildInternalHeaders(actor),
    },
  )

  return Array.isArray(organizations) ? organizations : []
}

async function getOrganizationDetails(
  actor: {
    userId: string
    userEmail: string
    userName: string
  },
  orgId: string,
) {
  return await fetchInternalJson<OrganizationDetails>(
    `${ORG_SERVICE_URL}/orgs/${orgId}`,
    {
      method: 'GET',
      headers: buildInternalHeaders(actor),
    },
  )
}

function readLegacyMigrationState(): MigrationState {
  if (!existsSync(LEGACY_MIGRATION_STATE_PATH)) {
    return {}
  }

  try {
    const raw = readFileSync(LEGACY_MIGRATION_STATE_PATH, 'utf8')
    if (!raw.trim()) {
      return {}
    }

    return JSON.parse(raw) as MigrationState
  } catch {
    return {}
  }
}

function writeLegacyMigrationState(state: MigrationState) {
  mkdirSync(path.dirname(LEGACY_MIGRATION_STATE_PATH), { recursive: true })
  writeFileSync(LEGACY_MIGRATION_STATE_PATH, JSON.stringify(state, null, 2), 'utf8')
}

function readLegacySessions(): LegacySessionStore {
  if (!existsSync(LEGACY_CHAT_SESSION_STORE_PATH)) {
    return {}
  }

  try {
    const raw = readFileSync(LEGACY_CHAT_SESSION_STORE_PATH, 'utf8')
    if (!raw.trim()) {
      return {}
    }

    return JSON.parse(raw) as LegacySessionStore
  } catch {
    return {}
  }
}

async function migrateLegacySessionsForActor(actor: ChatActorContext) {
  const migrationState = readLegacyMigrationState()

  if (migrationState[actor.userId]) {
    return
  }

  const legacySessions = Object.values(readLegacySessions())
    .filter((session) => session.userId === actor.userId && session.messages.length > 0)
    .sort((left, right) => left.createdAt.localeCompare(right.createdAt))

  for (const legacySession of legacySessions) {
    const createdAt = Date.parse(legacySession.createdAt) || Date.now()
    const updatedAt = Date.parse(legacySession.updatedAt) || createdAt

    const conversationId = await convexMutation<string>('conversations:create', {
      orgId: actor.convexOrgId,
      userId: actor.convexUserId,
      title: legacySession.title || buildChatTitle(legacySession.messages[0]?.content ?? ''),
      sessionId: legacySession.id,
      createdAt,
      updatedAt,
    })

    for (const legacyMessage of legacySession.messages) {
      const messageTimestamp = Date.parse(legacyMessage.timestamp) || createdAt

      await convexMutation<string>('messages:create', {
        conversationId,
        role: legacyMessage.role,
        content: legacyMessage.content,
        userId: legacyMessage.role === 'user' ? actor.convexUserId : undefined,
        metadata: legacyMessage.metadata
          ? {
              source: legacyMessage.metadata.source,
              citations: legacyMessage.metadata.citations,
              error: legacyMessage.error,
            }
          : legacyMessage.error
            ? {
                error: legacyMessage.error,
                source: 'reasoning-plane',
              }
            : undefined,
        createdAt: messageTimestamp,
        updatedAt: messageTimestamp,
      })
    }
  }

  migrationState[actor.userId] = true
  writeLegacyMigrationState(migrationState)
}

async function ensureConvexOrganization(
  externalOrgId: string,
  orgDetails: OrganizationDetails | null,
) {
  const existing = await convexQuery<ConvexOrganization | null>('organizations:getByExternalId', {
    externalOrgId,
  })

  const name = orgDetails?.name || `Organization ${externalOrgId}`
  const slug = orgDetails?.slug || slugify(name || externalOrgId)
  const externalCreatedAt =
    Date.parse(orgDetails?.created_at || orgDetails?.createdAt || '') || Date.now()

  if (!existing) {
    const convexOrgId = await convexMutation<string>('organizations:createFromExternal', {
      externalOrgId,
      name,
      slug,
      externalCreatedAt,
    })

    const created = await convexQuery<ConvexOrganization | null>('organizations:getById', {
      orgId: convexOrgId,
    })

    if (!created) {
      throw new ChatStoreError('Failed to initialize organization context', 502)
    }

    return created
  }

  if (existing.name !== name || existing.slug !== slug) {
    await convexMutation('organizations:updateFromExternal', {
      convexOrgId: existing._id,
      name,
      slug,
    })
  }

  return (
    (await convexQuery<ConvexOrganization | null>('organizations:getById', {
      orgId: existing._id,
    })) ?? existing
  )
}

async function ensureConvexUser(
  actor: {
    userId: string
    userEmail: string
    userName: string
    role: 'admin' | 'member' | 'viewer'
  },
  convexOrgId: string,
) {
  const existing = await convexQuery<ConvexUser | null>('users:getByExternalAndOrg', {
    externalAuthId: actor.userId,
    orgId: convexOrgId,
  })

  if (
    !existing ||
    existing.email !== actor.userEmail ||
    (actor.userName && existing.name !== actor.userName) ||
    existing.role !== actor.role
  ) {
    await convexMutation('users:createOrUpdateFromExternal', {
      externalAuthId: actor.userId,
      email: actor.userEmail,
      name: actor.userName,
      convexOrgId,
      role: actor.role,
      externalCreatedAt: Date.now(),
    })
  }

  const resolved = await convexQuery<ConvexUser | null>('users:getByExternalAndOrg', {
    externalAuthId: actor.userId,
    orgId: convexOrgId,
  })

  if (!resolved) {
    throw new ChatStoreError('Failed to initialize user context', 502)
  }

  return resolved
}

async function resolveActorFromConvexMirror(
  actor: {
    userId: string
    userEmail: string
    userName: string
  },
): Promise<ChatActorContext | null> {
  const memberships = await convexQuery<ConvexUser[]>('users:listByExternalAuthId', {
    externalAuthId: actor.userId,
  })

  const activeMembership = memberships.find((membership) => membership.syncStatus !== 'deleted')

  if (!activeMembership) {
    return null
  }

  const convexOrg = await convexQuery<ConvexOrganization | null>('organizations:getById', {
    orgId: activeMembership.orgId,
  })

  if (!convexOrg) {
    return null
  }

  await convexMutation('users:updateLastSeen', {
    userId: activeMembership._id,
  })

  return {
    userId: actor.userId,
    userName: actor.userName,
    userEmail: actor.userEmail,
    orgId: convexOrg.externalOrgId,
    role: normalizeRole(activeMembership.role),
    convexOrgId: convexOrg._id,
    convexUserId: activeMembership._id,
  }
}

export async function resolveChatActor(): Promise<ChatActorContext> {
  const session = await getServerSession()

  if (!session?.user?.id || !session.user.email) {
    throw new ChatStoreError('Authentication required', 401)
  }

  const sessionActor = {
    userId: session.user.id,
    userName: session.user.name || session.user.email.split('@')[0] || 'User',
    userEmail: session.user.email,
  }

  // Fire session-context and org-list in parallel to avoid waterfall
  const [sessionContext, organizations] = await Promise.all([
    getSessionContext(sessionActor).catch(() => null),
    getOrganizationsForUser(sessionActor).catch(() => []),
  ])

  const orgId = sessionContext?.orgId || organizations[0]?.id

  if (!orgId) {
    let mirroredActor: ChatActorContext | null = null
    try {
      // Hard cap on the mirror lookup so a slow Convex doesn't make
      // the chat handler hang. 6s covers two serial queries + a mutation
      // with comfortable headroom; the convex client itself has no
      // built-in timeout.
      mirroredActor = await Promise.race([
        resolveActorFromConvexMirror(sessionActor),
        new Promise<never>((_, reject) =>
          setTimeout(() => reject(new Error('convex mirror timeout (6s)')), 6_000),
        ),
      ])
    } catch (err) {
      // Don't claim "Convex unreachable" — the failure could be a slow
      // query, a missing record, or a schema mismatch. Surface what the
      // upstream actually said so the UI can show something useful.
      const reason = err instanceof Error ? err.message : String(err)
      throw new ChatStoreError(`Organization resolution failed: ${reason}`, 503)
    }

    if (!mirroredActor) {
      throw new ChatStoreError('No active organization found for chat', 403)
    }

    await migrateLegacySessionsForActor(mirroredActor).catch(() => {})
    return mirroredActor
  }

  const orgDetails =
    organizations.find((organization) => organization.id === orgId) ||
    (await getOrganizationDetails(sessionActor, orgId))

  let convexOrg: Awaited<ReturnType<typeof ensureConvexOrganization>>
  let convexUser: Awaited<ReturnType<typeof ensureConvexUser>>
  try {
    convexOrg = await ensureConvexOrganization(orgId, orgDetails ?? null)
    const role = normalizeRole(sessionContext?.role)
    convexUser = await ensureConvexUser(
      {
        ...sessionActor,
        role,
      },
      convexOrg._id,
    )
  } catch {
    throw new ChatStoreError('Chat service temporarily unavailable — Convex unreachable', 503)
  }

  const role = normalizeRole(sessionContext?.role)
  const actor: ChatActorContext = {
    ...sessionActor,
    orgId,
    role,
    convexOrgId: convexOrg._id,
    convexUserId: convexUser._id,
  }

  // Fire-and-forget: don't block the response on these
  convexMutation('users:updateLastSeen', {
    userId: actor.convexUserId,
  }).catch(() => {})
  migrateLegacySessionsForActor(actor).catch(() => {})

  return actor
}

export function buildChatTitle(content: string): string {
  const normalized = content.trim().replace(/\s+/g, ' ')

  if (!normalized) {
    return 'Ny samtale'
  }

  return normalized.length > 60 ? `${normalized.slice(0, 57)}...` : normalized
}

export async function listChatSessions(actor: ChatActorContext) {
  const conversations = await convexQuery<ConvexConversation[]>('conversations:listByUser', {
    orgId: actor.convexOrgId,
    userId: actor.convexUserId,
  })

  return conversations.map((conversation) => toSessionListItem(conversation, actor))
}

export async function getChatSession(
  actor: ChatActorContext,
  sessionId: string,
): Promise<StoredChatSession | null> {
  // Wave 9 follow-up: the playground hook (`useAgentPlayground`) coins a
  // synthetic session id `playground-{agentId}-{ts}` so it can resume the
  // same conversation across turns without hitting the server first. That
  // string is NOT a valid Convex `v.id("conversations")`, so the underlying
  // `conversations:get` query throws an ArgumentValidationError rather than
  // returning null. Previously that error escaped both `ensureChatSession`
  // and the outer route handler, surfacing as a generic 502 "Reasoning
  // plane unavailable" — which made the playground look like the gateway
  // was down. Swallow the validator error here and report "no such
  // session" so the caller can fall through to `createChatSession`.
  let conversation: (ConvexConversation & { messages: ConvexMessage[] }) | null = null
  try {
    conversation = await convexQuery<(ConvexConversation & { messages: ConvexMessage[] }) | null>(
      'conversations:get',
      {
        conversationId: sessionId,
        orgId: actor.convexOrgId,
        userId: actor.convexUserId,
      },
    )
  } catch (error) {
    const message = error instanceof Error ? error.message : ''
    // Convex's ArgumentValidationError reliably contains "Value does not
    // match validator" or "ArgumentValidationError" in the message.
    if (
      /ArgumentValidationError|Value does not match validator|Validator: v\.id/i.test(message)
    ) {
      return null
    }
    throw error
  }

  if (!conversation) {
    return null
  }

  return toStoredSession(conversation, actor)
}

export async function createChatSession(
  actor: ChatActorContext,
  title = 'Ny samtale',
): Promise<StoredChatSession> {
  const conversationId = await convexMutation<string>('conversations:create', {
    orgId: actor.convexOrgId,
    userId: actor.convexUserId,
    title,
  })

  const session = await getChatSession(actor, conversationId)

  if (!session) {
    throw new ChatStoreError('Failed to create chat session', 502)
  }

  return session
}

export async function ensureChatSession(
  actor: ChatActorContext,
  options: {
    sessionId?: string
    title?: string
  },
): Promise<StoredChatSession> {
  if (options.sessionId) {
    const existing = await getChatSession(actor, options.sessionId)
    if (existing) {
      return existing
    }
    // The id was supplied but couldn't be resolved — could be a
    // synthetic playground id, a stale tab pointing at a deleted
    // conversation, or a brand-new "fresh chat" request from a client
    // that mints an id locally. Treat all three the same way: spin up a
    // new Convex conversation. The caller's subsequent
    // `appendMessagesToSession` will then write to a real id. Throwing
    // 404 here used to break the playground (synthetic ids) and any
    // "resume after refresh" flow where the conversation had been
    // garbage-collected.
    return await createChatSession(actor, options.title ?? 'Ny samtale')
  }

  return await createChatSession(actor, options.title ?? 'Ny samtale')
}

export async function updateChatSessionTitle(
  actor: ChatActorContext,
  sessionId: string,
  title: string,
): Promise<StoredChatSession> {
  await convexMutation('conversations:updateTitle', {
    conversationId: sessionId,
    orgId: actor.convexOrgId,
    userId: actor.convexUserId,
    title,
  })

  const session = await getChatSession(actor, sessionId)
  if (!session) {
    throw new ChatStoreError('Chat session not found', 404)
  }

  return session
}

export async function appendMessagesToSession(
  actor: ChatActorContext,
  options: {
    sessionId: string
    title?: string
    messages: StoredChatMessage[]
  },
): Promise<StoredChatSession> {
  const existing = await getChatSession(actor, options.sessionId)
  if (!existing) {
    throw new ChatStoreError('Chat session not found', 404)
  }

  if (options.title && options.title !== existing.title) {
    await updateChatSessionTitle(actor, options.sessionId, options.title)
  }

  for (const message of options.messages) {
    await convexMutation<string>('messages:create', {
      conversationId: options.sessionId,
      role: message.role,
      content: message.content,
      userId: message.role === 'user' ? actor.convexUserId : undefined,
      clientId: message.clientId,
      metadata: message.metadata
        ? {
            source: message.metadata.source,
            citations: message.metadata.citations,
            error: message.error,
          }
        : message.error
          ? {
              error: message.error,
              source: 'reasoning-plane',
            }
          : undefined,
      createdAt: Date.parse(message.timestamp) || Date.now(),
      updatedAt: Date.parse(message.timestamp) || Date.now(),
    })
  }

  const updated = await getChatSession(actor, options.sessionId)

  if (!updated) {
    throw new ChatStoreError('Failed to sync chat session', 502)
  }

  return updated
}

export async function createStreamingAssistantMessage(
  _actor: ChatActorContext,
  sessionId: string,
): Promise<StoredChatMessage> {
  const createdAt = Date.now()
  const messageId = await convexMutation<string>('messages:createStreaming', {
    conversationId: sessionId,
    createdAt,
  })

  return {
    id: messageId,
    content: '',
    role: 'assistant',
    timestamp: new Date(createdAt).toISOString(),
    sessionId,
    isThinking: true,
  }
}

export async function updateStreamingAssistantMessage(
  _actor: ChatActorContext,
  messageId: string,
  chunk: string,
) {
  const updated = await convexMutation<ConvexMessage | null>('messages:updateStreaming', {
    messageId,
    chunk,
  })

  if (!updated) {
    throw new ChatStoreError('Streaming message not found', 404)
  }

  return toStoredMessage(updated.conversationId, updated)
}

export async function finalizeStreamingAssistantMessage(
  _actor: ChatActorContext,
  options: {
    messageId: string
    content: string
    metadata?: ChatMessageMetadata
    error?: string
  },
) {
  const updated = await convexMutation<ConvexMessage | null>('messages:finalizeStreaming', {
    messageId: options.messageId,
    content: options.content,
    metadata: options.metadata
      ? {
          source: options.metadata.source,
          citations: options.metadata.citations,
          error: options.error,
        }
      : options.error
        ? {
            source: 'reasoning-plane',
            error: options.error,
          }
        : undefined,
  })

  if (!updated) {
    throw new ChatStoreError('Streaming message not found', 404)
  }

  return toStoredMessage(updated.conversationId, updated)
}

export async function deleteChatSession(actor: ChatActorContext, sessionId: string) {
  await convexMutation('conversations:remove', {
    conversationId: sessionId,
    orgId: actor.convexOrgId,
    userId: actor.convexUserId,
  })

  return true
}
