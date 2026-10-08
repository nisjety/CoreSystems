// @vitest-environment jsdom
import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createStore, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))
vi.mock('@/features/chat/lib/chat-appearance-preview', () => ({ chatAppearanceActive: () => true }))
vi.mock('@/features/chat/components/ChatPage', () => ({ default: () => null }))
vi.mock('./DashboardComposer', () => ({ DashboardComposer: (props: { message: string }) => <input id="verevon-dashboard-input" aria-label="Draft" value={props.message} /> }))
vi.mock('./DashboardSearchPanel', () => ({ SearchPanel: () => <p>Web tool</p> }))
vi.mock('./KnowledgeComposer', () => ({ KnowledgeComposer: () => <p>Extract tool</p> }))
vi.mock('./DashboardCards', () => ({ DashboardCardsRail: () => null }))
vi.mock('./DashboardOverview', () => ({ DashboardOverview: (props: { onPrompt: (card: never) => void }) => <button onClick={() => props.onPrompt({ prompt: 'Skriv et sammendrag', promptEn: 'Write a summary' } as never)}>Prompt card</button> }))
vi.mock('./DashboardSectionOverview', () => ({ DashboardSectionOverview: () => null }))
import DashboardHome from './DashboardHome'
import { homeLayoutStorageKey, writeHomeLayout } from './home-layout-state'

