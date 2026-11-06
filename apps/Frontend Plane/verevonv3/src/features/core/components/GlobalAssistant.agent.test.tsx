// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => ({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } }) }))
import { GlobalAssistant, type AssistantAgentView, type AssistantNavigationAgentView } from './GlobalAssistant'

const idleChat = { turns: [], status: 'idle' as const, error: null, activity: null, threadId: null, needsFullChat: false }
const base: AssistantAgentView = { available: true, kind: 'browser', status: 'idle', goal: '', error: null, action: null, sessionId: null, runId: null, receipt: null }
function setup(initial: AssistantAgentView) {
  const [agent, setAgent] = createSignal(initial)
  const [message, setMessage] = createSignal('')
  const [workspaceVisible, setWorkspaceVisible] = createSignal(false)
  const stop = vi.fn()
  const openBrowser = vi.fn()
  const retry = vi.fn()
  const decide = vi.fn()
  const send = vi.fn().mockResolvedValue(undefined)
  const allowSiteAccess = vi.fn().mockResolvedValue(undefined)
  const declineSiteAccess = vi.fn()
  const noop = () => undefined
  render(() => <GlobalAssistant forceReveal message={message()} onMessageChange={setMessage} onBeforeSubmit={() => false} canHandleBeforeSubmit={() => false} onSubmit={send} onClose={noop} onStop={noop} onStopAgent={stop} onOpenAgentBrowser={openBrowser} onRetryAgent={retry} onAllowSiteAccess={allowSiteAccess} onDeclineSiteAccess={declineSiteAccess} onApprovalDecision={decide} onOpenChat={noop} onNewChat={noop} sourceScope="workspace" onSourceScopeChange={noop} state={idleChat} agentView={agent()} browserPresentedInWorkspace={workspaceVisible()} />)
  return { setAgent, setMessage, setWorkspaceVisible, stop, openBrowser, send, retry, decide, allowSiteAccess, declineSiteAccess }
}
beforeEach(() => { localStorage.clear(); sessionStorage.clear() })
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllEnvs() })

