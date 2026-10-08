// @vitest-environment jsdom
import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createEffect, createSignal, createStore, flush, onCleanup } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mode = vi.hoisted(() => ({ overview: true }))
const api = vi.hoisted(() => ({ listChatThreads: vi.fn() }))
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))
vi.mock('@/features/chat/lib/chat-appearance-preview', () => ({ chatAppearanceActive: () => mode.overview }))
vi.mock('@/features/chat/components/ChatPage', () => ({ default: () => null }))
vi.mock('@/shared/api/chat-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/chat-client')>()),
  listChatThreads: api.listChatThreads,
}))
// Stands in for the composer with the parts Home depends on: it edits the
// message, reports zero retention the way the real one does (on mount, on every
// change, `undefined` when it goes away) and resets then launches on send.
vi.mock('./DashboardComposer', () => ({
  DashboardComposer: (props: {
    message: string
    onMessageChange: (value: string) => void
    onLaunchStart?: () => void
    onZeroRetentionChange?: (zeroRetention: boolean | undefined) => void
  }) => {
    createEffect(() => temporary(), (value) => { props.onZeroRetentionChange?.(value) })
    onCleanup(() => props.onZeroRetentionChange?.(undefined))
    return (
      <>
        <textarea id="verevon-dashboard-input" aria-label="Draft" value={props.message} onInput={(event) => props.onMessageChange(event.currentTarget.value)} />
        <button type="button" onClick={() => { props.onMessageChange(''); props.onLaunchStart?.() }}>Send</button>
      </>
    )
  },
}))
vi.mock('./DashboardSearchPanel', () => ({ SearchPanel: () => <p>Web tool</p> }))
vi.mock('./KnowledgeComposer', () => ({ KnowledgeComposer: () => <p>Extract tool</p> }))
vi.mock('./DashboardCards', () => ({ DashboardCardsRail: () => null }))
vi.mock('./DashboardOverview', () => ({ DashboardOverview: (props: { onPrompt: (card: never) => void }) => <button onClick={() => props.onPrompt({ prompt: 'Skriv et sammendrag', promptEn: 'Write a summary' } as never)}>Prompt card</button> }))
vi.mock('./DashboardSectionOverview', () => ({ DashboardSectionOverview: () => null }))
import DashboardHome from './DashboardHome'
import { __resetRetentionForTests, applyServerRetention } from '@/features/chat/lib/chat-retention'
import { HOME_DRAFT_DEBOUNCE_MS } from './home-draft'
import { clearHomeDrafts, homeDraftStorageKey, readHomeDraft, writeHomeDraft } from '@/shared/session/home-draft-storage'

