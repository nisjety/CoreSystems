import { describe, expect, it, vi } from 'vitest'
vi.mock('@/shared/api/spaces-client', () => ({ listSpaces: vi.fn() }))
vi.mock('@/shared/api/inbox-client', () => ({ listConversations: vi.fn() }))
import { listSpaces } from '@/shared/api/spaces-client'
import { listConversations } from '@/shared/api/inbox-client'
import { loadHomeSection } from './home-section-data'
const tr = (_no: string, en: string) => en

describe('Home overview owner data', () => {
  it('reports lifecycle status without inferring sharing from the open-ended room kind', async () => {
    vi.mocked(listSpaces).mockResolvedValue([
      { space_ref: 'space/a', name: 'Team room', kind: 'room', lifecycle: 'active' },
      { space_ref: 'archived', name: 'Archived room', kind: 'room', lifecycle: 'archived' },
    ])
    const data = await loadHomeSection('spaces', 'org-a', tr)
    expect(data.metrics).toEqual([{ label: 'Available rooms', value: 2 }, { label: 'Active rooms', value: 1 }])
    expect(data.items[0]).toMatchObject({ detail: 'Room', href: '/spaces/space%2Fa' })
  })

  it('labels a partial support queue as a view count, not a workspace total', async () => {
    vi.mocked(listConversations).mockResolvedValue({ tickets: [], total: 0, nextCursor: { id: 'more', updated: '2026-09-28' } })
    const data = await loadHomeSection('support', 'org-a', tr)
    expect(listConversations).toHaveBeenCalledWith('org-a', { limit: 6 })
    expect(data.metrics[0]?.label).toBe('Conversations in this view')
    expect(data.note).toContain('Open Support for the full queue.')
  })
})
