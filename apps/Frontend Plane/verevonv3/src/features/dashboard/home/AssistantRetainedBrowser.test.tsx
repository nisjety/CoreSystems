// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { AssistantRetainedBrowser } from './AssistantRetainedBrowser'

const api = vi.hoisted(() => ({ get: vi.fn(), select: vi.fn(), act: vi.fn(), control: vi.fn(), scope: { user: { id: 'user-1' }, activeOrg: { id: 'org-1' } } }))
vi.mock('@/shared/api/browser-client', async (importOriginal) => ({ ...(await importOriginal<typeof import('@/shared/api/browser-client')>()), getBrowserSession: api.get, selectBrowserTab: api.select, runBrowserAction: api.act }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => api.scope }))
vi.mock('@/features/browser/dock/browser-control', () => ({ changeBrowserControl: api.control, STALE_CONTROL_COPY: { no: 'Styringen ble endret', en: 'Control changed' } }))
vi.mock('@/features/dashboard/home/BrowserChrome', () => ({ BrowserChrome: (props: { onBrowserAction: (action: unknown) => void; session: { tabs: { tabId: string; active: boolean }[] } }) => <button data-active-tab={props.session.tabs.find(tab => tab.active)?.tabId} onClick={() => props.onBrowserAction({ type: 'click_point', x: 100, y: 200 })}>Page</button> }))
const receipt = { runId: 'model-1', planId: 'plan-1', attachId: 'attach-1', sessionRunId: 'session-1', tabId: 'agent-tab' }
const projection = (mode: 'agent_control' | 'human_takeover') => ({ session: { id: 'session-1', status: 'live', capabilities: ['human_browsing'], control: { mode, epoch: 4 }, profile: { scope: 'run_scoped', storage: 'isolated' }, renderMode: 'chromium', tabs: [], title: 'Example', url: 'https://example.com/', viewport: { width: 1280, height: 800 } } })
afterEach(() => { cleanup(); vi.clearAllMocks(); api.scope.user.id = 'user-1'; api.scope.activeOrg.id = 'org-1' })

describe('completed assistant browser handoff', () => {
  it('takes over and selects the receipt-bound tab before accepting human input', async () => {
    const claim = vi.fn(() => true)
    api.get.mockResolvedValue(projection('agent_control'))
    api.control.mockResolvedValue({ response: projection('human_takeover'), stale: false })
    api.select.mockResolvedValue({ tabs: [{ tabId: 'agent-tab', active: true }] })
    api.act.mockResolvedValue(projection('human_takeover'))
    render(() => <AssistantRetainedBrowser orgId="org-1" sessionId="session-1" modelRunId="model-1" receipt={receipt} onClaim={claim} />)
    fireEvent.click(screen.getByText('Bruk nettleseren'))
    await waitFor(() => expect(screen.getByText('Page')).toBeTruthy())
    expect(api.get).toHaveBeenCalledWith('org-1', 'session-1')
    expect(api.control).toHaveBeenCalledWith('org-1', 'session-1', 'human_takeover', 4)
    expect(api.select).toHaveBeenCalledWith('org-1', 'session-1', 'agent-tab')
    expect(claim).toHaveBeenCalledWith('session-1')
    fireEvent.click(screen.getByText('Page'))
    await waitFor(() => expect(api.act).toHaveBeenCalledWith('org-1', 'session-1', { type: 'click_point', x: 100, y: 200 }, { actor: 'human' }))
  })
  it('confirms an already-human session with an idempotent control request', async () => {
    api.get.mockResolvedValue(projection('human_takeover'))
    api.control.mockResolvedValue({ response: projection('human_takeover'), stale: false })
    api.select.mockResolvedValue({ tabs: [{ tabId: 'agent-tab', active: true }] })
    render(() => <AssistantRetainedBrowser orgId="org-1" sessionId="session-1" modelRunId="model-1" receipt={receipt} onClaim={() => true} />)
    fireEvent.click(screen.getByText('Bruk nettleseren'))
    await waitFor(() => expect(screen.getByText('Page')).toBeTruthy())
    expect(api.control).toHaveBeenCalledWith('org-1', 'session-1', 'human_takeover', 4)
  })
  it('resumes the owner’s selected tab when returning to a claimed browser', async () => {
    const ownerView = projection('human_takeover')
    const response = { session: { ...ownerView.session, tabs: [
      { tabId: 'agent-tab', active: false, url: 'https://example.com/' },
      { tabId: 'owner-tab', active: true, url: 'https://example.com/next' },
    ] } }
    api.get.mockResolvedValue(ownerView)
    api.control.mockResolvedValue({ response, stale: false })
    render(() => <AssistantRetainedBrowser orgId="org-1" sessionId="session-1" modelRunId="model-1" receipt={receipt} claimed onClaim={() => true} />)
    await waitFor(() => expect(screen.getByText('Page').getAttribute('data-active-tab')).toBe('owner-tab'))
    expect(api.select).not.toHaveBeenCalled()
  })
  it('does not transfer a session if the signed-in owner changes during the read', async () => {
    let finish!: (value: ReturnType<typeof projection>) => void
    api.get.mockReturnValue(new Promise(resolve => { finish = resolve }))
    const claim = vi.fn(() => true)
    render(() => <AssistantRetainedBrowser orgId="org-1" sessionId="session-1" modelRunId="model-1" receipt={receipt} onClaim={claim} />)
    fireEvent.click(screen.getByText('Bruk nettleseren'))
    api.scope.user.id = 'user-2'
    finish(projection('agent_control'))
    await waitFor(() => expect(screen.getByText('Bruk nettleseren')).toBeTruthy())
    expect(api.control).not.toHaveBeenCalled()
    expect(api.select).not.toHaveBeenCalled()
    expect(claim).not.toHaveBeenCalled()
  })
  it('refuses to select or expose a tab when takeover was not confirmed', async () => {
    const claim = vi.fn(() => true)
    api.get.mockResolvedValue(projection('agent_control'))
    api.control.mockResolvedValue({ response: projection('agent_control'), stale: false })
    render(() => <AssistantRetainedBrowser orgId="org-1" sessionId="session-1" modelRunId="model-1" receipt={receipt} onClaim={claim} />)
    fireEvent.click(screen.getByText('Bruk nettleseren'))
    await waitFor(() => expect(screen.getByRole('alert')).toBeTruthy())
    expect(api.select).not.toHaveBeenCalled()
    expect(claim).not.toHaveBeenCalled()
    expect(screen.queryByText('Page')).toBeNull()
  })
  it('does not open a different run or a tab that was not selected', async () => {
    const claim = vi.fn(() => true)
    const first = render(() => <AssistantRetainedBrowser orgId="org-1" sessionId="session-1" modelRunId="other" receipt={receipt} onClaim={claim} />)
    fireEvent.click(screen.getByText('Bruk nettleseren'))
    expect(api.get).not.toHaveBeenCalled()
    first.unmount()
    api.get.mockResolvedValue(projection('human_takeover'))
    api.control.mockResolvedValue({ response: projection('human_takeover'), stale: false })
    api.select.mockResolvedValue({ tabs: [{ tabId: 'other-tab', active: true }] })
    render(() => <AssistantRetainedBrowser orgId="org-1" sessionId="session-1" modelRunId="model-1" receipt={receipt} onClaim={claim} />)
    fireEvent.click(screen.getByText('Bruk nettleseren'))
    await waitFor(() => expect(screen.getByRole('alert')).toBeTruthy())
    expect(claim).not.toHaveBeenCalled()
    expect(screen.queryByText('Page')).toBeNull()
  })
})
