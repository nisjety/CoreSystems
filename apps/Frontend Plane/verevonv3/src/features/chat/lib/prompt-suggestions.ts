import type { LiveTicket } from '@/shared/api/inbox-client'
import type { Notification } from '@/shared/api/notifications-client'

export type PromptSuggestion = {
  id: string
  source: 'alert' | 'inbox'
  title: string
  detail: string
  occurredAt: string
}

const RECENT_WINDOW_MS = 14 * 24 * 60 * 60 * 1000

function isRecent(value: string, now: number): boolean {
  const time = Date.parse(value)
  return Number.isFinite(time) && time <= now && now - time <= RECENT_WINDOW_MS
}

function clean(value: string | undefined): string {
  return (value ?? '').replace(/[\u034f\u200b-\u200f\u2060\ufeff]/g, '').replace(/\s+/g, ' ').trim()
}

const FOLLOW_UP_CUES = [
  'failed', 'failure', 'error', 'incident', 'alert', 'action required', 'overdue', 'urgent',
  'feil', 'avvik', 'mangler', 'haster', 'forfalt', 'bekreft', 'spørsmål', 'kan dere',
  'could you', 'please', 'request', 'forespørsel', 'ordre', 'faktura',
]

function needsFollowUp(item: LiveTicket): boolean {
  const summary = `${clean(item.title)} ${clean(item.lastMessagePreview).slice(0, 160)}`.toLocaleLowerCase()
  if (summary.includes('no action required') || summary.includes('ingen handling nødvendig')) return false
  return summary.includes('?') || FOLLOW_UP_CUES.some((cue) => summary.includes(cue))
}

/** Only records already scoped by the gateway can become a first-turn suggestion. */
export function buildPromptSuggestions(input: {
  notifications: Notification[]
  conversations: LiveTicket[]
  orgId: string
  userId: string
  now?: number
}): PromptSuggestion[] {
  const now = input.now ?? Date.now()
  const alerts: PromptSuggestion[] = input.notifications
    .filter((item) => item.organization_id === input.orgId
      && item.recipient_id === input.userId
      && !item.read
      && !item.archived
      && isRecent(item.submitted_at, now))
    .map((item) => ({
      id: `alert:${item.id}`,
      source: 'alert' as const,
      title: clean(item.title),
      detail: clean(item.body),
      occurredAt: item.submitted_at,
    }))
    .filter((item) => item.title)

  const inbox: PromptSuggestion[] = input.conversations
    .filter((item) => item.state?.name === 'open'
      && (!item.assigneeUserId || item.assigneeUserId === input.userId)
      && clean(item.lastMessagePreview)
      && needsFollowUp(item)
      && isRecent(item.updated_at, now))
    .map((item) => ({
      id: `inbox:${item.conversationId}`,
      source: 'inbox' as const,
      title: clean(item.title),
      detail: clean(item.lastMessagePreview),
      occurredAt: item.updated_at,
    }))
    .filter((item) => item.title)

  const seen = new Set<string>()
  return [...alerts, ...inbox]
    .sort((a, b) => b.occurredAt.localeCompare(a.occurredAt))
    .filter((item) => {
      const key = item.title.toLocaleLowerCase()
      if (seen.has(key)) return false
      seen.add(key)
      return true
    })
    .slice(0, 3)
}
