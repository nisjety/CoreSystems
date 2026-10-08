// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createStore, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { RunItem } from '@/shared/api/ingestions-client'

let language: 'no' | 'en' = 'en'
vi.mock('@/shared/api/ingestions-client', () => ({ listIngestionRuns: vi.fn(), listIngestionSchedules: vi.fn() }))
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (no: string, en: string) => (language === 'no' ? no : en), locale: () => language }) }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))
import { listIngestionRuns } from '@/shared/api/ingestions-client'
import { DashboardCrawlRuns } from './DashboardCrawlRuns'

const [identity, setIdentity] = createStore({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const run = (id: string, target: string, createdAt: string, extra: Partial<RunItem> = {}) => ({ id, kind: 'crawl', status: 'completed', createdAt, target, progress: {}, stats: {}, ...extra }) as RunItem
// Long enough for a rejection to pass through .then and .catch.
const settle = () => new Promise((resolve) => setTimeout(resolve, 0))
const rows = (container: HTMLElement) => [...container.querySelectorAll<HTMLAnchorElement>('.home-recent__row')]
const day = (value: string, locale: string) => new Date(value).toLocaleDateString(locale, { day: 'numeric', month: 'short' })

beforeEach(() => {
  language = 'en'
  vi.mocked(listIngestionRuns).mockReset()
  setIdentity((state) => { state.user.id = 'user-a'; state.activeOrg.id = 'org-a' })
  flush()
})
afterEach(() => vi.restoreAllMocks())

describe('Hent ut latest ingestions card', () => {
  it('shows the newest runs first with their kind, status and date, and at most four', async () => {
    vi.mocked(listIngestionRuns).mockResolvedValue([
      run('r1', 'https://oldest.example', '2026-09-01T12:00:00Z'),
      run('r2', 'https://docs.example', '2026-09-27T12:00:00Z', { kind: 'extract', status: 'running' }),
      run('r3', 'https://third.example', '2026-09-10T12:00:00Z', { status: 'failed' }),
      run('r4', 'https://fourth.example', '2026-09-05T12:00:00Z'),
      run('r5', 'https://newest.example', '2026-09-28T12:00:00Z', { kind: 'scrape' }),
      run('r6', 'https://second-oldest.example', '2026-09-02T12:00:00Z'),
    ])
    const view = render(() => <DashboardCrawlRuns />)
    await waitFor(() => expect(rows(view.container)).toHaveLength(4))
    expect(rows(view.container).map((row) => row.querySelector('strong')?.textContent)).toEqual([
      'https://newest.example', 'https://docs.example', 'https://third.example', 'https://fourth.example',
    ])
    expect(rows(view.container).map((row) => row.querySelector('small')?.textContent)).toEqual([
      'Scrape · Completed', 'Extract · Running', 'Crawl · Failed', 'Crawl · Completed',
    ])
    const first = rows(view.container)[0]!.querySelector('time')!
    expect(first.getAttribute('datetime')).toBe('2026-09-28T12:00:00Z')
    expect(first.textContent).toBe(day('2026-09-28T12:00:00Z', 'en-GB'))
    expect(screen.queryByText('https://oldest.example')).toBeNull()
    expect(view.container.querySelector('section')?.getAttribute('aria-busy')).toBe('false')
    // Nothing on the card is a figure the API did not send.
    expect(view.container.textContent).not.toMatch(/\d+\s*%|success rate|total/i)
  })

  it('links every row and the footer to the ingestions page', async () => {
    vi.mocked(listIngestionRuns).mockResolvedValue([run('r1', 'https://a.example', '2026-09-27T12:00:00Z'), run('r2', 'https://b.example', '2026-09-26T12:00:00Z')])
    const view = render(() => <DashboardCrawlRuns />)
    await waitFor(() => expect(rows(view.container)).toHaveLength(2))
    expect(rows(view.container).map((row) => row.getAttribute('href'))).toEqual(['/ingestions', '/ingestions'])
    expect(rows(view.container).every((row) => row.hasAttribute('link'))).toBe(true)
    const footer = screen.getByRole('link', { name: /All runs/ })
    expect(footer.getAttribute('href')).toBe('/ingestions')
    expect(footer.closest('footer')).not.toBeNull()
  })

  it('labels the card and uses the recent-work card markup', async () => {
    vi.mocked(listIngestionRuns).mockResolvedValue([])
    const view = render(() => <DashboardCrawlRuns />)
    const section = view.container.querySelector('section')!
    expect(section.className).toBe('home-overview-card home-recent')
    expect(section.getAttribute('aria-labelledby')).toBe('home-crawl-runs-title')
    expect(screen.getByRole('heading', { name: 'Latest ingestions' }).id).toBe('home-crawl-runs-title')
    expect(section.querySelector('header')?.className).toBe('home-overview-card__header')
    expect(section.querySelector('.home-eyebrow')?.textContent).toBe('Extract')
    await waitFor(() => expect(screen.getByText('No ingestions yet.')).toBeTruthy())
  })

  it('says it is loading until the runs arrive', async () => {
    let resolve: (value: RunItem[]) => void = () => undefined
    vi.mocked(listIngestionRuns).mockImplementation(() => new Promise((next) => { resolve = next }))
    const view = render(() => <DashboardCrawlRuns />)
    expect(screen.getByText('Loading your ingestions…').getAttribute('role')).toBe('status')
    expect(view.container.querySelector('section')?.getAttribute('aria-busy')).toBe('true')
    expect(screen.queryByText('No ingestions yet.')).toBeNull()
    resolve([run('r1', 'https://a.example', '2026-09-27T12:00:00Z')])
    await waitFor(() => expect(screen.getByText('https://a.example')).toBeTruthy())
    expect(screen.queryByText('Loading your ingestions…')).toBeNull()
    expect(view.container.querySelector('section')?.getAttribute('aria-busy')).toBe('false')
  })

  it('shows an empty state, not a made-up run, when there are none', async () => {
    vi.mocked(listIngestionRuns).mockResolvedValue([])
    const view = render(() => <DashboardCrawlRuns />)
    await waitFor(() => expect(screen.getByText('No ingestions yet.')).toBeTruthy())
    expect(screen.getByText('Runs from Ingestions appear here.')).toBeTruthy()
    expect(rows(view.container)).toHaveLength(0)
    expect(screen.queryByRole('button', { name: 'Try again' })).toBeNull()
  })

  it('shows a recoverable error rather than an empty list, and retry reloads', async () => {
    vi.mocked(listIngestionRuns).mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce([run('r1', 'https://back.example', '2026-09-27T12:00:00Z')])
    const view = render(() => <DashboardCrawlRuns />)
    await waitFor(() => expect(screen.getByText('Ingestions could not be loaded.')).toBeTruthy())
    expect(screen.getByText('Ingestions could not be loaded.').closest('[role="status"]')).not.toBeNull()
    expect(screen.queryByText('No ingestions yet.')).toBeNull()
    expect(screen.queryByText('offline')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }))
    flush()
    await waitFor(() => expect(rows(view.container)).toHaveLength(1))
    expect(screen.queryByText('Ingestions could not be loaded.')).toBeNull()
    expect(listIngestionRuns).toHaveBeenCalledTimes(2)
  })

  it('can fail again on retry and stays retryable', async () => {
    vi.mocked(listIngestionRuns).mockRejectedValue(new Error('offline'))
    render(() => <DashboardCrawlRuns />)
    await waitFor(() => expect(screen.getByText('Ingestions could not be loaded.')).toBeTruthy())
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }))
    flush()
    await waitFor(() => expect(listIngestionRuns).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Try again' })).toBeTruthy())
  })

  it('treats a response that is not a list as an error', async () => {
    vi.mocked(listIngestionRuns).mockResolvedValue({ runs: [] } as unknown as RunItem[])
    render(() => <DashboardCrawlRuns />)
    await waitFor(() => expect(screen.getByText('Ingestions could not be loaded.')).toBeTruthy())
  })

  it('ignores an old workspace response after the workspace changes', async () => {
    let resolveOld: (value: RunItem[]) => void = () => undefined
    vi.mocked(listIngestionRuns).mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve }))
      .mockResolvedValueOnce([run('new', 'https://current.example', '2026-09-28T10:00:00Z')])
    render(() => <DashboardCrawlRuns />)
    flush()
    setIdentity((state) => { state.activeOrg.id = 'org-b' })
    flush()
    await waitFor(() => expect(screen.getByText('https://current.example')).toBeTruthy())
    resolveOld([run('old', 'https://old-workspace.example', '2026-09-28T11:00:00Z')])
    await Promise.resolve()
    flush()
    expect(screen.queryByText('https://old-workspace.example')).toBeNull()
    expect(screen.getByText('https://current.example')).toBeTruthy()
  })

  it('ignores an old user response, including a late failure, and aborts the old request', async () => {
    const signals: AbortSignal[] = []
    let rejectOld: (reason: Error) => void = () => undefined
    vi.mocked(listIngestionRuns).mockImplementationOnce((signal) => { signals.push(signal!); return new Promise((_resolve, reject) => { rejectOld = reject }) })
      .mockImplementationOnce((signal) => { signals.push(signal!); return Promise.resolve([run('mine', 'https://mine.example', '2026-09-28T10:00:00Z')]) })
    render(() => <DashboardCrawlRuns />)
    flush()
    expect(signals[0]!.aborted).toBe(false)
    setIdentity((state) => { state.user.id = 'user-b' })
    flush()
    await waitFor(() => expect(screen.getByText('https://mine.example')).toBeTruthy())
    expect(signals[0]!.aborted).toBe(true)
    expect(signals[1]!.aborted).toBe(false)
    rejectOld(new Error('late failure'))
    await settle()
    flush()
    expect(screen.queryByText('Ingestions could not be loaded.')).toBeNull()
    expect(screen.getByText('https://mine.example')).toBeTruthy()
  })

  it('drops the old runs at once when the workspace changes and shows loading for the new one', async () => {
    let resolveNew: (value: RunItem[]) => void = () => undefined
    vi.mocked(listIngestionRuns).mockResolvedValueOnce([run('a', 'https://org-a.example', '2026-09-28T10:00:00Z')])
      .mockImplementationOnce(() => new Promise((resolve) => { resolveNew = resolve }))
    const view = render(() => <DashboardCrawlRuns />)
    await waitFor(() => expect(screen.getByText('https://org-a.example')).toBeTruthy())
    setIdentity((state) => { state.activeOrg.id = 'org-b' })
    flush()
    await waitFor(() => expect(screen.getByText('Loading your ingestions…')).toBeTruthy())
    expect(screen.queryByText('https://org-a.example')).toBeNull()
    resolveNew([])
    await waitFor(() => expect(screen.getByText('No ingestions yet.')).toBeTruthy())
    expect(rows(view.container)).toHaveLength(0)
  })

  it('does not ask the API until both the user and the workspace are known', async () => {
    setIdentity((state) => { state.activeOrg.id = '' })
    flush()
    vi.mocked(listIngestionRuns).mockResolvedValue([run('a', 'https://a.example', '2026-09-28T10:00:00Z')])
    render(() => <DashboardCrawlRuns />)
    flush()
    await Promise.resolve()
    expect(listIngestionRuns).not.toHaveBeenCalled()
    expect(screen.queryByText('Loading your ingestions…')).toBeNull()
    expect(screen.getByText('No ingestions yet.')).toBeTruthy()
    setIdentity((state) => { state.activeOrg.id = 'org-a' })
    flush()
    await waitFor(() => expect(screen.getByText('https://a.example')).toBeTruthy())
    expect(listIngestionRuns).toHaveBeenCalledTimes(1)
  })

  it('copes with a run the API left half empty', async () => {
    vi.mocked(listIngestionRuns).mockResolvedValue([
      run('bare', '', 'not a date', { kind: 'extract', status: 'waiting_for_review' }),
      run('nothing', '', '2026-09-01T12:00:00Z', { kind: '', status: '' }),
    ])
    const view = render(() => <DashboardCrawlRuns />)
    await waitFor(() => expect(rows(view.container)).toHaveLength(2))
    const [nothing, bare] = rows(view.container)
    expect(nothing!.querySelector('strong')?.textContent).toBe('')
    expect(nothing!.querySelector('small')?.textContent).toBe('')
    expect(bare!.querySelector('strong')?.textContent).toBe('Extract')
    expect(bare!.querySelector('small')?.textContent).toBe('Extract · waiting for review')
    expect(bare!.querySelector('time')?.textContent).toBe('')
  })

  it('speaks Norwegian first', async () => {
    language = 'no'
    vi.mocked(listIngestionRuns).mockResolvedValue([run('a', 'https://a.example', '2026-09-28T12:00:00Z', { kind: 'extract' })])
    const view = render(() => <DashboardCrawlRuns />)
    expect(screen.getByText('Henter innhentingene dine …')).toBeTruthy()
    await waitFor(() => expect(rows(view.container)).toHaveLength(1))
    expect(screen.getByRole('heading', { name: 'Siste innhentinger' })).toBeTruthy()
    expect(rows(view.container)[0]!.querySelector('small')?.textContent).toBe('Uttrekk · Fullført')
    expect(rows(view.container)[0]!.querySelector('time')?.textContent).toBe(day('2026-09-28T12:00:00Z', 'nb-NO'))
    expect(screen.getByRole('link', { name: /Alle kjøringer/ })).toBeTruthy()
    expect(screen.getByText('Nyeste først')).toBeTruthy()
  })
})
