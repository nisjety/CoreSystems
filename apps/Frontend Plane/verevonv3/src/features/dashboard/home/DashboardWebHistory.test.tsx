// @vitest-environment jsdom
import { fireEvent, render, screen } from '@solidjs/testing-library'
import { createStore, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))
import { DashboardWebHistory } from './DashboardWebHistory'
import { query, registerSearchRunner, setQuery } from './dashboard-search-session'
import { addWebHistory, recordWebPage, recordWebQuery, webHistoryStorageKey } from './web-history'

type Identity = { user: { id: string } | null; activeOrg: { id: string } | null }
const [identity, setIdentity] = createStore<Identity>({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const signIn = (user: string | null, org: string | null) => {
  setIdentity((state) => { state.user = user ? { id: user } : null; state.activeOrg = org ? { id: org } : null })
  flush()
}
const keyA = webHistoryStorageKey('user-a', 'org-a')!
const keyB = webHistoryStorageKey('user-b', 'org-a')!
const at = (minute: number) => new Date(Date.UTC(2026, 8, 29, 10, minute))
const search = (text: string, minute: number, key = keyA) => addWebHistory(key, { kind: 'query', text }, at(minute))
const visited = (url: string, title: string | null, minute: number, key = keyA) => addWebHistory(key, { kind: 'page', text: title, url }, at(minute))
const rows = (container: HTMLElement) => [...container.querySelectorAll<HTMLElement>('.home-recent__row')]
const clearButton = () => screen.queryByRole('button', { name: 'Clear' })
const unregister: Array<() => void> = []
// The test setup swaps in plain in-memory storage objects, so spies go on those objects (both backends are written).
const spyOnStorage = (method: 'getItem' | 'setItem') => {
  const spies = [vi.spyOn(localStorage, method), vi.spyOn(sessionStorage, method)]
  return { calls: () => spies.flatMap((spy) => spy.mock.calls) }
}

beforeEach(() => {
  localStorage.clear(); sessionStorage.clear()
  setQuery('')
  signIn('user-a', 'org-a')
})
afterEach(() => {
  for (const off of unregister.splice(0)) off()
  vi.restoreAllMocks()
})

describe('Nett history card', () => {
  it('uses the recent-work card markup, with its own heading, and says where the history lives', () => {
    const view = render(() => <DashboardWebHistory />)
    const card = view.container.querySelector('section')!
    expect(card.className).toBe('home-overview-card home-recent')
    expect(card.getAttribute('aria-labelledby')).toBe('home-web-history-title')
    expect(screen.getByRole('heading', { level: 2 }).textContent).toBe('Pick up where you left off')
    expect(card.querySelector('.home-eyebrow')?.textContent).toBe('Web')
    expect(card.querySelector(':scope > header.home-overview-card__header')).not.toBeNull()
    expect(card.querySelector(':scope > footer')?.textContent).toContain('Saved in this browser only')
  })

  it('explains that searches will appear here when there is nothing yet, without a Clear button', () => {
    const view = render(() => <DashboardWebHistory />)
    expect(view.container.querySelector('.home-recent__empty')).not.toBeNull()
    expect(screen.getByText('No searches yet.')).toBeTruthy()
    expect(screen.getByText('Your searches and the pages you open from results will appear here.')).toBeTruthy()
    expect(rows(view.container)).toHaveLength(0)
    expect(clearButton()).toBeNull()
  })

  it('lists searches and opened pages newest first, in the recent-work row markup', () => {
    search('quarterly report', 1)
    visited('https://www.example.com/status', 'Agent status overview', 2)
    search('agent status', 3)
    const view = render(() => <DashboardWebHistory />)
    const [first, second, third] = rows(view.container) as [HTMLElement, HTMLElement, HTMLElement]
    expect(rows(view.container)).toHaveLength(3)

    expect(first.tagName).toBe('BUTTON')
    expect(first.querySelector('strong')?.textContent).toBe('agent status')
    expect(first.querySelector('small')?.textContent).toBe('Web search')
    expect(first.querySelector('time')?.getAttribute('datetime')).toBe(at(3).toISOString())
    expect(first.querySelector('.home-recent__icon')).not.toBeNull()
    expect(first.querySelector('.home-recent__copy')).not.toBeNull()

    expect(second.tagName).toBe('A')
    expect(second.querySelector('strong')?.textContent).toBe('Agent status overview')
    expect(second.querySelector('small')?.textContent).toBe('example.com')
    expect(third.querySelector('strong')?.textContent).toBe('quarterly report')
  })

  it('opens a page row in a new tab without leaking the opener, and never as an in-app link', () => {
    visited('https://alice:pw@example.com/status?tab=1', 'Status', 1)
    const view = render(() => <DashboardWebHistory />)
    const link = rows(view.container)[0] as HTMLAnchorElement
    expect(link.getAttribute('href')).toBe('https://example.com/status?tab=1')
    expect(link.getAttribute('target')).toBe('_blank')
    expect(link.getAttribute('rel')).toBe('noopener noreferrer')
    expect(link.hasAttribute('link')).toBe(false)
    // The card is a shortcut; opening a page from it changes nothing in the history.
    link.addEventListener('click', (event) => event.preventDefault())
    const before = localStorage.getItem(keyA)
    fireEvent.click(link); flush()
    expect(localStorage.getItem(keyA)).toBe(before)
  })

  it('shows at most four rows and keeps room for both kinds', () => {
    search('older search', 0)
    for (let index = 1; index <= 5; index += 1) visited(`https://example.com/${index}`, `Page ${index}`, index)
    const view = render(() => <DashboardWebHistory />)
    expect(rows(view.container).map((row) => row.querySelector('strong')?.textContent)).toEqual(['Page 5', 'Page 4', 'Page 3', 'older search'])
  })

  it('renders text as text and drops entries that are not safe links', () => {
    localStorage.setItem(keyA, JSON.stringify([
      { kind: 'query', text: '<img src=x onerror=alert(1)>', at: at(3).toISOString() },
      { kind: 'page', text: 'Sneaky', url: 'javascript:alert(document.cookie)', at: at(2).toISOString() },
      { kind: 'page', text: 'Fine', url: 'https://example.com/fine', at: at(1).toISOString() },
    ]))
    const view = render(() => <DashboardWebHistory />)
    expect(view.container.querySelector('img')).toBeNull()
    expect(screen.getByText('<img src=x onerror=alert(1)>')).toBeTruthy()
    expect(screen.queryByText('Sneaky')).toBeNull()
    expect(view.container.querySelector('a[href^="javascript"]')).toBeNull()
    expect(rows(view.container)).toHaveLength(2)
  })
})

describe('searching again from the card', () => {
  it('runs the search through the panel\'s own flow when a panel is mounted', () => {
    search('agent status', 1)
    const runner = vi.fn()
    unregister.push(registerSearchRunner(runner))
    const view = render(() => <DashboardWebHistory />)
    fireEvent.click(rows(view.container)[0]!); flush()
    expect(runner).toHaveBeenCalledTimes(1)
    expect(runner).toHaveBeenCalledWith('agent status', { focus: true })
    // The panel owns the query; the card does not touch it.
    expect(query()).toBe('')
  })

  it('asks for focus on the results page only when the row was pressed from the keyboard', () => {
    search('agent status', 1)
    const runner = vi.fn()
    unregister.push(registerSearchRunner(runner))
    const view = render(() => <DashboardWebHistory />)
    // A keyboard press is a click with no pointer (detail 0); a mouse or touch press has one.
    fireEvent.click(rows(view.container)[0]!, { detail: 1 }); flush()
    fireEvent.click(rows(view.container)[0]!, { detail: 0 }); flush()
    expect(runner.mock.calls).toEqual([
      ['agent status', { focus: false }],
      ['agent status', { focus: true }],
    ])
  })

  it('runs the search that belongs to the clicked row', () => {
    search('first', 1); search('second', 2)
    const runner = vi.fn()
    unregister.push(registerSearchRunner(runner))
    const view = render(() => <DashboardWebHistory />)
    fireEvent.click(rows(view.container)[1]!); flush()
    expect(runner).toHaveBeenLastCalledWith('first', { focus: true })
  })

  it('puts the search in the box and focuses it when no panel is mounted', () => {
    search('agent status', 1)
    const input = document.createElement('input')
    input.id = 'dashboard-search'
    document.body.append(input)
    const view = render(() => <DashboardWebHistory />)
    fireEvent.click(rows(view.container)[0]!); flush()
    expect(query()).toBe('agent status')
    expect(document.activeElement).toBe(input)
    input.remove()
  })

  it('does not fail when there is no search box either', () => {
    search('agent status', 1)
    const view = render(() => <DashboardWebHistory />)
    expect(() => { fireEvent.click(rows(view.container)[0]!); flush() }).not.toThrow()
    expect(query()).toBe('agent status')
  })

  it('does not run a search from a page row', () => {
    visited('https://example.com/a', 'A', 1)
    const runner = vi.fn()
    unregister.push(registerSearchRunner(runner))
    const view = render(() => <DashboardWebHistory />)
    fireEvent.click(rows(view.container)[0]!); flush()
    expect(runner).not.toHaveBeenCalled()
  })
})

describe('clearing the history', () => {
  it('removes this account\'s record and shows the empty state', () => {
    search('mine', 1); visited('https://example.com/a', 'A', 2)
    search('theirs', 3, keyB)
    const view = render(() => <DashboardWebHistory />)
    expect(rows(view.container)).toHaveLength(2)
    fireEvent.click(clearButton()!); flush()
    expect(rows(view.container)).toHaveLength(0)
    expect(screen.getByText('No searches yet.')).toBeTruthy()
    expect(clearButton()).toBeNull()
    expect(localStorage.getItem(keyA)).toBeNull()
    expect(localStorage.getItem(keyB)).not.toBeNull()
  })

  it('uses the quiet header button the Pinned filter uses', () => {
    search('mine', 1)
    render(() => <DashboardWebHistory />)
    const button = clearButton()!
    expect(button.className).toBe('home-quiet-button')
    expect(button.closest('header.home-overview-card__header')).not.toBeNull()
    expect(button.getAttribute('type')).toBe('button')
  })

  it('starts a fresh history after it was cleared', () => {
    search('mine', 1)
    const view = render(() => <DashboardWebHistory />)
    fireEvent.click(clearButton()!); flush()
    recordWebQuery('after the clear'); flush()
    expect(rows(view.container).map((row) => row.querySelector('strong')?.textContent)).toEqual(['after the clear'])
    expect(clearButton()).not.toBeNull()
  })
})

describe('keyboard and screen-reader use after clearing', () => {
  it('moves focus from the Clear button that disappears to the card heading, not the top of the page', () => {
    search('mine', 1)
    render(() => <DashboardWebHistory />)
    const button = clearButton()!
    button.focus()
    expect(document.activeElement).toBe(button)

    fireEvent.click(button); flush()

    expect(clearButton()).toBeNull()
    const heading = screen.getByRole('heading', { level: 2 })
    expect(document.activeElement).toBe(heading)
    // Focusable by script only: it is not a new stop when tabbing through the card.
    expect(heading.getAttribute('tabindex')).toBe('-1')
  })

  it('says what happened in a status line that is already on the page before it is needed', () => {
    search('mine', 1)
    const view = render(() => <DashboardWebHistory />)
    const status = screen.getByRole('status')
    expect(status.textContent).toBe('')
    expect(status.className).toBe('sr-only')
    expect(view.container.contains(status)).toBe(true)

    fireEvent.click(clearButton()!); flush()

    expect(screen.getByRole('status')).toBe(status)
    expect(status.textContent).toBe('The searches and pages were removed.')
  })

  it('says it again after the next clearing: the line is emptied once something is remembered again', () => {
    search('mine', 1)
    render(() => <DashboardWebHistory />)
    const status = screen.getByRole('status')
    fireEvent.click(clearButton()!); flush()
    expect(status.textContent).not.toBe('')

    recordWebQuery('after the clear'); flush()
    expect(status.textContent).toBe('')

    fireEvent.click(clearButton()!); flush()
    expect(status.textContent).toBe('The searches and pages were removed.')
  })

  it('announces nothing and takes no focus just by being shown', () => {
    search('mine', 1)
    render(() => <DashboardWebHistory />)
    expect(screen.getByRole('status').textContent).toBe('')
    expect(document.activeElement).toBe(document.body)
  })
})

describe('following the account', () => {
  it('shows a search or page as soon as one is recorded', () => {
    const view = render(() => <DashboardWebHistory />)
    expect(rows(view.container)).toHaveLength(0)
    recordWebQuery('agent status'); flush()
    recordWebPage({ url: 'https://example.com/status', title: 'Status' }); flush()
    expect(rows(view.container).map((row) => row.querySelector('strong')?.textContent)).toEqual(['Status', 'agent status'])
  })

  it('shows each person and workspace only their own history', () => {
    search('as user a', 1)
    search('as user b', 2, keyB)
    const view = render(() => <DashboardWebHistory />)
    const texts = () => rows(view.container).map((row) => row.querySelector('strong')?.textContent)
    expect(texts()).toEqual(['as user a'])
    signIn('user-b', 'org-a')
    expect(texts()).toEqual(['as user b'])
    signIn('user-b', 'org-b')
    expect(texts()).toEqual([])
    expect(screen.getByText('No searches yet.')).toBeTruthy()
    signIn('user-a', 'org-a')
    expect(texts()).toEqual(['as user a'])
  })

  it('shows the empty state, with no Clear button, while the account is unknown', () => {
    search('kept', 1)
    signIn(null, null)
    const view = render(() => <DashboardWebHistory />)
    expect(rows(view.container)).toHaveLength(0)
    expect(clearButton()).toBeNull()
    expect(localStorage.getItem(keyA)).not.toBeNull()
  })

  it('does not write anything just by showing the card', () => {
    search('kept', 1)
    const write = spyOnStorage('setItem')
    render(() => <DashboardWebHistory />)
    signIn('user-b', 'org-a')
    expect(write.calls()).toEqual([])
  })
})
