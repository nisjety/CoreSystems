// @vitest-environment jsdom
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, createStore, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatThreadSession } from '@/shared/api/chat-client'
import type { RunItem, ScheduleItem } from '@/shared/api/ingestions-client'
import type { DashboardTab } from './dashboard-home-types'
import { __resetRetentionForTests, applyServerRetention } from '@/features/chat/lib/chat-retention'
import { addWebHistory, webHistoryStorageKey } from './web-history'

vi.mock('@/shared/api/chat-client', () => ({ listChatThreads: vi.fn() }))
vi.mock('@/shared/api/ingestions-client', () => ({ listIngestionRuns: vi.fn(), listIngestionSchedules: vi.fn() }))
vi.mock('./DashboardCards', () => ({ DashboardImageCard: () => <section data-information-card="weather">Weather card</section> }))
vi.mock('./DashboardUpdatesCard', () => ({ DashboardUpdatesCard: () => <section data-testid="updates-card">Updates card</section> }))
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))
vi.mock('@solidjs/router', () => ({ useNavigate: () => () => undefined }))
import { listChatThreads } from '@/shared/api/chat-client'
import { listIngestionRuns, listIngestionSchedules } from '@/shared/api/ingestions-client'
import { DashboardOverview } from './DashboardOverview'

const [identity, setIdentity] = createStore({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const thread = { threadId: 'thread-1', title: 'Quarterly plan', preview: '', updatedAt: '2026-09-28T10:00:00Z', pinned: false } as ChatThreadSession
const ingestion = { id: 'run-1', kind: 'crawl', status: 'completed', createdAt: '2026-09-28T10:00:00Z', target: 'https://docs.example', progress: {}, stats: {} } as RunItem
const schedule = { id: 'sch-1', name: 'Weekly docs crawl', kind: 'crawl', status: 'active', createdAt: '2026-09-01T10:00:00Z', nextRunAt: '2026-10-01T10:00:00Z', target: 'https://docs.example', config: {} } as ScheduleItem
const seedWebHistory = () => {
  const key = webHistoryStorageKey('user-a', 'org-a')!
  addWebHistory(key, { kind: 'query', text: 'agent status' }, new Date('2026-09-29T09:00:00Z'))
  addWebHistory(key, { kind: 'page', text: 'Agent status overview', url: 'https://example.com/status' }, new Date('2026-09-29T10:00:00Z'))
}
const headings = () => screen.queryAllByRole('heading', { level: 2 }).map((heading) => heading.textContent)

function setup(initial: DashboardTab) {
  const [tab, setTab] = createSignal<DashboardTab>(initial)
  const onPrompt = vi.fn()
  const view = render(() => <DashboardOverview tab={tab()} onPrompt={onPrompt} />)
  const show = (next: DashboardTab) => { setTab(next); flush() }
  return { ...view, show, onPrompt }
}

beforeEach(() => {
  localStorage.clear(); sessionStorage.clear()
  vi.mocked(listChatThreads).mockReset().mockResolvedValue([thread])
  vi.mocked(listIngestionRuns).mockReset().mockResolvedValue([ingestion])
  vi.mocked(listIngestionSchedules).mockReset().mockResolvedValue([schedule])
  setIdentity((state) => { state.user.id = 'user-a'; state.activeOrg.id = 'org-a' })
  flush()
})
afterEach(() => {
  vi.restoreAllMocks()
  __resetRetentionForTests()
})

describe('Home overview follows the active tab', () => {
  it('keeps today\'s cards on Chat: recent work, weather, updates and tools', async () => {
    setup('Chat')
    await waitFor(() => expect(screen.getByText('Quarterly plan')).toBeTruthy())
    expect(headings()).toEqual(['Pick up where you left off', 'Ready for what’s next'])
    expect(screen.getByText('Weather card')).toBeTruthy()
    expect(screen.getByTestId('updates-card')).toBeTruthy()
    expect(screen.queryByText('Latest ingestions')).toBeNull()
    expect(screen.queryByText('Scheduled ingestions')).toBeNull()
    // Chat never touches the ingestions API.
    expect(listIngestionRuns).not.toHaveBeenCalled()
    expect(listIngestionSchedules).not.toHaveBeenCalled()
  })

  it('shows the last ingestions and scheduled ingestions on Hent ut instead of recent work and weather', async () => {
    const view = setup('Crawl')
    await waitFor(() => expect(screen.getByText('https://docs.example', { selector: 'strong' })).toBeTruthy())
    await waitFor(() => expect(screen.getByText('Weekly docs crawl')).toBeTruthy())
    expect(headings()).toEqual(['Latest ingestions', 'Scheduled ingestions', 'Ready for what’s next'])
    expect(screen.queryByText('Pick up where you left off')).toBeNull()
    expect(screen.queryByText('Quarterly plan')).toBeNull()
    expect(screen.queryByText('Weather card')).toBeNull()
    expect(view.container.querySelector('[data-information-card="weather"]')).toBeNull()
    // Hent ut never loads conversations, and the updates and tools cards are still there.
    expect(listChatThreads).not.toHaveBeenCalled()
    expect(screen.getByTestId('updates-card')).toBeTruthy()
    expect(screen.getByRole('navigation', { name: 'Workspace shortcuts' })).toBeTruthy()
  })

  it('places the two Hent ut cards in the slots recent work and weather use, ahead of updates and tools', async () => {
    const view = setup('Crawl')
    await waitFor(() => expect(screen.getByText('Weekly docs crawl')).toBeTruthy())
    const grid = view.container.querySelector('.home-overview-grid')!
    expect([...grid.children].map((child) => child.getAttribute('aria-labelledby') ?? child.getAttribute('data-testid'))).toEqual([
      'home-crawl-runs-title', 'home-crawl-schedules-title', 'updates-card', 'home-destinations-title',
    ])
    expect(grid.querySelectorAll(':scope > .home-recent')).toHaveLength(2)
    expect(grid.querySelectorAll(':scope > .home-recent--schedules')).toHaveLength(1)
  })

  it('shows recent searches and opened pages on Nett in the recent-work slot, and keeps the weather, updates and tools cards', () => {
    seedWebHistory()
    const view = setup('Søk')
    expect(headings()).toEqual(['Pick up where you left off', 'Ready for what’s next'])
    expect(view.container.querySelector('#home-web-history-title')).not.toBeNull()
    expect(view.container.querySelector('#home-recent-title')).toBeNull()
    expect(screen.getByText('Agent status overview', { selector: 'strong' })).toBeTruthy()
    expect(screen.getByText('agent status', { selector: 'strong' })).toBeTruthy()
    expect(screen.getByText('Weather card')).toBeTruthy()
    expect(screen.getByTestId('updates-card')).toBeTruthy()
    expect(screen.getByRole('navigation', { name: 'Workspace shortcuts' })).toBeTruthy()
    // Nett shows no conversations and no ingestions.
    expect(screen.queryByText('Quarterly plan')).toBeNull()
    expect(listChatThreads).not.toHaveBeenCalled()
    expect(listIngestionRuns).not.toHaveBeenCalled()
    expect(listIngestionSchedules).not.toHaveBeenCalled()
  })

  it('places the Nett card in the recent-work slot, ahead of the weather, updates and tools cards', () => {
    const view = setup('Søk')
    const grid = view.container.querySelector('.home-overview-grid')!
    expect([...grid.children].map((child) => child.getAttribute('aria-labelledby') ?? child.getAttribute('data-information-card') ?? child.getAttribute('data-testid'))).toEqual([
      'home-web-history-title', 'weather', 'updates-card', 'home-destinations-title',
    ])
    expect(grid.querySelectorAll(':scope > .home-recent')).toHaveLength(1)
    expect(grid.querySelector(':scope > .home-recent')?.className).toBe('home-overview-card home-recent')
  })

  it('shows the empty Nett card while nothing has been searched, without touching the network', () => {
    setup('Søk')
    expect(screen.getByText('No searches yet.')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Clear' })).toBeNull()
    expect(listChatThreads).not.toHaveBeenCalled()
  })

  it('keeps recent work on Nett in a zero-data-retention workspace, where nothing is remembered', async () => {
    seedWebHistory()
    applyServerRetention(true)
    const view = setup('Søk')
    await waitFor(() => expect(screen.getByText('Quarterly plan')).toBeTruthy())
    expect(view.container.querySelector('#home-web-history-title')).toBeNull()
    expect(screen.queryByText('No searches yet.')).toBeNull()
    expect(screen.queryByText('agent status')).toBeNull()
    expect(screen.getByText('Weather card')).toBeTruthy()
    expect(screen.getByTestId('updates-card')).toBeTruthy()
  })

  it("follows the workspace's verdict while the Nett tab is showing", async () => {
    seedWebHistory()
    const view = setup('Søk')
    expect(view.container.querySelector('#home-web-history-title')).not.toBeNull()
    const updates = screen.getByTestId('updates-card')

    applyServerRetention(true); flush()
    await waitFor(() => expect(screen.getByText('Quarterly plan')).toBeTruthy())
    expect(view.container.querySelector('#home-web-history-title')).toBeNull()
    expect(screen.getByTestId('updates-card')).toBe(updates)

    applyServerRetention(false); flush()
    await waitFor(() => expect(view.container.querySelector('#home-web-history-title')).not.toBeNull())
    expect(view.container.querySelector('#home-recent-title')).toBeNull()
    // What was remembered before the verdict was removed with it.
    expect(screen.getByText('No searches yet.')).toBeTruthy()
  })

  it('keeps the Nett history off the Chat and Hent ut tabs', async () => {
    seedWebHistory()
    const view = setup('Chat')
    await waitFor(() => expect(screen.getByText('Quarterly plan')).toBeTruthy())
    expect(screen.queryByText('agent status')).toBeNull()
    expect(view.container.querySelector('#home-web-history-title')).toBeNull()
    view.show('Crawl')
    await waitFor(() => expect(screen.getByText('Weekly docs crawl')).toBeTruthy())
    expect(screen.queryByText('agent status')).toBeNull()
    expect(view.container.querySelector('#home-web-history-title')).toBeNull()
  })

  it('swaps between conversations and the Nett history as the tab changes, without reloading updates or tools', async () => {
    seedWebHistory()
    const view = setup('Chat')
    await waitFor(() => expect(screen.getByText('Quarterly plan')).toBeTruthy())
    const updates = screen.getByTestId('updates-card')
    const tools = view.container.querySelector('.home-destinations')!

    view.show('Søk')
    expect(screen.getByText('agent status', { selector: 'strong' })).toBeTruthy()
    expect(screen.queryByText('Quarterly plan')).toBeNull()
    expect(screen.getByText('Weather card')).toBeTruthy()
    expect(screen.getByTestId('updates-card')).toBe(updates)
    expect(view.container.querySelector('.home-destinations')).toBe(tools)

    view.show('Chat')
    await waitFor(() => expect(screen.getByText('Quarterly plan')).toBeTruthy())
    expect(screen.queryByText('agent status')).toBeNull()
    expect(screen.getByTestId('updates-card')).toBe(updates)
  })

  it('swaps the first two cards when the tab changes and keeps the updates and tools cards mounted', async () => {
    const view = setup('Chat')
    await waitFor(() => expect(screen.getByText('Quarterly plan')).toBeTruthy())
    const updates = screen.getByTestId('updates-card')
    const tools = view.container.querySelector('.home-destinations')!

    view.show('Crawl')
    await waitFor(() => expect(screen.getByText('Weekly docs crawl')).toBeTruthy())
    expect(screen.queryByText('Pick up where you left off')).toBeNull()
    expect(screen.queryByText('Weather card')).toBeNull()
    expect(screen.getByText('Latest ingestions')).toBeTruthy()
    expect(screen.getByTestId('updates-card')).toBe(updates)
    expect(view.container.querySelector('.home-destinations')).toBe(tools)

    view.show('Chat')
    await waitFor(() => expect(screen.getByText('Quarterly plan')).toBeTruthy())
    expect(screen.queryByText('Latest ingestions')).toBeNull()
    expect(screen.queryByText('Scheduled ingestions')).toBeNull()
    expect(screen.getByText('Weather card')).toBeTruthy()
    expect(screen.getByTestId('updates-card')).toBe(updates)
    expect(view.container.querySelector('.home-destinations')).toBe(tools)
  })

  it('loads each tab\'s data only while that tab is showing', async () => {
    const view = setup('Chat')
    await waitFor(() => expect(listChatThreads).toHaveBeenCalledTimes(1))
    expect(listIngestionRuns).not.toHaveBeenCalled()
    view.show('Crawl')
    await waitFor(() => expect(listIngestionRuns).toHaveBeenCalledTimes(1))
    expect(listIngestionSchedules).toHaveBeenCalledTimes(1)
    view.show('Søk')
    // Nett reads its history from the browser: no conversations, no ingestions.
    expect(screen.getByText('No searches yet.')).toBeTruthy()
    expect(listChatThreads).toHaveBeenCalledTimes(1)
    expect(listIngestionRuns).toHaveBeenCalledTimes(1)
    view.show('Chat')
    await waitFor(() => expect(screen.getByText('Quarterly plan')).toBeTruthy())
    expect(listChatThreads).toHaveBeenCalledTimes(2)
  })

  it('answers a workspace switch made on the Hent ut tab with that workspace\'s data only', async () => {
    let resolveOld: (value: RunItem[]) => void = () => undefined
    vi.mocked(listIngestionRuns).mockReset()
      .mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve }))
      .mockResolvedValueOnce([{ ...ingestion, id: 'run-b', target: 'https://org-b.example' }])
    setup('Crawl')
    flush()
    setIdentity((state) => { state.activeOrg.id = 'org-b' })
    flush()
    await waitFor(() => expect(screen.getByText('https://org-b.example')).toBeTruthy())
    resolveOld([{ ...ingestion, id: 'run-a', target: 'https://org-a.example' }])
    await Promise.resolve()
    flush()
    expect(screen.queryByText('https://org-a.example')).toBeNull()
    expect(screen.getByText('https://org-b.example')).toBeTruthy()
  })

  it('gives the schedules card the weather card\'s grid placement at every breakpoint', () => {
    const css = readFileSync(resolve(process.cwd(), 'src/features/dashboard/home/dashboard-overview.css'), 'utf8')
    const spans = (breakpoint: 'base' | 'narrow') => {
      const scope = breakpoint === 'narrow' ? css.slice(css.indexOf('@media (max-width: 850px)'), css.indexOf('@media (max-width: 640px)')) : css.slice(0, css.indexOf('@media (max-width: 1200px)'))
      const rule = scope.split('\n').find((line) => line.includes("[data-information-card='weather']") && line.includes('grid-column'))
      return { selectors: rule?.slice(0, rule.indexOf('{')).trim(), span: rule?.match(/grid-column:\s*span\s*(\d+)/)?.[1] }
    }
    expect(spans('base')).toEqual({ selectors: ".home-overview-grid [data-information-card='weather'], .dashboard-home--overview .home-recent--schedules", span: '4' })
    expect(spans('narrow')).toEqual({ selectors: ".home-overview-grid [data-information-card='weather'], .dashboard-home--overview .home-recent--schedules", span: '5' })
  })
})
