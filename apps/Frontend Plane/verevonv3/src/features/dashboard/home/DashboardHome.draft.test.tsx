// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { beforeEach, describe, expect, it, vi } from 'vitest'
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => ({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } }) }))
import { GlobalAssistant } from '@/features/core/components/GlobalAssistant'

const start = vi.fn<(text: string) => boolean>()
const submit = vi.fn<(text: string) => Promise<void>>()
function setup() {
  const [message, setMessage] = createSignal('')
  const noop = () => undefined
  const state = { turns: [], status: 'idle' as const, error: null, activity: null, threadId: null, needsFullChat: false }
  return render(() => <GlobalAssistant message={message()} onMessageChange={setMessage}
    onBeforeSubmit={start} canHandleBeforeSubmit={() => false} onSubmit={submit} onClose={noop} onStop={noop}
    onOpenChat={noop} onNewChat={() => setMessage('')} sourceScope="workspace" onSourceScopeChange={noop} state={state} />)
}
const input = () => screen.getByRole('textbox', { name: 'Task for Verevon' }) as HTMLTextAreaElement
const type = (value: string) => { fireEvent.input(input(), { target: { value } }); flush() }
beforeEach(() => { localStorage.clear(); sessionStorage.clear(); start.mockReset(); submit.mockReset() })

describe('assistant task input', () => {
  it('starts supported navigation without starting a model request', () => {
    start.mockReturnValue(true)
    setup(); type('where is knowledge')
    fireEvent.click(screen.getByRole('button', { name: 'Start task' })); flush()
    expect(start).toHaveBeenCalledWith('where is knowledge')
    expect(submit).not.toHaveBeenCalled()
    expect(input().value).toBe('')
  })
  it('retains an unavailable task and shows the error without starting chat', async () => {
    start.mockReturnValue(false)
    submit.mockRejectedValue(new Error('Agent tasks are not available yet. Your draft has been kept.'))
    setup(); type('Make a report')
    fireEvent.click(screen.getByRole('button', { name: 'Start task' })); flush()
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('not available yet'))
    expect(input().value).toBe('Make a report')
    expect(submit).toHaveBeenCalledExactlyOnceWith('Make a report')
    expect(sessionStorage.length).toBe(0)
  })
  it('clears an error for a new task and can subsequently navigate', async () => {
    start.mockReturnValue(false)
    submit.mockRejectedValue(new Error('Unavailable'))
    setup(); type('Make a report')
    fireEvent.click(screen.getByRole('button', { name: 'Start task' })); flush()
    await waitFor(() => expect(screen.getByRole('alert')).toBeTruthy())
    fireEvent.click(screen.getByRole('button', { name: 'New task' })); flush()
    expect(screen.queryByRole('alert')).toBeNull()
    expect(input().value).toBe('')
    start.mockReturnValue(true)
    type('where is knowledge')
    fireEvent.click(screen.getByRole('button', { name: 'Start task' })); flush()
    expect(start).toHaveBeenLastCalledWith('where is knowledge')
    expect(input().value).toBe('')
  })
  it('does not interpret slash commands as chat actions', async () => {
    start.mockReturnValue(false)
    submit.mockRejectedValue(new Error('Unavailable'))
    setup(); type('/schedule monthly report')
    fireEvent.keyDown(input(), { key: 'Enter' }); flush()
    await waitFor(() => expect(screen.getByRole('alert').textContent).toBe('Unavailable'))
    expect(input().value).toBe('/schedule monthly report')
    expect(submit).toHaveBeenCalledExactlyOnceWith('/schedule monthly report')
  })
})