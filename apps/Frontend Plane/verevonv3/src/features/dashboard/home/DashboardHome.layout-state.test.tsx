// @vitest-environment jsdom
import { fireEvent, render, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => ({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } }) }))
vi.mock('./DashboardComposer', () => ({ DashboardComposer: (props: { message: string }) => <input aria-label="Draft" value={props.message} /> }))
import { GlobalAssistant } from '@/features/core/components/GlobalAssistant'
import { homeLayoutStorageKey } from './home-layout-state'

const key = homeLayoutStorageKey('user-a', 'org-a')!
const state = { turns: [], status: 'idle' as const, error: null, activity: null, threadId: null, needsFullChat: false }
function setup(forceReveal = false) {
  const [message, setMessage] = createSignal('')
  const noop = () => undefined
  const view = render(() => <GlobalAssistant forceReveal={forceReveal} message={message()} onMessageChange={setMessage} onSubmit={noop} onBeforeSubmit={() => false} canHandleBeforeSubmit={() => false} onClose={noop} onStop={noop} onOpenChat={noop} onNewChat={noop} sourceScope="workspace" onSourceScopeChange={noop} state={state} />)
  return { ...view, setMessage }
}
beforeEach(() => { localStorage.clear(); sessionStorage.clear() })
afterEach(() => vi.restoreAllMocks())

describe('Shared assistant layout', () => {
  it('remembers the minimized layout for the current user and workspace', () => {
    const first = setup()
    fireEvent.click(screen.getByRole('button', { name: 'Minimize chat bar' })); flush()
    expect(screen.queryByLabelText('Task for Verevon')).toBeNull()
    expect(JSON.parse(localStorage.getItem(key)!)).toMatchObject({ dockMinimized: true })
    first.unmount()
    setup()
    expect(screen.getByRole('button', { name: 'Expand chat bar' })).toBeTruthy()
    expect(screen.queryByLabelText('Task for Verevon')).toBeNull()
  })

  it('reveals an explicit navbar opening without overwriting the saved Home preference', () => {
    const first = setup()
    fireEvent.click(screen.getByRole('button', { name: 'Minimize chat bar' })); flush()
    first.unmount()
    const explicit = setup(true)
    expect(screen.getByLabelText('Task for Verevon')).toBeTruthy()
    expect(JSON.parse(localStorage.getItem(key)!)).toMatchObject({ dockMinimized: true })
    explicit.unmount()
    setup()
    expect(screen.queryByLabelText('Task for Verevon')).toBeNull()
  })
  it('remembers expanded controls and restores them on the next opening', () => {
    const first = setup()
    fireEvent.click(screen.getByRole('button', { name: 'Expand composer' })); flush()
    expect(JSON.parse(localStorage.getItem(key)!)).toMatchObject({ dockExpanded: true, dockMinimized: false })
    first.unmount()
    setup()
    expect(screen.getByRole('button', { name: 'Make composer smaller' })).toBeTruthy()
  })

  it('reveals a suggested prompt without overwriting the remembered layout', () => {
    const first = setup()
    fireEvent.click(screen.getByRole('button', { name: 'Minimize chat bar' })); flush()
    first.setMessage('Write a summary'); flush()
    expect((screen.getByLabelText('Task for Verevon') as HTMLTextAreaElement).value).toBe('Write a summary')
    expect(JSON.parse(localStorage.getItem(key)!)).toMatchObject({ dockMinimized: true })
    first.unmount()
    setup()
    expect(screen.queryByLabelText('Task for Verevon')).toBeNull()
  })
})
