// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createStore, flush } from 'solid-js'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatThreadSession } from '@/shared/api/chat-client'

vi.mock('@/shared/api/chat-client', () => ({ listChatThreads: vi.fn() }))
vi.mock('./DashboardCards', () => ({ DashboardImageCard: () => null }))
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))
import { listChatThreads } from '@/shared/api/chat-client'
import { DashboardRecentWork } from './DashboardOverview'

const [identity, setIdentity] = createStore({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const thread = (threadId: string, title: string, updatedAt: string, pinned = false) => ({ threadId, title, preview: '', updatedAt, pinned }) as ChatThreadSession
beforeEach(() => {
  vi.mocked(listChatThreads).mockReset()
  setIdentity((state) => { state.user.id = 'user-a'; state.activeOrg.id = 'org-a' })
  flush()
})
describe('Home recent work', () => {
  it('shows real recent threads in update order and filters pinned threads', async () => {
    vi.mocked(listChatThreads).mockResolvedValue([
      thread('older', 'Older work', '2026-09-20T10:00:00Z', true),
      thread('newer', 'Newer work', '2026-09-28T10:00:00Z'),
    ])
    const view = render(() => <DashboardRecentWork />)
    await waitFor(() => expect(view.container.querySelectorAll('.home-recent__row')).toHaveLength(2))
    expect(view.container.querySelector('.home-recent__row')?.textContent).toContain('Newer work')
    fireEvent.click(screen.getByRole('button', { name: 'Pinned' }))
    flush()
    expect(screen.queryByText('Newer work')).toBeNull()
    expect(screen.getByText('Older work')).toBeTruthy()
  })
  it('shows a recoverable error rather than a fabricated empty or metric', async () => {
    vi.mocked(listChatThreads).mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce([])
    render(() => <DashboardRecentWork />)
    await waitFor(() => expect(screen.getByText('Conversations could not be loaded.')).toBeTruthy())
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }))
    flush()
    await waitFor(() => expect(screen.getByText('Your next piece of work starts here.')).toBeTruthy())
    expect(listChatThreads).toHaveBeenCalledTimes(2)
  })
  it('ignores an old workspace response after the workspace changes', async () => {
    let resolveOld: (value: ChatThreadSession[]) => void = () => undefined
    vi.mocked(listChatThreads).mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve }))
      .mockResolvedValueOnce([thread('new-org', 'Current workspace work', '2026-09-28T10:00:00Z')])
    render(() => <DashboardRecentWork />)
    flush()
    setIdentity((state) => { state.activeOrg.id = 'org-b' })
    flush()
    await waitFor(() => expect(screen.getByText('Current workspace work')).toBeTruthy())
    resolveOld([thread('old-org', 'Old workspace work', '2026-09-28T11:00:00Z')])
    await Promise.resolve()
    flush()
    expect(screen.queryByText('Old workspace work')).toBeNull()
    expect(screen.getByText('Current workspace work')).toBeTruthy()
  })
})
