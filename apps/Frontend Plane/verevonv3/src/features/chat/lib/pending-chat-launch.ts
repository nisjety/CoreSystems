import { objectUrlToDataUrl } from '@/shared/lib/blob-data'
import { normalizePrivacyTier, type PrivacyTier } from '@/shared/api/privacy-tier'
import { removeClientValue } from '@/shared/session/client-storage'

const pendingLaunchKey = 'verevon.chat.pendingLaunch'
// Keep normal launches tab-local and bounded. Temporary chats never touch storage.
export const MAX_PENDING_LAUNCH_CHARS = 2_000_000
let temporaryLaunch: PendingChatLaunch | null = null

export class PendingChatLaunchError extends Error {
  readonly reason: 'unreadable' | 'too_large' | 'storage'
  readonly filename?: string
  constructor(reason: 'unreadable' | 'too_large' | 'storage', filename?: string) {
    super(reason)
    this.reason = reason
    this.filename = filename
    this.name = 'PendingChatLaunchError'
  }
}

export type PendingChatTool = 'image' | 'research' | 'search'

export type PendingChatAction = {
  id: string
  kind: 'skill' | 'capability' | 'connector' | 'tool'
  name: string
}

export type PendingChatAttachment = {
  extractedText?: string
  id: string
  name: string
  size: number
  type: string
  url: string
}

export type PendingSupportHandoff = {
  conversationId: string
  orgId: string
  userId: string
}

export type PendingChatLaunch = {
  sourceScope?: import('@/shared/actions/chat-source-scope').ChatSourceScope
  actions?: PendingChatAction[]
  attachments?: PendingChatAttachment[]
  createdAt?: string
  effort?: 'quick' | 'deep'
  minPrivacyTier?: PrivacyTier
  model?: string
  /** Explicit route for a connected user-owned model subscription. */
  provider?: string
  /** Deliberate handoffs must not append to the previously active global thread. */
  startNewThread?: boolean
  /** Opaque Integration Core connection id; never an OAuth credential. */
  subscriptionConnectionId?: string
  /** Exact support scope that should receive the resulting Chat answer. */
  supportHandoff?: PendingSupportHandoff
  text: string
  tone?: 'concise' | 'detailed'
  tools?: PendingChatTool[]
  zdr?: boolean
}

export async function writePendingChatLaunch(payload: PendingChatLaunch): Promise<void> {
  const attachments: PendingChatAttachment[] = []

  for (const attachment of payload.attachments ?? []) {
    let url = attachment.url
    try {
      if (url.startsWith('blob:')) {
        url = await objectUrlToDataUrl(url)
      }
      if (!url) throw new Error('missing attachment URL')
    } catch {
      throw new PendingChatLaunchError('unreadable', attachment.name)
    }
    attachments.push({ ...attachment, url })
  }

  const launch = {
    ...payload,
    attachments,
    createdAt: payload.createdAt ?? new Date().toISOString(),
  }
  const serialized = JSON.stringify(launch)
  if (serialized.length > MAX_PENDING_LAUNCH_CHARS) throw new PendingChatLaunchError('too_large')
  if (payload.zdr) {
    removeClientValue(pendingLaunchKey)
    temporaryLaunch = launch
    return
  }
  try {
    // Do not use the best-effort preferences writer: the composer may clear its
    // draft only after this write succeeds. A quota/security error must reject.
    window.sessionStorage.setItem(pendingLaunchKey, serialized)
  } catch {
    throw new PendingChatLaunchError('storage')
  }
  temporaryLaunch = null
  // Remove content left by versions that persisted launches in localStorage.
  try { window.localStorage.removeItem(pendingLaunchKey) } catch { /* unavailable */ }
}