describe('GlobalAssistant browser agent', () => {
  it('describes a failed terminal receipt that followed a stop request', () => {
    setup({ ...base, status: 'failed', stopRequested: true, error: 'The browser returned an error.' })
    expect(screen.getByText('The task ended after the stop request.')).toBeTruthy()
    expect(screen.getByRole('alert').textContent).toContain('The browser returned an error.')
  })

  it('keeps the dock at the bottom with the flag off', () => {
    setup(base)
    expect(screen.getByRole('region', { name: 'Ask Verevon' }).className).toContain('global-assistant--dock-bottom')
    expect(screen.queryByRole('button', { name: 'Change dock position' })).toBeNull()
  })

  it('moves the dock right and left behind the flag', () => {
    vi.stubEnv('VITE_ASSISTANT_DOCK_POSITION', 'true')
    setup(base)
    const dock = screen.getByRole('region', { name: 'Ask Verevon' })
    expect(dock.className).toContain('global-assistant--dock-bottom')
    fireEvent.click(screen.getByRole('button', { name: 'Change dock position' }))
    flush()
    expect(screen.getByRole('region', { name: 'Ask Verevon' }).className).toContain('global-assistant--dock-right')
    fireEvent.click(screen.getByRole('button', { name: 'Change dock position' }))
    flush()
    expect(screen.getByRole('region', { name: 'Ask Verevon' }).className).toContain('global-assistant--dock-left')
  })

  it('keeps a refused draft and opens site choices in a separate tab', async () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    const { setAgent, setMessage, send } = setup(base)
    const draft = 'Inspect https://example.com'
    setMessage(draft); flush()
    send.mockRejectedValueOnce(new Error('Browser access was not granted.'))
    fireEvent.click(screen.getByRole('button', { name: 'Start task' }))
    await waitFor(() => expect(send).toHaveBeenCalledOnce())
    setAgent({ ...base, status: 'failed', error: 'Review your site choices and any administrator restrictions.', errorCode: 'browser_grant_denied' }); flush()
    expect((screen.getByLabelText('Task for Verevon') as HTMLTextAreaElement).value).toBe(draft)
    const link = screen.getByRole('link', { name: 'Open site choices in a new tab' }) as HTMLAnchorElement
    expect(link.getAttribute('href')).toBe('/settings/browser-sites')
    expect(link.target).toBe('_blank')
  })

  it('asks for temporary website access inline and clears the original draft after starting', async () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    const draft = 'Inspect example.com'
    const { setAgent, setMessage, allowSiteAccess, declineSiteAccess, send } = setup(base)
    setMessage(draft); flush()
    send.mockImplementationOnce(async () => {
      setAgent({ ...base, status: 'failed', goal: draft, errorCode: 'browser_redirect_access_denied', siteAccessRequest: { host: 'portal.example.net' } })
      throw new Error('Redirect access denied.')
    })
    fireEvent.click(screen.getByRole('button', { name: 'Start task' }))
    await waitFor(() => expect(screen.getByRole<HTMLButtonElement>('button', { name: 'Allow for 10 minutes' }).disabled).toBe(false))
    expect(screen.getByText('The agent needs access to this site')).toBeTruthy()
    expect(screen.getByText(/portal\.example\.net/)).toBeTruthy()
    expect(screen.queryByText('The agent task could not be completed.')).toBeNull()
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.queryByRole('link', { name: 'Open site choices in a new tab' })).toBeNull()
    expect(allowSiteAccess).not.toHaveBeenCalled()
    expect(screen.getByLabelText<HTMLTextAreaElement>('Task for Verevon').value).toBe(draft)

    let finish!: () => void
    allowSiteAccess.mockReturnValueOnce(new Promise<void>((resolve) => { finish = resolve }))
    fireEvent.click(screen.getByRole('button', { name: 'Allow for 10 minutes' }))
    flush()
    expect(allowSiteAccess).toHaveBeenCalledOnce()
    expect(declineSiteAccess).not.toHaveBeenCalled()
    expect(screen.getByRole<HTMLButtonElement>('button', { name: 'Not now' }).disabled).toBe(true)
    expect(screen.getByLabelText<HTMLTextAreaElement>('Task for Verevon').value).toBe(draft)
    setAgent({ ...base, status: 'starting', goal: draft }); flush()
    finish()
    await waitFor(() => expect(screen.getByLabelText<HTMLTextAreaElement>('Task for Verevon').value).toBe(''))
    expect(send).toHaveBeenCalledOnce()
  })

  it('keeps the draft when the user declines website access', () => {
    const draft = 'Inspect example.com'
    const { setMessage, allowSiteAccess, declineSiteAccess, send } = setup({
      ...base, status: 'failed', goal: draft, siteAccessRequest: { host: 'portal.example.net' },
    })
    setMessage(draft); flush()
    fireEvent.click(screen.getByRole('button', { name: 'Not now' }))
    expect(declineSiteAccess).toHaveBeenCalledOnce()
    expect(allowSiteAccess).not.toHaveBeenCalled()
    expect(send).not.toHaveBeenCalled()
    expect(screen.getByLabelText<HTMLTextAreaElement>('Task for Verevon').value).toBe(draft)
  })

  it('keeps a new draft typed while a previous submission is pending', async () => {
    const { setMessage, send } = setup(base)
    let finish!: () => void
    send.mockReturnValueOnce(new Promise<void>((resolve) => { finish = resolve }))
    setMessage('Inspect https://example.com'); flush()
    fireEvent.click(screen.getByRole('button', { name: 'Start task' }))
    setMessage('A different task'); flush()
    finish()
    await waitFor(() => expect(screen.getByRole<HTMLButtonElement>('button', { name: 'Start task' }).disabled).toBe(false))
    expect(screen.getByLabelText<HTMLTextAreaElement>('Task for Verevon').value).toBe('A different task')
  })
  it('shows and stops model-powered navigation without opening a browser view', () => {
    const [navigation, setNavigation] = createSignal<AssistantNavigationAgentView>({ status: 'thinking', error: null })
    const stop = vi.fn(), noop = () => undefined
    render(() => <GlobalAssistant forceReveal message="Find the knowledge page" onMessageChange={noop} onBeforeSubmit={() => false} canHandleBeforeSubmit={() => false} onSubmit={noop} onClose={noop} onStop={noop} onStopNavigation={stop} navigationAgent={navigation()} onOpenChat={noop} onNewChat={noop} sourceScope="workspace" onSourceScopeChange={noop} state={idleChat} />)
    expect(screen.getByText('Finding the page…')).toBeTruthy()
    expect(screen.queryByRole('region', { name: 'Agent browser' })).toBeNull()
    expect((screen.getByRole('button', { name: 'Start task' }) as HTMLButtonElement).disabled).toBe(true)
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }))
    expect(stop).toHaveBeenCalledOnce()
    setNavigation({ status: 'stopped', error: null }); flush()
    expect(screen.getByText('Stopped locally.')).toBeTruthy()
    setNavigation({ status: 'error', error: 'No page matched.' }); flush()
    expect(screen.getByRole('alert').textContent).toBe('No page matched.')
    expect(screen.queryByRole('button', { name: 'Stop' })).toBeNull()
  })

  it('describes in-app navigation without suggesting a separate browser', () => {
    setup({ ...base, kind: 'navigation' })
    expect(screen.getByText('Ask Verevon to find or open a page.')).toBeTruthy()
    expect(screen.queryByRole('region', { name: 'Agent browser' })).toBeNull()
  })

  it('does not advertise browser execution until the server confirms availability', () => {
    setup({ ...base, available: false })
    expect(screen.getByText('Agent tasks are not available yet. Verevon can show and open pages.')).toBeTruthy()
    expect(screen.queryByText(/include a web address/i)).toBeNull()
  })

  it('offers Nett and dispatches stop while a run is active', () => {
    const receipt = { runId: 'model-1', sessionRunId: 'session-1', planId: 'plan-1', attachId: 'attach-1', tabId: 'tab-1' }
    const { stop, openBrowser, setMessage, send } = setup({ ...base, status: 'running', goal: 'Inspect example.com', sessionId: 'session-1', runId: 'model-1', receipt })
    expect(screen.getByText('The agent is working')).toBeTruthy()
    expect(screen.queryByRole('region', { name: 'Agent browser' })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Watch agent in Web' }))
    expect(openBrowser).toHaveBeenCalledOnce()
    setMessage('another task'); flush()
    expect((screen.getByRole('button', { name: 'Start task' }) as HTMLButtonElement).disabled).toBe(true)
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }))
    expect(stop).toHaveBeenCalledOnce()
    expect(send).not.toHaveBeenCalled()
  })

  it('keeps status and controls while Nett displays the single browser stream', () => {
    const receipt = { runId: 'model-1', sessionRunId: 'session-1', planId: 'plan-1', attachId: 'attach-1', tabId: 'tab-1' }
    const { setWorkspaceVisible, stop } = setup({ ...base, status: 'running', goal: 'Inspect example.com', action: 'Reading the page', sessionId: 'session-1', runId: 'model-1', receipt })
    expect(screen.getByRole('button', { name: 'Watch agent in Web' })).toBeTruthy()
    setWorkspaceVisible(true); flush()
    expect(screen.queryByRole('button', { name: 'Watch agent in Web' })).toBeNull()
    expect(screen.getByText('Reading the page')).toBeTruthy()
    expect(screen.getByText('The agent is working')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }))
    expect(stop).toHaveBeenCalledOnce()
    setWorkspaceVisible(false); flush()
    expect(screen.getByRole('button', { name: 'Watch agent in Web' })).toBeTruthy()
    expect(screen.queryByRole('region', { name: 'Agent browser' })).toBeNull()
  })

  it('shows disconnection as uncertain and never claims that the run completed', () => {
    const { setAgent, stop } = setup({ ...base, status: 'disconnected', goal: 'Inspect example.com', runId: 'model-1' })
    expect(screen.getByText('Connection lost. The task may still be active.')).toBeTruthy()
    expect(screen.queryByText('The agent stopped.')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }))
    expect(stop).toHaveBeenCalledOnce()
    setAgent({ ...base, status: 'stopping' }); flush()
    expect((screen.getByRole('button', { name: 'Stop' }) as HTMLButtonElement).disabled).toBe(true)
    expect(screen.getByText('Asking the agent to stop…')).toBeTruthy()
  })

  it('offers recovery instead of a false stop when a start has no run id', () => {
    const { retry } = setup({ ...base, status: 'disconnected', sessionId: 'session-1' })
    expect(screen.queryByRole('button', { name: 'Stop' })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Recover task' }))
    expect(retry).toHaveBeenCalledOnce()
  })

  it('shows the actual failure without presenting a browser as live', () => {
    setup({ ...base, status: 'failed', error: 'Browser attachment was refused.' })
    expect(screen.getByRole('alert').textContent).toBe('Browser attachment was refused.')
    expect(screen.queryByRole('region', { name: 'Agent browser' })).toBeNull()
  })

  it('does not imply a result is available when only a terminal receipt exists', () => {
    setup({ ...base, status: 'completed', goal: 'Inspect example.com' })
    expect(screen.getByText('The run has ended.')).toBeTruthy()
    expect(screen.getByText('A result is not available in this view.')).toBeTruthy()
    expect(screen.queryByText('The agent task completed.')).toBeNull()
    expect(screen.queryByRole('region', { name: 'Agent browser' })).toBeNull()
  })

  it('displays the verified final result when it exists', () => {
    setup({ ...base, status: 'completed', goal: 'Inspect example.com', result: 'Found the requested page.' })
    expect(screen.getByText('Found the requested page.')).toBeTruthy()
    expect(screen.queryByText('A result is not available in this view.')).toBeNull()
  })

  it('shows the exact requested action and requires an explicit one-time approval', () => {
    const approval = { id: 'approval-1', actionId: 'action-1', action: 'Submit a form or content.', destination: 'https://example.com/form', target: 'button.submit', reason: 'Review before sending', canApprove: true, state: 'pending' as const, error: null }
    const { decide, setAgent } = setup({ ...base, status: 'running', runId: 'model-1', approval })
    expect(screen.getByText('Submit a form or content.')).toBeTruthy()
    expect(screen.getByText('https://example.com/form')).toBeTruthy()
    expect(screen.getByText('Target: button.submit')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Approve once' }))
    expect(decide).toHaveBeenCalledWith('approve')
    setAgent({ ...base, status: 'running', runId: 'model-1', approval: { ...approval, state: 'submitted' } }); flush()
    expect(screen.getByText('Decision sent. Waiting for confirmation…')).toBeTruthy()
    expect((screen.getByRole('button', { name: 'Approve once' }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('surfaces a pending decision in the minimized dock', () => {
    const { setAgent } = setup({ ...base, status: 'running', runId: 'model-1' })
    fireEvent.click(screen.getByRole('button', { name: 'Minimize chat bar' }))
    flush()
    expect(screen.getByRole('button', { name: 'Working' })).toBeTruthy()
    setAgent({ ...base, status: 'running', runId: 'model-1', approval: { id: 'approval-1', actionId: null, action: 'Submit a form.', destination: null, target: null, reason: null, canApprove: true, state: 'pending', error: null } }); flush()
    fireEvent.click(screen.getByRole('button', { name: 'Needs your review' }))
    flush()
    expect(screen.getByText('Submit a form.')).toBeTruthy()
  })
})
