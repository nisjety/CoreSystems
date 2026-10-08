// @vitest-environment jsdom
import { createRouter, memoryHistory, useLocation } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('@/features/chat/lib/chat-appearance-preview', () => ({ chatAppearanceActive: () => preview() }))
vi.mock('@/features/chat/components/ChatPage', () => ({ default: () => null }))
vi.mock('./DashboardComposer', () => ({ DashboardComposer: (props: { message: string; onMessageChange: (value: string) => void }) => {
  const [attached, setAttached] = createSignal(false)
  return <>
    <input id="verevon-dashboard-input" aria-label="Draft" value={props.message} onInput={(event) => props.onMessageChange(event.currentTarget.value)} onFocus={(event) => focusedWhileHidden.push(!!event.currentTarget.closest('[hidden]'))} />
    <button onClick={() => setAttached(true)}>Attach fixture</button><span>{attached() ? 'Attachment retained' : ''}</span>
  </>
} }))
vi.mock('./DashboardSearchPanel', () => ({ SearchPanel: (props: { onExpandedChange: (value: boolean) => void; onPreviewActiveChange: (value: boolean) => void }) => <>
  <p>Web tool</p><button onClick={() => props.onPreviewActiveChange(true)}>Web preview on</button><button onClick={() => props.onExpandedChange(true)}>Web expand</button>
</> }))
vi.mock('./KnowledgeComposer', () => ({ KnowledgeComposer: (props: { onPreviewActiveChange: (value: boolean) => void }) => <>
  <p>Extract tool</p><button onClick={() => props.onPreviewActiveChange(true)}>Extract preview on</button>
</> }))
vi.mock('./DashboardCards', () => ({ DashboardCardsRail: () => null }))
vi.mock('./DashboardOverview', () => ({ DashboardOverview: (props: { onPrompt: (card: never) => void }) => <button onClick={() => props.onPrompt({ prompt: 'Skriv et sammendrag', promptEn: 'Write a summary' } as never)}>Prompt card</button> }))
vi.mock('./DashboardSectionOverview', () => ({ DashboardSectionOverview: (props: { section: string }) => <p>Section {props.section}</p> }))
import DashboardHome from './DashboardHome'

const [preview, setPreview] = createSignal(true)
const focusedWhileHidden: boolean[] = []
beforeEach(() => {
  setPreview(true)
  focusedWhileHidden.length = 0
  flush()
})

function setup(path = '/dashboard') {
  const history = memoryHistory(path)
  const TestRouter = createRouter({ history, explicitLinks: true, routes: [{ path: '/*all', component: () => {
    const location = useLocation()
    return <><DashboardHome /><output data-testid="location">{location.pathname}{location.search}</output></>
  } }] })
  const view = render(() => <TestRouter>{(props) => props.children}</TestRouter>)
  return { history, ...view }
}
const tab = (name: 'Chat' | 'Web' | 'Extract') => screen.getByRole('button', { name })
const pressed = () => (['Chat', 'Web', 'Extract'] as const).filter((name) => tab(name).getAttribute('aria-pressed') === 'true')
const url = () => screen.getByTestId('location').textContent
const goTo = async (name: 'Chat' | 'Web' | 'Extract', search: string) => {
  fireEvent.click(tab(name)); flush()
  await waitFor(() => expect(url()).toBe(`/dashboard${search}`))
}

