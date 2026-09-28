// @vitest-environment jsdom
import { createRouter, memoryHistory, useLocation } from '@solidjs/router'
import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { createSignal, createStore, flush } from 'solid-js'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { homeOverviewSectionSchema } from '@/shared/actions/home-overview'
import type { HomeSectionData } from './home-section-data'

vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))
vi.mock('@/features/chat/lib/chat-appearance-preview', () => ({ chatAppearanceActive: () => preview() }))
vi.mock('@/features/chat/components/ChatPage', () => ({ default: () => null }))
vi.mock('./DashboardComposer', () => ({ DashboardComposer: (props: { message: string; onMessageChange: (value: string) => void }) => {
  const [attached, setAttached] = createSignal(false)
  return <><input aria-label="Draft" value={props.message} onInput={(event) => props.onMessageChange(event.currentTarget.value)} /><button onClick={() => setAttached(true)}>Attach fixture</button><span>{attached() ? 'Attachment retained' : ''}</span></>
} }))
vi.mock('./DashboardSearchPanel', () => ({ SearchPanel: () => <p>Web tool</p> }))
vi.mock('./KnowledgeComposer', () => ({ KnowledgeComposer: () => <p>Extract tool</p> }))
vi.mock('./DashboardCards', () => ({ DashboardCardsRail: () => null }))
vi.mock('./DashboardOverview', () => ({ DashboardOverview: () => <p>Home overview</p> }))
vi.mock('./home-section-data', async (importOriginal) => ({ ...await importOriginal<typeof import('./home-section-data')>(), loadHomeSection: vi.fn() }))
import { loadHomeSection } from './home-section-data'
import { DashboardSectionOverview } from './DashboardSectionOverview'
import DashboardHome from './DashboardHome'
import { CoreSidebar } from '@/features/core/components/CoreSidebar'

const [identity, setIdentity] = createStore({ user: { id: 'user-a' }, activeOrg: { id: 'org-a', role: 'owner' } })
const [preview, setPreview] = createSignal(true)
const empty: HomeSectionData = { items: [], metrics: [] }
beforeEach(() => {
  vi.mocked(loadHomeSection).mockReset().mockResolvedValue(empty)
  setIdentity((state) => { state.activeOrg.id = 'org-a' })
  setPreview(true)
  flush()
})

function setup(path = '/dashboard') {
  const history = memoryHistory(path)
  const TestRouter = createRouter({ history, explicitLinks: true, routes: [{ path: '/*all', component: () => {
    const location = useLocation()
    return <><CoreSidebar activeRoute="/dashboard" expanded onExpandedChange={() => undefined} onOpenSearch={() => undefined} /><DashboardHome /><output data-testid="location">{location.pathname}{location.search}</output></>
  } }] })
  const view = render(() => <TestRouter>{(props) => props.children}</TestRouter>)
  return { history, ...view }
}

