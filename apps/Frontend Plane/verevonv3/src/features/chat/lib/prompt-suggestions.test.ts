import { describe, expect, it } from 'vitest'
import { toLiveTicket, type ConversationSummary } from '@/shared/api/inbox-client'
import type { Notification } from '@/shared/api/notifications-client'
import { buildPromptSuggestions } from './prompt-suggestions'

const now = Date.parse('2026-10-11T12:00:00Z')

function notification(overrides: Partial<Notification> = {}): Notification {
  return {
    id: 'notification-1',
    organization_id: 'org-a',
    recipient_id: 'user-a',
    event_type: 'case.alert',
    channel: 'in_app',
    title: 'Leveransen trenger avklaring',
    body: 'En ny hendelse er registrert.',
    read: false,
    archived: false,
    submitted_at: '2026-10-11T11:00:00Z',
    ...overrides,
  }
}

function conversation(overrides: Partial<ConversationSummary> = {}) {
  return toLiveTicket({
    id: 'conv-1',
    org_id: 'org-a',
    inbox_id: 'inbox-1',
    title: 'Spørsmål om levering',
    status: 'open',
    priority: 'normal',
    channel: 'email',
    last_message_preview: 'Kan dere bekrefte tidspunktet?',
    created_at: '2026-10-10T08:00:00Z',
    updated_at: '2026-10-11T10:00:00Z',
    ...overrides,
  })
}

describe('buildPromptSuggestions', () => {
  it('keeps the quiet state empty when there is no recent actionable activity', () => {
    expect(buildPromptSuggestions({ notifications: [], conversations: [], orgId: 'org-a', userId: 'user-a', now })).toEqual([])
    expect(buildPromptSuggestions({
      notifications: [notification({ read: true }), notification({ id: 'old', submitted_at: '2026-09-01T11:00:00Z' })],
      conversations: [conversation({ status: 'closed' })],
      orgId: 'org-a', userId: 'user-a', now,
    })).toEqual([])
  })

  it('uses only this person’s alerts and active conversations, newest first', () => {
    const result = buildPromptSuggestions({
      notifications: [
        notification(),
        notification({ id: 'other-org', organization_id: 'org-b' }),
        notification({ id: 'other-user', recipient_id: 'user-b' }),
        notification({ id: 'archived', archived: true }),
      ],
      conversations: [
        conversation(),
        conversation({ id: 'conv-other', title: 'Someone else’s case', assignee_user_id: 'user-b' }),
      ],
      orgId: 'org-a', userId: 'user-a', now,
    })

    expect(result.map((item) => [item.source, item.title])).toEqual([
      ['alert', 'Leveransen trenger avklaring'],
      ['inbox', 'Spørsmål om levering'],
    ])
  })

  it('removes repeated subjects and limits the suggestions to three', () => {
    const result = buildPromptSuggestions({
      notifications: [
        notification(),
        notification({ id: 'duplicate', title: '  leveransen trenger avklaring  ', submitted_at: '2026-10-11T09:00:00Z' }),
        notification({ id: 'third', title: 'Tredje varsel', submitted_at: '2026-10-11T08:00:00Z' }),
        notification({ id: 'fourth', title: 'Fjerde varsel', submitted_at: '2026-10-11T07:00:00Z' }),
      ],
      conversations: [conversation()],
      orgId: 'org-a', userId: 'user-a', now,
    })

    expect(result.map((item) => item.title)).toEqual([
      'Leveransen trenger avklaring',
      'Spørsmål om levering',
      'Tredje varsel',
    ])
  })

  it('leaves routine digests and promotions out of follow-up suggestions', () => {
    const result = buildPromptSuggestions({
      notifications: [],
      conversations: [
        conversation({ id: 'failed', title: 'Backup failed', last_message_preview: 'Network share failed\u034f' }),
        conversation({ id: 'digest', title: 'Your weekly PIM digest', last_message_preview: 'Here is a summary of activities.' }),
        conversation({ id: 'promotion', title: 'Barn | Comfort Capsule', last_message_preview: 'Myke plagg til hverdagen.\u034f' }),
        conversation({ id: 'no-action', title: 'Service has been deprecated', last_message_preview: 'No action required.' }),
      ],
      orgId: 'org-a', userId: 'user-a', now,
    })

    expect(result.map((item) => item.title)).toEqual(['Backup failed'])
    expect(result[0]?.detail).toBe('Network share failed')
  })
})