describe('Home tab in the URL', () => {
  it('opens the tab named by ?tab=', () => {
    for (const [path, name, tool] of [['/dashboard', 'Chat', null], ['/dashboard?tab=web', 'Web', 'Web tool'], ['/dashboard?tab=extract', 'Extract', 'Extract tool']] as const) {
      const { container, unmount } = setup(path)
      expect(pressed()).toEqual([name])
      expect(container.querySelector('.dashboard-home')?.getAttribute('data-home-tab')).toBe({ Chat: 'Chat', Web: 'Søk', Extract: 'Crawl' }[name])
      for (const other of ['Web tool', 'Extract tool']) expect(screen.queryByText(other) !== null).toBe(other === tool)
      unmount()
    }
  })

  it('falls back to Chat for an unknown, cased or repeated ?tab=', () => {
    for (const path of ['/dashboard?tab=unknown', '/dashboard?tab=WEB', '/dashboard?tab=', '/dashboard?tab=web&tab=extract', '/dashboard?tab=S%C3%B8k', '/dashboard?tab=Crawl']) {
      const { unmount } = setup(path)
      expect(pressed()).toEqual(['Chat'])
      expect(screen.queryByText('Web tool')).toBeNull()
      expect(screen.queryByText('Extract tool')).toBeNull()
      unmount()
    }
  })

  it('writes ?tab= on click, keeps unrelated params and drops the param for Chat', async () => {
    setup('/dashboard?ref=mail')
    await goTo('Web', '?ref=mail&tab=web')
    expect(pressed()).toEqual(['Web'])
    await goTo('Extract', '?ref=mail&tab=extract')
    expect(pressed()).toEqual(['Extract'])
    await goTo('Chat', '?ref=mail')
    expect(pressed()).toEqual(['Chat'])
  })

  it('moves between tabs with Back and Forward, one history entry per tab', async () => {
    const { history } = setup()
    await goTo('Web', '?tab=web')
    await goTo('Extract', '?tab=extract')
    fireEvent.click(tab('Extract')); flush()
    await Promise.resolve(); flush()
    history.back(); flush()
    await waitFor(() => expect(pressed()).toEqual(['Web']))
    expect(screen.getByText('Web tool')).toBeTruthy()
    history.back(); flush()
    await waitFor(() => expect(pressed()).toEqual(['Chat']))
    expect(url()).toBe('/dashboard')
    history.forward(); flush()
    await waitFor(() => expect(pressed()).toEqual(['Web']))
    history.forward(); flush()
    await waitFor(() => expect(pressed()).toEqual(['Extract']))
    expect(screen.getByText('Extract tool')).toBeTruthy()
  })

  it('follows a direct link that changes the tab', async () => {
    const { history } = setup()
    history.set({ value: '/dashboard?tab=extract' }); flush()
    await waitFor(() => expect(pressed()).toEqual(['Extract']))
    history.set({ value: '/dashboard?tab=nonsense' }); flush()
    await waitFor(() => expect(pressed()).toEqual(['Chat']))
  })

  it('lets ?overview= win over ?tab= and keep the tabs hidden', async () => {
    const { history, container } = setup('/dashboard?overview=studio&tab=web')
    expect(screen.getByText('Section studio')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Web' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Chat' })).toBeNull()
    expect(screen.queryByText('Web tool')).toBeNull()
    expect(container.querySelector('.dashboard-home')?.getAttribute('data-home-tab')).toBe('Chat')
    history.set({ value: '/dashboard?tab=web' }); flush()
    await waitFor(() => expect(pressed()).toEqual(['Web']))
    expect(screen.getByText('Web tool')).toBeTruthy()
    expect(screen.queryByText('Section studio')).toBeNull()
  })

  it('keeps the mounted chat composer, its draft and attachment across tab changes', async () => {
    const { history } = setup()
    const draft = screen.getByLabelText('Draft')
    fireEvent.input(draft, { target: { value: 'Keep this draft' } })
    fireEvent.click(screen.getByText('Attach fixture')); flush()
    await goTo('Web', '?tab=web')
    expect(screen.getByLabelText('Draft')).toBe(draft)
    expect(draft.closest('[hidden]')).not.toBeNull()
    await goTo('Extract', '?tab=extract')
    history.back(); flush()
    await waitFor(() => expect(pressed()).toEqual(['Web']))
    history.back(); flush()
    await waitFor(() => expect(pressed()).toEqual(['Chat']))
    expect(screen.getByLabelText('Draft')).toBe(draft)
    expect(draft.closest('[hidden]')).toBeNull()
    expect((draft as HTMLInputElement).value).toBe('Keep this draft')
    expect(screen.getByText('Attachment retained')).toBeTruthy()
  })

  it('leaves the Chat tab with a prompt card and focuses the composer once it shows again', async () => {
    setup('/dashboard?tab=web&ref=mail')
    expect(screen.getByText('Web tool')).toBeTruthy()
    fireEvent.click(screen.getByText('Prompt card')); flush()
    await waitFor(() => expect(url()).toBe('/dashboard?ref=mail'))
    expect(pressed()).toEqual(['Chat'])
    expect((screen.getByLabelText('Draft') as HTMLInputElement).value).toBe('Write a summary')
    await waitFor(() => expect(document.activeElement).toBe(screen.getByLabelText('Draft')))
    expect(focusedWhileHidden).toEqual([false])
  })
})

