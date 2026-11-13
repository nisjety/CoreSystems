// @vitest-environment jsdom
import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { listModels, listSubscriptions } = vi.hoisted(() => ({ listModels: vi.fn(), listSubscriptions: vi.fn() }))
vi.mock('@/shared/api/chat-client', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/shared/api/chat-client')>(),
  listModels, listChatThreads: vi.fn().mockResolvedValue([]), saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
}))
vi.mock('@/shared/api/chatgpt-subscription-client', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/shared/api/chatgpt-subscription-client')>(),
  listChatGptSubscriptions: listSubscriptions,
}))
vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'admin' } }),
}))
vi.mock('@/features/chat/lib/prepare-chat-attachments', () => ({
  prepareChatAttachments: async (attachments: unknown[]) => attachments,
}))
import { LocalizedError } from '@/shared/i18n/errors'
import { DashboardComposer } from './DashboardComposer'

const intercept = vi.fn<(text: string) => boolean>()
const submit = vi.fn()
function ComposerPage(props: { browseWeb?: boolean; retrievalScope?: 'auto' | 'knowledge_only' }) {
  const [message, setMessage] = createSignal('')
  return <DashboardComposer message={message()} onMessageChange={setMessage} onBeforeSubmit={intercept}
    canHandleBeforeSubmit={(text) => text === 'hvor er kunskaps'}
    onSubmit={submit} browseWeb={props.browseWeb} retrievalScope={props.retrievalScope} />
}
function setup(props: { browseWeb?: boolean; retrievalScope?: 'auto' | 'knowledge_only' } = {}) {
  const TestRouter = createRouter({ routes: [{ path: '/*all', component: () => <ComposerPage {...props} /> }], history: memoryHistory(), explicitLinks: true })
  render(() => <TestRouter>{(route) => <>{route.children}</>}</TestRouter>)
}
function send(text: string) {
  const field = screen.getByRole('textbox')
  fireEvent.input(field, { target: { value: text } })
  flush()
  fireEvent.submit(field.closest('form')!)
}
beforeEach(() => {
  window.localStorage.clear()
  listModels.mockResolvedValue([])
  listSubscriptions.mockResolvedValue([])
  intercept.mockReset()
  submit.mockReset()
})
afterEach(() => { vi.clearAllMocks(); window.localStorage.clear() })

describe('local pre-submit handling', () => {
  it('handles a navigation request even while a selected subscription is unavailable', async () => {
    window.localStorage.setItem('verevon.ai-model-selection.v1:org-1', JSON.stringify({ model: 'gpt-5.6-luna', label: 'Luna', provider: 'openai-codex-subscription', subscriptionConnectionId: 'missing' }))
    intercept.mockReturnValue(true)
    setup()
    send('hvor er kunskaps')
    expect(intercept).toHaveBeenCalledWith('hvor er kunskaps')
    expect(submit).not.toHaveBeenCalled()
    expect((screen.getByRole('textbox') as HTMLTextAreaElement).value).toBe('')
  })
  it('enables Send only for a local request while the subscription is still being checked', async () => {
    window.localStorage.setItem('verevon.ai-model-selection.v1:org-1', JSON.stringify({ model: 'gpt-5.6-luna', label: 'Luna', provider: 'openai-codex-subscription', subscriptionConnectionId: 'missing' }))
    listSubscriptions.mockReturnValue(new Promise(() => {}))
    intercept.mockReturnValue(true)
    setup()
    await screen.findByText(/Kontrollerer ChatGPT-abonnementet/)
    const field = screen.getByRole('textbox')
    const button = screen.getByRole('button', { name: 'Send melding' }) as HTMLButtonElement
    fireEvent.input(field, { target: { value: 'En vanlig melding' } })
    flush()
    expect(button.disabled).toBe(true)
    fireEvent.input(field, { target: { value: 'hvor er kunskaps' } })
    flush()
    expect(button.disabled).toBe(false)
    fireEvent.click(button)
    expect(intercept).toHaveBeenCalledWith('hvor er kunskaps')
    expect(submit).not.toHaveBeenCalled()
  })
  it('keeps an unavailable assistant task as a draft and shows the error', async () => {
    intercept.mockReturnValue(false)
    submit.mockRejectedValue(new LocalizedError({ no: 'Agentoppgaver er ikke tilgjengelige ennå. Utkastet er beholdt.', en: 'Agent tasks are not available yet. Your draft has been kept.' }))
    setup()
    send('Lag en rapport')
    await screen.findByText('Agentoppgaver er ikke tilgjengelige ennå. Utkastet er beholdt.')
    expect((screen.getByRole('textbox') as HTMLTextAreaElement).value).toBe('Lag en rapport')
    expect(submit).toHaveBeenCalledOnce()
  })
  it('sends an ordinary message normally when the callback declines it', async () => {
    intercept.mockReturnValue(false)
    setup()
    send('Hei Verevon')
    await waitFor(() => expect(submit).toHaveBeenCalledOnce())
    expect(intercept).toHaveBeenCalledWith('Hei Verevon')
    expect(submit.mock.calls[0]?.[0].text).toBe('Hei Verevon')
  })
  it.each([{ browseWeb: true }, { retrievalScope: 'knowledge_only' as const }])('does not intercept an explicitly scoped or tooled request: %o', async (options) => {
    intercept.mockReturnValue(true)
    setup(options)
    send('hvor er kunskaps')
    await waitFor(() => expect(submit).toHaveBeenCalledOnce())
    expect(intercept).not.toHaveBeenCalled()
  })
  it('does not intercept a request with an attached file', async () => {
    intercept.mockReturnValue(true)
    setup()
    const input = document.querySelector<HTMLInputElement>('input[type="file"]')!
    fireEvent.change(input, { target: { files: [new File(['test'], 'test.txt', { type: 'text/plain' })] } })
    await waitFor(() => expect(screen.getByText('test.txt')).toBeTruthy())
    send('hvor er kunskaps')
    await waitFor(() => expect(submit).toHaveBeenCalledOnce())
    expect(intercept).not.toHaveBeenCalled()
    expect(submit.mock.calls[0]?.[0].attachments).toHaveLength(1)
  })
})