describe('Home section navigation', () => {
  it('selects all sidebar overviews in Home and preserves the composer draft and attachment', async () => {
    setup()
    const draft = screen.getByLabelText('Draft')
    fireEvent.input(draft, { target: { value: 'Keep this draft' } })
    fireEvent.click(screen.getByText('Attach fixture')); flush()
    for (const [label, section] of [['Rom', 'spaces'], ['Studio', 'studio'], ['Support', 'support']]) {
      fireEvent.click(within(screen.getByRole('navigation', { name: 'Home navigation' })).getByRole('button', { name: label })); flush()
      await waitFor(() => expect(screen.getByRole('heading', { level: 1, name: label })).toBeTruthy())
      expect(screen.getByTestId('location').textContent).toBe(`/dashboard?overview=${section}`)
      expect(screen.getByLabelText('Draft')).toBe(draft)
    }
    fireEvent.click(screen.getByRole('button', { name: 'Shared with me' })); flush()
    for (const [label, section] of [['Social', 'social'], ['Knowledge', 'knowledge'], ['Insights', 'insights'], ['Leads', 'leads'], ['Agents', 'agents']]) {
      fireEvent.click(within(screen.getByRole('navigation', { name: 'Home navigation' })).getByRole('button', { name: label })); flush()
      await waitFor(() => expect(screen.getByRole('heading', { level: 1, name: label })).toBeTruthy())
      expect(screen.getByTestId('location').textContent).toBe(`/dashboard?overview=${section}`)
    }
    fireEvent.click(screen.getByRole('button', { name: 'My account' })); flush()
    fireEvent.click(within(screen.getByRole('navigation', { name: 'Home navigation' })).getByRole('button', { name: 'Home' })); flush()
    await waitFor(() => expect(screen.getByText('Home overview')).toBeTruthy())
    expect(screen.getByLabelText('Draft')).toBe(draft)
    expect((draft as HTMLInputElement).value).toBe('Keep this draft')
    expect(screen.getByText('Attachment retained')).toBeTruthy()
    expect(loadHomeSection).toHaveBeenCalledTimes(homeOverviewSectionSchema.options.length - 1)
  })

  it('restores a shared overview from the URL and returns to ordinary links when disabled', async () => {
    setup('/dashboard?overview=knowledge')
    await waitFor(() => expect(screen.getByRole('heading', { level: 1, name: 'Knowledge' })).toBeTruthy())
    expect(within(screen.getByRole('navigation', { name: 'Home navigation' })).getByRole('button', { name: 'Knowledge' }).getAttribute('aria-pressed')).toBe('true')
    setPreview(false); flush()
    expect(within(screen.getByRole('navigation', { name: 'Home navigation' })).getByRole('link', { name: 'Rom' }).getAttribute('href')).toBe('/spaces')
    expect(screen.queryByRole('heading', { level: 1, name: 'Knowledge' })).toBeNull()
  })

  it('falls back to Home for an invalid section', () => {
    setup('/dashboard?overview=unknown')
    expect(screen.getByText('Home overview')).toBeTruthy()
    expect(loadHomeSection).not.toHaveBeenCalled()
  })

  it('restores the selected overview with Back and Forward', async () => {
    const { history } = setup()
    const navigation = within(screen.getByRole('navigation', { name: 'Home navigation' }))
    fireEvent.click(navigation.getByRole('button', { name: 'Rom' })); flush()
    await waitFor(() => expect(screen.getByRole('heading', { level: 1, name: 'Rom' })).toBeTruthy())
    fireEvent.click(navigation.getByRole('button', { name: 'Studio' })); flush()
    await waitFor(() => expect(screen.getByRole('heading', { level: 1, name: 'Studio' })).toBeTruthy())
    history.back(); flush()
    await waitFor(() => expect(screen.getByRole('heading', { level: 1, name: 'Rom' })).toBeTruthy())
    history.forward(); flush()
    await waitFor(() => expect(screen.getByRole('heading', { level: 1, name: 'Studio' })).toBeTruthy())
  })
})

describe('Home section data states', () => {
  it('retries an unavailable source without showing a false zero summary', async () => {
    vi.mocked(loadHomeSection).mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce(empty)
    render(() => <DashboardSectionOverview section="spaces" />)
    await waitFor(() => expect(screen.getByText('This overview is unavailable right now.')).toBeTruthy())
    expect(screen.queryByText('At a glance')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Try again' })); flush()
    await waitFor(() => expect(screen.getByText('No rooms are available yet.')).toBeTruthy())
  })

  it('ignores a late response from a previous workspace', async () => {
    let resolveOld!: (data: HomeSectionData) => void
    vi.mocked(loadHomeSection).mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve }))
      .mockResolvedValueOnce({ items: [{ id: 'current', title: 'Current room', href: '/spaces/current' }], metrics: [] })
    render(() => <DashboardSectionOverview section="spaces" />); flush()
    setIdentity((state) => { state.activeOrg.id = 'org-b' }); flush()
    await waitFor(() => expect(screen.getByText('Current room')).toBeTruthy())
    resolveOld({ items: [{ id: 'old', title: 'Previous organization room', href: '/spaces/old' }], metrics: [] })
    await Promise.resolve(); flush()
    expect(screen.queryByText('Previous organization room')).toBeNull()
  })
})
