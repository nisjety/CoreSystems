// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createStore, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ScheduleItem } from '@/shared/api/ingestions-client'

let language: 'no' | 'en' = 'en'
vi.mock('@/shared/api/ingestions-client', () => ({ listIngestionRuns: vi.fn(), listIngestionSchedules: vi.fn() }))
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (no: string, en: string) => (language === 'no' ? no : en), locale: () => language }) }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))
import { listIngestionSchedules } from '@/shared/api/ingestions-client'
import { DashboardCrawlSchedules } from './DashboardCrawlSchedules'

const [identity, setIdentity] = createStore({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const schedule = (id: string, name: string, nextRunAt: string | null | undefined, extra: Partial<ScheduleItem> = {}) => ({ id, name, kind: 'crawl', status: 'active', cron: '0 7 * * *', createdAt: '2026-09-01T12:00:00Z', nextRunAt, target: `https://${id}.example`, config: {}, ...extra }) as ScheduleItem
// Long enough for a rejection to pass through .then and .catch.
const settle = () => new Promise((resolve) => setTimeout(resolve, 0))
const rows = (container: HTMLElement) => [...container.querySelectorAll<HTMLAnchorElement>('.home-recent__row')]
const day = (value: string, locale: string) => new Date(value).toLocaleDateString(locale, { day: 'numeric', month: 'short' })

beforeEach(() => {
  language = 'en'
  vi.mocked(listIngestionSchedules).mockReset()
  setIdentity((state) => { state.user.id = 'user-a'; state.activeOrg.id = 'org-a' })
  flush()
})
afterEach(() => vi.restoreAllMocks())

describe('Hent ut scheduled ingestions card', () => {
  it('shows the soonest schedules first with kind, status and next run, and at most four', async () => {
    vi.mocked(listIngestionSchedules).mockResolvedValue([
      schedule('s1', 'Later crawl', '2026-10-09T12:00:00Z'),
      schedule('s2', 'Paused docs', null, { status: 'paused', kind: 'extract' }),
      schedule('s3', 'Soonest crawl', '2026-10-01T12:00:00Z'),
      schedule('s4', 'Prices', '2026-10-05T12:00:00Z', { kind: 'batch' }),
      schedule('s5', 'Furthest', '2026-11-20T12:00:00Z'),
      schedule('s6', 'Newer paused', undefined, { status: 'paused', createdAt: '2026-09-20T12:00:00Z' }),
    ])
    const view = render(() => <DashboardCrawlSchedules />)
    await waitFor(() => expect(rows(view.container)).toHaveLength(4))
    expect(rows(view.container).map((row) => row.querySelector('strong')?.textContent)).toEqual(['Soonest crawl', 'Prices', 'Later crawl', 'Furthest'])
    expect(rows(view.container).map((row) => row.querySelector('small')?.textContent)).toEqual([
      'Crawl · Active', 'Batch · Active', 'Crawl · Active', 'Crawl · Active',
    ])
    const next = rows(view.container)[0]!.querySelector('time')!
    expect(next.getAttribute('datetime')).toBe('2026-10-01T12:00:00Z')
    expect(next.getAttribute('title')).toBe('Next run')
    expect(next.textContent).toBe(day('2026-10-01T12:00:00Z', 'en-GB'))
    expect(screen.queryByText('Paused docs')).toBeNull()
    expect(view.container.querySelector('section')?.getAttribute('aria-busy')).toBe('false')
  })

  it('lists a schedule with no next run after the ones with one, without a date', async () => {
    vi.mocked(listIngestionSchedules).mockResolvedValue([
      schedule('s1', 'Paused docs', null, { status: 'paused', kind: 'extract' }),
      schedule('s2', 'Weekly crawl', '2026-10-01T12:00:00Z'),
    ])
    const view = render(() => <DashboardCrawlSchedules />)
    await waitFor(() => expect(rows(view.container)).toHaveLength(2))
    const [weekly, paused] = rows(view.container)
    expect(weekly!.querySelector('time')).not.toBeNull()
    expect(paused!.querySelector('strong')?.textContent).toBe('Paused docs')
    expect(paused!.querySelector('small')?.textContent).toBe('Extract · Paused')
    expect(paused!.querySelector('time')).toBeNull()
  })

  it('names an unnamed schedule after its target and shows a status it does not know as sent', async () => {
    vi.mocked(listIngestionSchedules).mockResolvedValue([schedule('s1', '', '2026-10-01T12:00:00Z', { status: 'needs_review' })])
    const view = render(() => <DashboardCrawlSchedules />)
    await waitFor(() => expect(rows(view.container)).toHaveLength(1))
    expect(rows(view.container)[0]!.querySelector('strong')?.textContent).toBe('https://s1.example')
    expect(rows(view.container)[0]!.querySelector('small')?.textContent).toBe('Crawl · needs review')
  })

  it('links every row and the footer to the ingestions page and never edits a schedule', async () => {
    vi.mocked(listIngestionSchedules).mockResolvedValue([schedule('s1', 'One', '2026-10-01T12:00:00Z'), schedule('s2', 'Two', '2026-10-02T12:00:00Z')])
    const view = render(() => <DashboardCrawlSchedules />)
    await waitFor(() => expect(rows(view.container)).toHaveLength(2))
    expect(rows(view.container).map((row) => row.getAttribute('href'))).toEqual(['/ingestions', '/ingestions'])
    expect(rows(view.container).every((row) => row.hasAttribute('link'))).toBe(true)
    const footer = screen.getByRole('link', { name: /All schedules/ })
    expect(footer.getAttribute('href')).toBe('/ingestions')
    expect(footer.closest('footer')).not.toBeNull()
    expect(screen.queryByRole('button', { name: /pause|resume|trigger|retire/i })).toBeNull()
  })

  it('takes the narrow weather slot: recent-work markup plus the schedules placement class', () => {
    vi.mocked(listIngestionSchedules).mockResolvedValue([])
    const view = render(() => <DashboardCrawlSchedules />)
    const section = view.container.querySelector('section')!
    expect(section.className).toBe('home-overview-card home-recent home-recent--schedules')
    expect(section.getAttribute('aria-labelledby')).toBe('home-crawl-schedules-title')
    expect(screen.getByRole('heading', { name: 'Scheduled ingestions' }).id).toBe('home-crawl-schedules-title')
    expect(section.querySelector('header')?.className).toBe('home-overview-card__header')
    expect(section.querySelector('.home-eyebrow')?.textContent).toBe('Recurring')
  })

  it('says it is loading until the schedules arrive', async () => {
    let resolve: (value: ScheduleItem[]) => void = () => undefined
    vi.mocked(listIngestionSchedules).mockImplementation(() => new Promise((next) => { resolve = next }))
    const view = render(() => <DashboardCrawlSchedules />)
    expect(screen.getByText('Loading your schedules…').getAttribute('role')).toBe('status')
    expect(view.container.querySelector('section')?.getAttribute('aria-busy')).toBe('true')
    expect(screen.queryByText('No scheduled ingestions yet.')).toBeNull()
    resolve([schedule('s1', 'Weekly crawl', '2026-10-01T12:00:00Z')])
    await waitFor(() => expect(screen.getByText('Weekly crawl')).toBeTruthy())
    expect(screen.queryByText('Loading your schedules…')).toBeNull()
    expect(view.container.querySelector('section')?.getAttribute('aria-busy')).toBe('false')
  })

  it('shows an empty state, not a made-up schedule, when there are none', async () => {
    vi.mocked(listIngestionSchedules).mockResolvedValue([])
    const view = render(() => <DashboardCrawlSchedules />)
    await waitFor(() => expect(screen.getByText('No scheduled ingestions yet.')).toBeTruthy())
    expect(screen.getByText('Schedules are created under Ingestions.')).toBeTruthy()
    expect(rows(view.container)).toHaveLength(0)
    expect(screen.queryByRole('button', { name: 'Try again' })).toBeNull()
  })

  it('shows a recoverable error rather than an empty list, and retry reloads', async () => {
    vi.mocked(listIngestionSchedules).mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce([schedule('s1', 'Back again', '2026-10-01T12:00:00Z')])
    const view = render(() => <DashboardCrawlSchedules />)
    await waitFor(() => expect(screen.getByText('Schedules could not be loaded.')).toBeTruthy())
    expect(screen.getByText('Schedules could not be loaded.').closest('[role="status"]')).not.toBeNull()
    expect(screen.queryByText('No scheduled ingestions yet.')).toBeNull()
    expect(screen.queryByText('offline')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }))
    flush()
    await waitFor(() => expect(rows(view.container)).toHaveLength(1))
    expect(screen.queryByText('Schedules could not be loaded.')).toBeNull()
    expect(listIngestionSchedules).toHaveBeenCalledTimes(2)
  })

  it('treats a response that is not a list as an error', async () => {
    vi.mocked(listIngestionSchedules).mockResolvedValue(null as unknown as ScheduleItem[])
    render(() => <DashboardCrawlSchedules />)
    await waitFor(() => expect(screen.getByText('Schedules could not be loaded.')).toBeTruthy())
  })

  it('ignores an old workspace response after the workspace changes', async () => {
    let resolveOld: (value: ScheduleItem[]) => void = () => undefined
    vi.mocked(listIngestionSchedules).mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve }))
      .mockResolvedValueOnce([schedule('new', 'Current workspace crawl', '2026-10-01T12:00:00Z')])
    render(() => <DashboardCrawlSchedules />)
    flush()
    setIdentity((state) => { state.activeOrg.id = 'org-b' })
    flush()
    await waitFor(() => expect(screen.getByText('Current workspace crawl')).toBeTruthy())
    resolveOld([schedule('old', 'Old workspace crawl', '2026-10-01T11:00:00Z')])
    await Promise.resolve()
    flush()
    expect(screen.queryByText('Old workspace crawl')).toBeNull()
    expect(screen.getByText('Current workspace crawl')).toBeTruthy()
  })

  it('ignores an old user response, including a late failure, and aborts the old request', async () => {
    const signals: AbortSignal[] = []
    let rejectOld: (reason: Error) => void = () => undefined
    vi.mocked(listIngestionSchedules).mockImplementationOnce((signal) => { signals.push(signal!); return new Promise((_resolve, reject) => { rejectOld = reject }) })
      .mockImplementationOnce((signal) => { signals.push(signal!); return Promise.resolve([schedule('mine', 'My crawl', '2026-10-01T12:00:00Z')]) })
    render(() => <DashboardCrawlSchedules />)
    flush()
    setIdentity((state) => { state.user.id = 'user-b' })
    flush()
    await waitFor(() => expect(screen.getByText('My crawl')).toBeTruthy())
    expect(signals[0]!.aborted).toBe(true)
    expect(signals[1]!.aborted).toBe(false)
    rejectOld(new Error('late failure'))
    await settle()
    flush()
    expect(screen.queryByText('Schedules could not be loaded.')).toBeNull()
    expect(screen.getByText('My crawl')).toBeTruthy()
  })

  it('drops the old schedules at once when the workspace changes and shows loading for the new one', async () => {
    let resolveNew: (value: ScheduleItem[]) => void = () => undefined
    vi.mocked(listIngestionSchedules).mockResolvedValueOnce([schedule('a', 'Org A crawl', '2026-10-01T12:00:00Z')])
      .mockImplementationOnce(() => new Promise((resolve) => { resolveNew = resolve }))
    const view = render(() => <DashboardCrawlSchedules />)
    await waitFor(() => expect(screen.getByText('Org A crawl')).toBeTruthy())
    setIdentity((state) => { state.activeOrg.id = 'org-b' })
    flush()
    await waitFor(() => expect(screen.getByText('Loading your schedules…')).toBeTruthy())
    expect(screen.queryByText('Org A crawl')).toBeNull()
    resolveNew([])
    await waitFor(() => expect(screen.getByText('No scheduled ingestions yet.')).toBeTruthy())
    expect(rows(view.container)).toHaveLength(0)
  })

  it('does not ask the API until both the user and the workspace are known', async () => {
    setIdentity((state) => { state.user.id = '' })
    flush()
    vi.mocked(listIngestionSchedules).mockResolvedValue([schedule('a', 'Weekly crawl', '2026-10-01T12:00:00Z')])
    render(() => <DashboardCrawlSchedules />)
    flush()
    await Promise.resolve()
    expect(listIngestionSchedules).not.toHaveBeenCalled()
    expect(screen.queryByText('Loading your schedules…')).toBeNull()
    expect(screen.getByText('No scheduled ingestions yet.')).toBeTruthy()
    setIdentity((state) => { state.user.id = 'user-a' })
    flush()
    await waitFor(() => expect(screen.getByText('Weekly crawl')).toBeTruthy())
    expect(listIngestionSchedules).toHaveBeenCalledTimes(1)
  })

  it('speaks Norwegian first', async () => {
    language = 'no'
    vi.mocked(listIngestionSchedules).mockResolvedValue([schedule('a', 'Ukentlig crawl', '2026-10-01T12:00:00Z', { status: 'paused', kind: 'extract' })])
    const view = render(() => <DashboardCrawlSchedules />)
    expect(screen.getByText('Henter tidsplanene dine …')).toBeTruthy()
    await waitFor(() => expect(rows(view.container)).toHaveLength(1))
    expect(screen.getByRole('heading', { name: 'Planlagte innhentinger' })).toBeTruthy()
    expect(rows(view.container)[0]!.querySelector('small')?.textContent).toBe('Uttrekk · Pauset')
    expect(rows(view.container)[0]!.querySelector('time')?.textContent).toBe(day('2026-10-01T12:00:00Z', 'nb-NO'))
    expect(rows(view.container)[0]!.querySelector('time')?.getAttribute('title')).toBe('Neste kjøring')
    expect(screen.getByRole('link', { name: /Alle tidsplaner/ })).toBeTruthy()
    expect(screen.getByText('Neste kjøring først')).toBeTruthy()
  })
})
