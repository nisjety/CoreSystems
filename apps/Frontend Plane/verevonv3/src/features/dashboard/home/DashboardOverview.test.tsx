// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createStore, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatThreadSession } from '@/shared/api/chat-client'

vi.mock('@/shared/api/chat-client', () => ({ listChatThreads: vi.fn() }))
vi.mock('./DashboardCards', () => ({ DashboardImageCard: () => null }))
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))
// These tests render the card bare; row navigation is covered with a real router in DashboardOverview.thread-link.test.tsx.
vi.mock('@solidjs/router', () => ({ useNavigate: () => () => undefined }))
import { listChatThreads } from '@/shared/api/chat-client'
import { DashboardRecentWork } from './DashboardOverview'
import { homeLayoutStorageKey } from './home-layout-state'

const [identity, setIdentity] = createStore({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const layoutKey = (user: string, org = 'org-a') => homeLayoutStorageKey(user, org)!
const pinnedButton = () => screen.getByRole('button', { name: 'Pinned' })
const thread = (threadId: string, title: string, updatedAt: string, pinned = false) => ({ threadId, title, preview: '', updatedAt, pinned }) as ChatThreadSession
beforeEach(() => {
  // The Pinned filter is remembered per account in the browser; each test starts fresh.
  localStorage.clear(); sessionStorage.clear()
  vi.mocked(listChatThreads).mockReset()
  setIdentity((state) => { state.user.id = 'user-a'; state.activeOrg.id = 'org-a' })
  flush()
})
afterEach(() => vi.restoreAllMocks())
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

describe('Home recent work Pinned filter is remembered per browser and account', () => {
  const threads = [thread('older', 'Older work', '2026-09-20T10:00:00Z', true), thread('newer', 'Newer work', '2026-09-28T10:00:00Z')]
  beforeEach(() => { vi.mocked(listChatThreads).mockResolvedValue(threads) })

  it('is off by default and remembers the filter when the card remounts', async () => {
    const first = render(() => <DashboardRecentWork />)
    expect(pinnedButton().getAttribute('aria-pressed')).toBe('false')
    expect(localStorage.getItem(layoutKey('user-a'))).toBeNull()
    fireEvent.click(pinnedButton()); flush()
    expect(JSON.parse(localStorage.getItem(layoutKey('user-a'))!)).toMatchObject({ pinnedOnly: true })
    first.unmount()

    const second = render(() => <DashboardRecentWork />)
    // Restored on the first render, not after the list has loaded.
    expect(pinnedButton().getAttribute('aria-pressed')).toBe('true')
    await waitFor(() => expect(second.container.querySelectorAll('.home-recent__row')).toHaveLength(1))
    expect(screen.getByText('Older work')).toBeTruthy()
    expect(screen.queryByText('Newer work')).toBeNull()
    fireEvent.click(pinnedButton()); flush()
    expect(pinnedButton().getAttribute('aria-pressed')).toBe('false')
    expect(JSON.parse(localStorage.getItem(layoutKey('user-a'))!)).toMatchObject({ pinnedOnly: false })
  })

  it('keeps the filter of each account apart and never writes under the previous one', async () => {
    render(() => <DashboardRecentWork />)
    fireEvent.click(pinnedButton()); flush()
    setIdentity((state) => { state.user.id = 'user-b' }); flush()
    expect(pinnedButton().getAttribute('aria-pressed')).toBe('false')
    await waitFor(() => expect(screen.getByText('Newer work')).toBeTruthy())
    expect(localStorage.getItem(layoutKey('user-b'))).toBeNull()
    fireEvent.click(pinnedButton()); flush()
    fireEvent.click(pinnedButton()); flush()
    expect(JSON.parse(localStorage.getItem(layoutKey('user-b'))!)).toMatchObject({ pinnedOnly: false })
    expect(JSON.parse(localStorage.getItem(layoutKey('user-a'))!)).toMatchObject({ pinnedOnly: true })
    setIdentity((state) => { state.user.id = 'user-a' }); flush()
    expect(pinnedButton().getAttribute('aria-pressed')).toBe('true')
    setIdentity((state) => { state.activeOrg.id = 'org-b' }); flush()
    expect(pinnedButton().getAttribute('aria-pressed')).toBe('false')
    expect(localStorage.getItem(layoutKey('user-a', 'org-b'))).toBeNull()
  })

  it('shows the default when the remembered value is invalid', () => {
    for (const raw of ['{broken', '"pinned"', JSON.stringify({ pinnedOnly: 'true' })]) {
      localStorage.setItem(layoutKey('user-a'), raw)
      const view = render(() => <DashboardRecentWork />)
      expect(pinnedButton().getAttribute('aria-pressed')).toBe('false')
      view.unmount()
    }
  })

  it('does not read or write the filter until the workspace is known', () => {
    setIdentity((state) => { state.activeOrg.id = '' }); flush()
    const write = vi.spyOn(Storage.prototype, 'setItem')
    render(() => <DashboardRecentWork />)
    fireEvent.click(pinnedButton()); flush()
    expect(pinnedButton().getAttribute('aria-pressed')).toBe('true')
    expect(write).not.toHaveBeenCalled()
  })
})