export function consumePendingChatLaunch(): PendingChatLaunch | null {
  let launch: PendingChatLaunch | null = temporaryLaunch
  temporaryLaunch = null
  if (!launch) {
    try {
      const stored: unknown = JSON.parse(window.sessionStorage.getItem(pendingLaunchKey) ?? 'null')
      if (isPendingChatLaunch(stored)) launch = stored
    } catch { /* no readable launch */ }
  }
  removeClientValue(pendingLaunchKey)
  if (!launch) return null
  return {
    actions: normalizePendingActions(launch.actions),
    attachments: normalizePendingAttachments(launch.attachments),
    createdAt: launch.createdAt,
    effort: normalizeEffort(launch.effort),
    minPrivacyTier: normalizePrivacyTier(launch.minPrivacyTier),
    model: launch.model,
    provider: normalizeOptionalString(launch.provider),
    startNewThread: launch.startNewThread === true,
    subscriptionConnectionId: normalizeOptionalString(launch.subscriptionConnectionId),
    supportHandoff: normalizePendingSupportHandoff(launch.supportHandoff),
    text: launch.text,
    sourceScope: launch.sourceScope === 'conversation' ? 'conversation' : 'workspace',
    tone: normalizeTone(launch.tone),
    tools: normalizePendingTools(launch.tools),
    zdr: launch.zdr === true,
  }
}

function isPendingChatLaunch(value: unknown): value is PendingChatLaunch {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  if (typeof record.text !== 'string' || !record.text.trim()) return false
  if (record.model !== undefined && typeof record.model !== 'string') return false
  if (record.provider !== undefined && typeof record.provider !== 'string') return false
  if (record.subscriptionConnectionId !== undefined && typeof record.subscriptionConnectionId !== 'string') return false
  if (record.minPrivacyTier !== undefined && !normalizePrivacyTier(record.minPrivacyTier)) return false
  if (record.effort !== undefined && !normalizeEffort(record.effort)) return false
  if (record.tone !== undefined && !normalizeTone(record.tone)) return false
  if (record.zdr !== undefined && typeof record.zdr !== 'boolean') return false
  if (record.createdAt !== undefined && typeof record.createdAt !== 'string') return false
  if (record.attachments !== undefined && !Array.isArray(record.attachments)) return false
  if (record.tools !== undefined && !Array.isArray(record.tools)) return false
  if (record.actions !== undefined && !Array.isArray(record.actions)) return false
  if (record.supportHandoff !== undefined && !isPendingSupportHandoff(record.supportHandoff)) return false
  return true
}

function normalizeOptionalString(value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined
  return value.trim() || undefined
}

function normalizeEffort(value: unknown): PendingChatLaunch['effort'] {
  return value === 'quick' || value === 'deep' ? value : undefined
}

function normalizeTone(value: unknown): PendingChatLaunch['tone'] {
  return value === 'concise' || value === 'detailed' ? value : undefined
}

export function normalizePendingAttachments(value: unknown): PendingChatAttachment[] {
  return Array.isArray(value) ? value.filter(isPendingAttachment) : []
}

export function normalizePendingTools(value: unknown): PendingChatTool[] {
  return Array.isArray(value) ? value.filter(isPendingTool) : []
}

export function normalizePendingActions(value: unknown): PendingChatAction[] {
  return Array.isArray(value) ? value.filter(isPendingAction) : []
}

export function normalizePendingSupportHandoff(value: unknown): PendingSupportHandoff | undefined {
  if (!isPendingSupportHandoff(value)) return undefined
  return {
    conversationId: value.conversationId.trim(),
    orgId: value.orgId.trim(),
    userId: value.userId.trim(),
  }
}

function isPendingAttachment(value: unknown): value is PendingChatAttachment {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return typeof record.id === 'string'
    && typeof record.name === 'string'
    && typeof record.size === 'number'
    && typeof record.type === 'string'
    && typeof record.url === 'string'
}

function isPendingTool(value: unknown): value is PendingChatTool {
  // A legacy hand-off carrying the removed 'reason' tool is filtered out
  // here rather than rejected: the value no longer means anything, but the
  // rest of the pending launch is still perfectly valid.
  return value === 'image' || value === 'research' || value === 'search'
}

function isPendingAction(value: unknown): value is PendingChatAction {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return typeof record.id === 'string'
    && typeof record.name === 'string'
    && (
      record.kind === 'skill'
      || record.kind === 'capability'
      || record.kind === 'connector'
      || record.kind === 'tool'
    )
}

function isPendingSupportHandoff(value: unknown): value is PendingSupportHandoff {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return [record.conversationId, record.orgId, record.userId].every(
    (part) => typeof part === 'string' && Boolean(part.trim()),
  )
}