describe('Home tab side effects follow the URL', () => {
  const bands = (container: HTMLElement) => container.querySelector('.verevon-home-bands')!.classList
  const resultsToggle = () => screen.queryByRole('button', { name: /Show the cards|Show the result again/ })

  it('clears a Web result and expanded search when Back or Forward changes the tab', async () => {
    setPreview(false); flush()
    const { history, container } = setup()
    await goTo('Web', '?tab=web')
    fireEvent.click(screen.getByText('Web preview on')); flush()
    expect(bands(container).contains('verevon-home-bands--results')).toBe(true)
    fireEvent.click(resultsToggle()!); flush()
    expect(resultsToggle()?.getAttribute('aria-label')).toBe('Show the result again')
    fireEvent.click(screen.getByText('Web expand')); flush()
    expect(bands(container).contains('verevon-home-bands--expanded')).toBe(true)

    history.back(); flush()
    await waitFor(() => expect(pressed()).toEqual(['Chat']))
    expect(bands(container).contains('verevon-home-bands--expanded')).toBe(false)
    expect(bands(container).contains('verevon-home-bands--results')).toBe(false)
    history.forward(); flush()
    await waitFor(() => expect(pressed()).toEqual(['Web']))
    expect(resultsToggle()).toBeNull()
    expect(bands(container).contains('verevon-home-bands--results')).toBe(false)
    expect(bands(container).contains('verevon-home-bands--expanded')).toBe(false)
  })

  it('clears an Extract result when a direct link changes the tab', async () => {
    setPreview(false); flush()
    const { history, container } = setup('/dashboard?tab=extract')
    fireEvent.click(screen.getByText('Extract preview on')); flush()
    expect(bands(container).contains('verevon-home-bands--results')).toBe(true)
    history.set({ value: '/dashboard?tab=web' }); flush()
    await waitFor(() => expect(pressed()).toEqual(['Web']))
    expect(bands(container).contains('verevon-home-bands--results')).toBe(false)
    history.set({ value: '/dashboard?tab=extract' }); flush()
    await waitFor(() => expect(pressed()).toEqual(['Extract']))
    expect(bands(container).contains('verevon-home-bands--results')).toBe(false)
    expect(resultsToggle()).toBeNull()
  })

  it('keeps the result when only an unrelated param changes', async () => {
    setPreview(false); flush()
    const { history, container } = setup('/dashboard?tab=web')
    fireEvent.click(screen.getByText('Web preview on')); flush()
    history.set({ value: '/dashboard?tab=web&ref=mail' }); flush()
    await waitFor(() => expect(url()).toBe('/dashboard?tab=web&ref=mail'))
    expect(bands(container).contains('verevon-home-bands--results')).toBe(true)
    expect(resultsToggle()).not.toBeNull()
  })

  it('still returns to the result when the active tab is clicked again, without a history entry', async () => {
    setPreview(false); flush()
    const { history } = setup()
    await goTo('Web', '?tab=web')
    fireEvent.click(screen.getByText('Web preview on')); flush()
    fireEvent.click(resultsToggle()!); flush()
    expect(resultsToggle()?.getAttribute('aria-label')).toBe('Show the result again')
    fireEvent.click(tab('Web')); flush()
    expect(resultsToggle()?.getAttribute('aria-label')).toBe('Show the cards')
    await Promise.resolve(); flush()
    history.back(); flush()
    await waitFor(() => expect(pressed()).toEqual(['Chat']))
    expect(url()).toBe('/dashboard')
  })
})
