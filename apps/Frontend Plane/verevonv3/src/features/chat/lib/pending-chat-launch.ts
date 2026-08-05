import { objectUrlToDataUrl } from '@/shared/lib/blob-data'
import { readClientJson, removeClientValue, writeClientJson } from '@/shared/session/client-storage'

const pendingLaunchKey = 'verevon.chat.pendingLaunch'

export type PendingChatTool = 'image' | 'reason' | 'research' | 'search'

export type PendingChatAction = {
  id: string
  kind: 'skill' | 'capability' | 'connector' | 'tool'
  name: string
}

export type PendingChatAttachment = {
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
  actions?: PendingChatAction[]
  attachments?: PendingChatAttachment[]
  createdAt?: string
  model?: string
  /** Deliberate handoffs must not append to the previously active global thread. */
  startNewThread?: boolean
  /** Exact support scope that should receive the resulting Chat answer. */
  supportHandoff?: PendingSupportHandoff
  text: string
  tools?: PendingChatTool[]
}

export async function writePendingChatLaunch(payload: PendingChatLaunch): Promise<void> {
  const attachments: PendingChatAttachment[] = []

  for (const attachment of payload.attachments ?? []) {
    let url = attachment.url
    try {
      if (url.startsWith('blob:') && attachment.type.startsWith('image/')) {
        url = await objectUrlToDataUrl(url)
      }
    } catch {
      if (attachment.type.startsWith('image/')) continue
    }
    attachments.push({ ...attachment, url })
  }

  writeClientJson(pendingLaunchKey, {
    ...payload,
    attachments,
    createdAt: payload.createdAt ?? new Date().toISOString(),
  })
}

export function consumePendingChatLaunch(): PendingChatLaunch | null {
  const launch = readClientJson(pendingLaunchKey, isPendingChatLaunch)
  removeClientValue(pendingLaunchKey)
  if (!launch) return null
  return {
    actions: normalizePendingActions(launch.actions),
    attachments: normalizePendingAttachments(launch.attachments),
    createdAt: launch.createdAt,
    model: launch.model,
    startNewThread: launch.startNewThread === true,
    supportHandoff: normalizePendingSupportHandoff(launch.supportHandoff),
    text: launch.text,
    tools: normalizePendingTools(launch.tools),
  }
}

function isPendingChatLaunch(value: unknown): value is PendingChatLaunch {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  if (typeof record.text !== 'string' || !record.text.trim()) return false
  if (record.model !== undefined && typeof record.model !== 'string') return false
  if (record.createdAt !== undefined && typeof record.createdAt !== 'string') return false
  if (record.attachments !== undefined && !Array.isArray(record.attachments)) return false
  if (record.tools !== undefined && !Array.isArray(record.tools)) return false
  if (record.actions !== undefined && !Array.isArray(record.actions)) return false
  if (record.supportHandoff !== undefined && !isPendingSupportHandoff(record.supportHandoff)) return false
  return true
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
  return value === 'image' || value === 'reason' || value === 'research' || value === 'search'
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
