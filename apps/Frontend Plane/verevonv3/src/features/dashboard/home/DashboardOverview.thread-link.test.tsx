// @vitest-environment jsdom
import { createRouter, memoryHistory, useLocation } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createStore, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatThreadSession } from '@/shared/api/chat-client'

vi.mock('@/shared/api/chat-client', () => ({ listChatThreads: vi.fn() }))
vi.mock('./DashboardCards', () => ({ DashboardImageCard: () => null }))
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))
import { listChatThreads } from '@/shared/api/chat-client'
import { readThreadDeepLink } from '@/features/chat/lib/chat-thread-deep-link'
import { readActiveChatThreadId, setActiveChatThreadId } from '@/features/chat/lib/chat-thread-history'
import { DashboardRecentWork } from './DashboardOverview'

const [identity] = createStore({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const thread = (threadId: string, title: string, updatedAt: string) => ({ threadId, title, preview: '', updatedAt, pinned: false }) as ChatThreadSession
const NEWER = 'thread-newer'
// An ID that needs percent-encoding, so the href proves the builder rather than string concatenation.
const ODD = 'thread older&x=1'

// What the browser would do with a click: recorded at the document, after Solid's delegated handler and the
// router's own link handler have both run, then cancelled so jsdom does not try to load the page.
let followedByBrowser: boolean[] = []
const trap = (event: Event) => { followedByBrowser.push(!event.defaultPrevented); event.preventDefault() }

async function setup() {
  const history = memoryHistory('/dashboard')
  const TestRouter = createRouter({ history, explicitLinks: true, routes: [{ path: '/*all', component: () => {
    const location = useLocation()
    return <><DashboardRecentWork /><output data-testid="location">{location.pathname}{location.search}</output></>
  } }] })
  const view = render(() => <TestRouter>{(props) => props.children}</TestRouter>)
  document.addEventListener('click', trap)
  await waitFor(() => expect(view.container.querySelectorAll('.home-recent__row')).toHaveLength(2))
  const [newer, older] = [...view.container.querySelectorAll<HTMLAnchorElement>('.home-recent__row')] as [HTMLAnchorElement, HTMLAnchorElement]
  return { newer, older }
}
const location = () => screen.getByTestId('location').textContent
const settle = () => new Promise((resolve) => setTimeout(resolve, 0))

beforeEach(() => {
  localStorage.clear(); sessionStorage.clear()
  followedByBrowser = []
  vi.mocked(listChatThreads).mockReset().mockResolvedValue([
    thread(ODD, 'Older work', '2026-09-20T10:00:00Z'),
    thread(NEWER, 'Newer work', '2026-09-28T10:00:00Z'),
  ])
  flush()
})
afterEach(() => {
  document.removeEventListener('click', trap)
  vi.restoreAllMocks()
})

describe('Home recent work rows are real links to their thread', () => {
  it('gives each row its own deep link and keeps All conversations on plain /chat', async () => {
    const { newer, older } = await setup()
    expect(newer.getAttribute('href')).toBe('/chat?thread_id=thread-newer')
    // Whatever the encoding, Chat reads back exactly this row's thread.
    expect(readThreadDeepLink(new URL(older.getAttribute('href')!, 'https://verevon.test').search)).toBe(ODD)
    expect(screen.getByRole('link', { name: /All conversations/ }).getAttribute('href')).toBe('/chat')
  })

  it('a plain click points Chat at the clicked thread and routes to /chat inside the app', async () => {
    const { older } = await setup()
    fireEvent.click(older)
    await waitFor(() => expect(location()).toBe('/chat'))
    expect(readActiveChatThreadId()).toBe(ODD)
    // The app handled it: the browser never follows the href, so there is no page load and no ?thread_id= left in the URL.
    expect(followedByBrowser).toEqual([false])
  })

  it.each([
    ['Ctrl', { ctrlKey: true }],
    ['Cmd', { metaKey: true }],
    ['Shift', { shiftKey: true }],
    ['Alt', { altKey: true }],
    ['middle button', { button: 1 }],
    ['right button', { button: 2 }],
  ])('a %s click leaves the shared active thread alone and lets the browser follow the href', async (_name, init) => {
    setActiveChatThreadId('thread-open-in-this-tab')
    const { newer } = await setup()
    fireEvent.click(newer, init)
    await settle(); flush()
    expect(readActiveChatThreadId()).toBe('thread-open-in-this-tab')
    expect(location()).toBe('/dashboard')
    expect(followedByBrowser).toEqual([true])
  })
})