type Identity = { user: { id: string } | null; activeOrg: { id: string } | null }
const [identity, setIdentity] = createStore<Identity>({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const signIn = (user: string | null, org: string | null) => {
  setIdentity((state) => { state.user = user ? { id: user } : null; state.activeOrg = org ? { id: org } : null })
  flush()
}
const [temporary, setTemporary] = createSignal(false)
const keyA = homeDraftStorageKey('user-a', 'org-a')!
const keyB = homeDraftStorageKey('user-b', 'org-a')!

function setup(path = '/dashboard') {
  const TestRouter = createRouter({ history: memoryHistory(path), explicitLinks: true, routes: [{ path: '/*all', component: () => <DashboardHome /> }] })
  return render(() => <TestRouter>{(props) => props.children}</TestRouter>)
}
const draft = () => screen.getByLabelText<HTMLTextAreaElement>('Draft')
const type = (text: string) => { fireEvent.input(draft(), { target: { value: text } }); flush() }
const pause = (ms = HOME_DRAFT_DEBOUNCE_MS + 60) => new Promise((resolve) => setTimeout(resolve, ms))
const stored = () => window.sessionStorage.getItem(keyA)

beforeEach(() => {
  mode.overview = true
  api.listChatThreads.mockReset().mockResolvedValue([])
  setTemporary(false)
  signIn('user-a', 'org-a')
})
afterEach(() => {
  vi.restoreAllMocks()
  __resetRetentionForTests()
})

describe('the unsent Home draft', () => {
  it('survives a reload, and is on screen on the first render', async () => {
    const first = setup()
    type('Halvferdig melding')
    expect(stored()).toBeNull()
    await waitFor(() => expect(readHomeDraft(keyA)).toBe('Halvferdig melding'), { timeout: 1500 })
    expect(window.localStorage.getItem(keyA)).toBeNull()
    first.unmount()

    setup()
    expect(draft().value).toBe('Halvferdig melding')
  })

  it('is not written again for a draft that was only restored', async () => {
    writeHomeDraft(keyA, 'Fra forrige gang')
    const write = vi.spyOn(window.sessionStorage, 'setItem')
    setup()
    await pause()
    expect(draft().value).toBe('Fra forrige gang')
    expect(write).not.toHaveBeenCalled()
  })

  it('never overwrites what was typed before the account was known', async () => {
    writeHomeDraft(keyA, 'Eldre utkast')
    signIn(null, null)
    setup()
    type('Skrevet nå')
    signIn('user-a', 'org-a')
    expect(draft().value).toBe('Skrevet nå')
    await waitFor(() => expect(readHomeDraft(keyA)).toBe('Skrevet nå'), { timeout: 1500 })
  })

  it('stores a prompt card like any other draft', async () => {
    setup()
    fireEvent.click(screen.getByText('Prompt card')); flush()
    await waitFor(() => expect(draft().value).toBe('Write a summary'))
    await waitFor(() => expect(readHomeDraft(keyA)).toBe('Write a summary'), { timeout: 1500 })
  })

  it('is forgotten as soon as the message is sent', async () => {
    const first = setup()
    type('Send denne')
    await waitFor(() => expect(readHomeDraft(keyA)).toBe('Send denne'), { timeout: 1500 })

    fireEvent.click(screen.getByRole('button', { name: 'Send' })); flush()
    expect(stored()).toBeNull()
    await pause()
    expect(stored()).toBeNull()
    first.unmount()

    setup()
    expect(draft().value).toBe('')
  })

  it('does not bring back a draft that was sent within the debounce window', async () => {
    setup()
    type('Sendt raskt')
    fireEvent.click(screen.getByRole('button', { name: 'Send' })); flush()
    await pause()
    expect(stored()).toBeNull()
  })

  it('is kept when the person leaves the chat tab, without a composer to report', async () => {
    mode.overview = false
    const first = setup()
    type('Tar en tur innom nettsøk')
    fireEvent.click(screen.getByRole('button', { name: 'Web' })); flush()
    await waitFor(() => expect(screen.getByText('Web tool')).toBeTruthy())
    expect(readHomeDraft(keyA)).toBe('Tar en tur innom nettsøk')

    await pause()
    expect(readHomeDraft(keyA)).toBe('Tar en tur innom nettsøk')
    fireEvent.click(screen.getByRole('button', { name: 'Chat' })); flush()
    await waitFor(() => expect(screen.queryByText('Web tool')).toBeNull())
    expect(draft().value).toBe('Tar en tur innom nettsøk')
    first.unmount()
    expect(readHomeDraft(keyA)).toBe('Tar en tur innom nettsøk')
  })

  it('is written when Home is left in the middle of typing', () => {
    const view = setup()
    type('Forlater siden')
    view.unmount()
    expect(readHomeDraft(keyA)).toBe('Forlater siden')
  })
})

describe('the unsent Home draft in Temporary chat', () => {
  it('is never written while the next turn is zero-retention', async () => {
    setTemporary(true)
    const write = vi.spyOn(window.sessionStorage, 'setItem')
    const local = vi.spyOn(window.localStorage, 'setItem')
    const view = setup()
    type('Kun for denne økten')
    await pause()
    window.dispatchEvent(new Event('pagehide'))
    view.unmount()
    expect(write).not.toHaveBeenCalled()
    expect(local).not.toHaveBeenCalled()
    expect(stored()).toBeNull()
  })

  it('removes the stored draft when the person switches to Temporary chat', async () => {
    setup()
    type('Vanlig utkast')
    await waitFor(() => expect(readHomeDraft(keyA)).toBe('Vanlig utkast'), { timeout: 1500 })

    setTemporary(true); flush()
    expect(stored()).toBeNull()
    await pause()
    expect(stored()).toBeNull()
    expect(draft().value).toBe('Vanlig utkast')
  })

  it('does not store text that was typed in Temporary chat once it is switched off', async () => {
    setup()
    setTemporary(true); flush()
    type('Midlertidig')
    setTemporary(false); flush()
    await pause()
    expect(stored()).toBeNull()
  })
})

// Home has no Temporary chat switch, so its composer always reports an ordinary
// turn; the zero-retention state that can occur here is the workspace's.
describe('the unsent Home draft in a zero-data-retention workspace', () => {
  it('is never written once the server says so, although the composer reports an ordinary turn', async () => {
    applyServerRetention(true)
    const view = setup()
    type('Kundens kontrakt')
    await pause()
    window.dispatchEvent(new Event('pagehide'))
    view.unmount()
    expect(stored()).toBeNull()
    expect(window.localStorage.getItem(keyA)).toBeNull()
  })

  it('is removed when the verdict arrives after it was stored; the text stays on screen and is not stored again', async () => {
    setup()
    type('Kundens kontrakt')
    await waitFor(() => expect(readHomeDraft(keyA)).toBe('Kundens kontrakt'), { timeout: 1500 })

    applyServerRetention(true)
    expect(stored()).toBeNull()
    type('Kundens kontrakt, endret')
    await pause()

    expect(draft().value).toBe('Kundens kontrakt, endret')
    expect(stored()).toBeNull()
  })

  it('is removed without the overview too, where no recent-work card lists the conversations: Home asks the server itself', async () => {
    mode.overview = false
    writeHomeDraft(keyA, 'Kundens kontrakt')
    api.listChatThreads.mockImplementation(async () => { applyServerRetention(true); return [] })
    setup()
    await waitFor(() => expect(api.listChatThreads).toHaveBeenCalledTimes(1))
    expect(stored()).toBeNull()

    type('Kundens kontrakt, endret')
    await pause()
    expect(draft().value).toBe('Kundens kontrakt, endret')
    expect(stored()).toBeNull()
  })

  it('leaves the asking to the recent-work card on the Chat overview', async () => {
    setup()
    type('Vanlig utkast')
    await pause()
    expect(api.listChatThreads).not.toHaveBeenCalled()
    expect(readHomeDraft(keyA)).toBe('Vanlig utkast')
  })

  it('shows no draft from an earlier visit while the workspace denies local copies', () => {
    applyServerRetention(true)
    // A leftover the purge did not see, such as one written by a build without this gate.
    writeHomeDraft(keyA, 'Fra forrige gang')
    setup()
    expect(draft().value).toBe('')
  })
})

describe('the unsent Home draft per account', () => {
  it('shows each account its own draft', async () => {
    writeHomeDraft(keyB, 'Brukerens eget')
    setup()
    type('Utkast fra A')
    signIn('user-b', 'org-a')
    expect(draft().value).toBe('Brukerens eget')
    expect(readHomeDraft(keyA)).toBe('Utkast fra A')

    type('Utkast fra B')
    await waitFor(() => expect(readHomeDraft(keyB)).toBe('Utkast fra B'), { timeout: 1500 })
    expect(readHomeDraft(keyA)).toBe('Utkast fra A')
    signIn('user-a', 'org-a')
    expect(draft().value).toBe('Utkast fra A')
  })

  it('reads and writes nothing until the account is known', async () => {
    const read = vi.spyOn(window.sessionStorage, 'getItem')
    const write = vi.spyOn(window.sessionStorage, 'setItem')
    signIn('user-a', null)
    setup()
    type('Hei')
    await pause()
    expect(read).not.toHaveBeenCalled()
    expect(write).not.toHaveBeenCalled()
  })

  it('does not bring the draft back after sign-out', async () => {
    const view = setup()
    type('Skrevet like før utlogging')
    clearHomeDrafts()
    signIn(null, null)
    await pause()
    view.unmount()
    expect(stored()).toBeNull()
  })
})