type Identity = { user: { id: string } | null; activeOrg: { id: string } | null }
const [identity, setIdentity] = createStore<Identity>({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const signIn = (user: string | null, org: string | null) => {
  setIdentity((state) => { state.user = user ? { id: user } : null; state.activeOrg = org ? { id: org } : null })
  flush()
}
const keyA = homeLayoutStorageKey('user-a', 'org-a')!
const keyB = homeLayoutStorageKey('user-b', 'org-a')!
const stored = (key: string) => JSON.parse(localStorage.getItem(key) ?? 'null')

function setup(path = '/dashboard') {
  const TestRouter = createRouter({ history: memoryHistory(path), explicitLinks: true, routes: [{ path: '/*all', component: () => <DashboardHome /> }] })
  const view = render(() => <TestRouter>{(props) => props.children}</TestRouter>)
  return { ...view, dock: () => view.container.querySelector<HTMLElement>('.home-chat-dock')! }
}
const dockState = (dock: HTMLElement) => ({ minimized: dock.dataset.dockMinimized, expanded: dock.dataset.dockExpanded })
const minimize = () => { fireEvent.click(screen.getByRole('button', { name: /chat bar/ })); flush() }
const moreOptions = () => { fireEvent.click(screen.getByRole('button', { name: 'More chat options' })); flush() }

beforeEach(() => {
  localStorage.clear(); sessionStorage.clear()
  signIn('user-a', 'org-a')
})
afterEach(() => vi.restoreAllMocks())

describe('Home dock is remembered per browser and account', () => {
  it('starts minimized-off and collapsed when nothing is remembered', () => {
    const { dock } = setup()
    expect(dockState(dock())).toEqual({ minimized: 'false', expanded: 'false' })
    expect(localStorage.getItem(keyA)).toBeNull()
  })

  it('remembers a minimized dock and shows it minimized on the first render of the next visit', () => {
    const first = setup()
    minimize()
    expect(dockState(first.dock())).toEqual({ minimized: 'true', expanded: 'false' })
    expect(stored(keyA)).toMatchObject({ dockMinimized: true, dockExpanded: false })
    first.unmount()

    const second = setup()
    // No flush, no await: the restored state is what is painted first.
    expect(dockState(second.dock())).toEqual({ minimized: 'true', expanded: 'false' })
    expect(screen.getByRole('button', { name: 'Expand chat bar' }).getAttribute('aria-expanded')).toBe('false')
    minimize()
    expect(dockState(second.dock()).minimized).toBe('false')
    expect(stored(keyA)).toMatchObject({ dockMinimized: false })
  })

  it('remembers an expanded dock, and expanding also un-minimizes it', () => {
    const first = setup()
    minimize()
    moreOptions()
    expect(dockState(first.dock())).toEqual({ minimized: 'false', expanded: 'true' })
    first.unmount()

    const second = setup()
    expect(dockState(second.dock())).toEqual({ minimized: 'false', expanded: 'true' })
    moreOptions()
    expect(dockState(second.dock()).expanded).toBe('false')
    expect(stored(keyA)).toMatchObject({ dockMinimized: false, dockExpanded: false })
  })

  it('opens the dock for a prompt card without changing what is remembered', async () => {
    const first = setup()
    minimize()
    fireEvent.click(screen.getByText('Prompt card')); flush()
    await waitFor(() => expect((screen.getByLabelText('Draft') as HTMLInputElement).value).toBe('Write a summary'))
    expect(dockState(first.dock())).toEqual({ minimized: 'false', expanded: 'true' })
    expect(stored(keyA)).toMatchObject({ dockMinimized: true, dockExpanded: false })
    first.unmount()
    expect(dockState(setup().dock())).toEqual({ minimized: 'true', expanded: 'false' })
  })

  it('lets the dock be collapsed and minimized again after a prompt card opened it', async () => {
    const { dock } = setup()
    fireEvent.click(screen.getByText('Prompt card')); flush()
    await waitFor(() => expect(dockState(dock())).toEqual({ minimized: 'false', expanded: 'true' }))
    moreOptions()
    expect(dockState(dock())).toEqual({ minimized: 'false', expanded: 'false' })

    fireEvent.click(screen.getByText('Prompt card')); flush()
    await waitFor(() => expect(dockState(dock()).expanded).toBe('true'))
    minimize()
    expect(dockState(dock())).toEqual({ minimized: 'true', expanded: 'false' })
    expect(stored(keyA)).toMatchObject({ dockMinimized: true, dockExpanded: false })
  })

  it('keeps the dock as it is when the tab changes', async () => {
    const { dock } = setup()
    minimize()
    fireEvent.click(screen.getByRole('button', { name: 'Web' })); flush()
    await waitFor(() => expect(screen.getByText('Web tool')).toBeTruthy())
    fireEvent.click(screen.getByRole('button', { name: 'Chat' })); flush()
    await waitFor(() => expect(screen.queryByText('Web tool')).toBeNull())
    expect(dockState(dock())).toEqual({ minimized: 'true', expanded: 'false' })
  })

  it('re-reads for a new user and never writes the change under the previous one', () => {
    writeHomeLayout(keyA, { dockMinimized: true })
    writeHomeLayout(keyB, { dockExpanded: true })
    const { dock } = setup()
    expect(dockState(dock())).toEqual({ minimized: 'true', expanded: 'false' })

    signIn('user-b', 'org-a')
    expect(dockState(dock())).toEqual({ minimized: 'false', expanded: 'true' })
    minimize()
    expect(stored(keyB)).toMatchObject({ dockMinimized: true, dockExpanded: true })
    expect(stored(keyA)).toMatchObject({ dockMinimized: true, dockExpanded: false })

    signIn('user-a', 'org-a')
    expect(dockState(dock())).toEqual({ minimized: 'true', expanded: 'false' })
  })

  it('re-reads when the workspace changes', () => {
    const keyOrgB = homeLayoutStorageKey('user-a', 'org-b')!
    writeHomeLayout(keyA, { dockMinimized: true })
    const { dock } = setup()
    signIn('user-a', 'org-b')
    expect(dockState(dock())).toEqual({ minimized: 'false', expanded: 'false' })
    moreOptions()
    expect(stored(keyOrgB)).toMatchObject({ dockExpanded: true })
    expect(stored(keyA)).toMatchObject({ dockMinimized: true, dockExpanded: false })
  })

  it('does not read or write while the identity is not known', () => {
    writeHomeLayout(keyA, { dockMinimized: true })
    const read = vi.spyOn(Storage.prototype, 'getItem')
    const write = vi.spyOn(Storage.prototype, 'setItem')
    signIn('user-a', null)
    const { dock } = setup()
    expect(dockState(dock())).toEqual({ minimized: 'false', expanded: 'false' })
    moreOptions()
    expect(dockState(dock()).expanded).toBe('true')
    expect(read).not.toHaveBeenCalled()
    expect(write).not.toHaveBeenCalled()

    // Once the workspace is known the remembered layout wins.
    signIn('user-a', 'org-a')
    expect(dockState(dock())).toEqual({ minimized: 'true', expanded: 'false' })
  })

  it('falls back to the defaults for corrupt or invalid remembered data', () => {
    for (const raw of ['{broken', '[]', JSON.stringify({ dockMinimized: 'yes', dockExpanded: 1 })]) {
      localStorage.setItem(keyA, raw)
      const { dock, unmount } = setup()
      expect(dockState(dock())).toEqual({ minimized: 'false', expanded: 'false' })
      unmount()
    }
  })
})
