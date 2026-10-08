// @vitest-environment jsdom
import { createRoot, createSignal, createStore, flush, untrack } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))
import { __resetRetentionForTests, applyServerRetention } from '@/features/chat/lib/chat-retention'
import {
  HOME_DRAFT_MAX_CHARS,
  clearHomeDrafts,
  homeDraftStorageKey,
  readHomeDraft,
  writeHomeDraft,
} from '@/shared/session/home-draft-storage'
import { HOME_DRAFT_DEBOUNCE_MS, useHomeDraft, type HomeDraft } from './home-draft'

type Identity = { user: { id: string } | null; activeOrg: { id: string } | null }
const [identity, setIdentity] = createStore<Identity>({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const signIn = (user: string | null, org: string | null) => {
  setIdentity((state) => { state.user = user ? { id: user } : null; state.activeOrg = org ? { id: org } : null })
  flush()
}
const keyA = homeDraftStorageKey('user-a', 'org-a')!
const keyB = homeDraftStorageKey('user-b', 'org-a')!
const keyAOrgB = homeDraftStorageKey('user-a', 'org-b')!

const [zeroRetention, setZeroRetention] = createSignal<boolean | undefined>(undefined)
/** What the composer reports: `false` ordinary, `true` Temporary chat, `undefined` gone. */
const composer = (value: boolean | undefined) => { setZeroRetention(value); flush() }

const roots: Array<() => void> = []
// Mounts the hook the way a component does and returns untracked accessors.
function mount() {
  let api!: HomeDraft
  let dispose!: () => void
  createRoot((d) => { dispose = d; api = useHomeDraft({ zeroRetention }) })
  roots.push(dispose)
  return {
    message: () => untrack(api[0]),
    type: (text: string) => { api[1](text); flush() },
    discard: api[2],
    unmount: dispose,
  }
}
const wait = (ms: number) => { vi.advanceTimersByTime(ms); flush() }
const writes = () => vi.spyOn(window.sessionStorage, 'setItem')

beforeEach(() => {
  vi.useFakeTimers()
  signIn('user-a', 'org-a')
  composer(undefined)
})
afterEach(() => {
  for (const dispose of roots.splice(0)) dispose()
  __resetRetentionForTests()
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('writing the draft', () => {
  it('writes after typing pauses, once, and only what was last typed', () => {
    const draft = mount()
    composer(false)
    const write = writes()
    draft.type('H')
    wait(200)
    draft.type('Hei')
    wait(HOME_DRAFT_DEBOUNCE_MS - 1)
    expect(readHomeDraft(keyA)).toBe('')
    wait(1)
    expect(readHomeDraft(keyA)).toBe('Hei')
    wait(5_000)
    expect(write).toHaveBeenCalledTimes(1)
    expect(HOME_DRAFT_DEBOUNCE_MS).toBe(400)
  })

  it('removes the key at once when the draft is emptied, and drops a write that was waiting', () => {
    const draft = mount()
    composer(false)
    draft.type('Hei')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    expect(readHomeDraft(keyA)).toBe('Hei')

    draft.type('Hei igjen')
    draft.type('')
    expect(window.sessionStorage.getItem(keyA)).toBeNull()
    wait(HOME_DRAFT_DEBOUNCE_MS * 2)
    expect(window.sessionStorage.getItem(keyA)).toBeNull()
  })

  it('treats a draft of only whitespace as empty', () => {
    const draft = mount()
    composer(false)
    draft.type('Hei')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    draft.type('  \n ')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    expect(window.sessionStorage.getItem(keyA)).toBeNull()
  })

  it('stores at most the first 20 000 characters', () => {
    const draft = mount()
    composer(false)
    draft.type('a'.repeat(HOME_DRAFT_MAX_CHARS + 5_000))
    wait(HOME_DRAFT_DEBOUNCE_MS)
    expect(readHomeDraft(keyA)).toHaveLength(HOME_DRAFT_MAX_CHARS)
    expect(draft.message()).toHaveLength(HOME_DRAFT_MAX_CHARS + 5_000)
  })

  it('writes to sessionStorage only', () => {
    const local = [vi.spyOn(window.localStorage, 'setItem'), vi.spyOn(window.localStorage, 'getItem'), vi.spyOn(window.localStorage, 'removeItem')]
    window.localStorage.setItem(keyA, JSON.stringify({ text: 'old copy' }))
    local[0]!.mockClear()
    const draft = mount()
    expect(draft.message()).toBe('')
    composer(false)
    draft.type('Hei')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    draft.type('')
    expect(readHomeDraft(keyA)).toBe('')
    for (const spy of local) expect(spy).not.toHaveBeenCalled()
  })

  it('saves the pending text when the document is hidden', () => {
    const draft = mount()
    composer(false)
    draft.type('Halvferdig')
    let state: DocumentVisibilityState = 'visible'
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => state })
    try {
      document.dispatchEvent(new Event('visibilitychange'))
      expect(readHomeDraft(keyA)).toBe('')
      state = 'hidden'
      document.dispatchEvent(new Event('visibilitychange'))
      expect(readHomeDraft(keyA)).toBe('Halvferdig')
    } finally {
      delete (document as { visibilityState?: unknown }).visibilityState
    }
  })

  it('saves the pending text on pagehide', () => {
    const draft = mount()
    composer(false)
    draft.type('Før reload')
    window.dispatchEvent(new Event('pagehide'))
    expect(readHomeDraft(keyA)).toBe('Før reload')
    const write = writes()
    wait(HOME_DRAFT_DEBOUNCE_MS * 2)
    expect(write).not.toHaveBeenCalled()
  })

  it('saves the pending text when Home goes away, and stops listening', () => {
    const draft = mount()
    composer(false)
    draft.type('På vei ut')
    draft.unmount()
    expect(readHomeDraft(keyA)).toBe('På vei ut')
    window.sessionStorage.removeItem(keyA)
    window.dispatchEvent(new Event('pagehide'))
    expect(window.sessionStorage.getItem(keyA)).toBeNull()
  })

  it('does not write on its own before the composer has reported an ordinary turn', () => {
    const draft = mount()
    const write = writes()
    draft.type('Hei')
    wait(HOME_DRAFT_DEBOUNCE_MS * 3)
    expect(write).not.toHaveBeenCalled()

    // The report opens the gate, and text that arrived before it (a card prompt) is kept.
    composer(false)
    wait(HOME_DRAFT_DEBOUNCE_MS)
    expect(readHomeDraft(keyA)).toBe('Hei')
  })

  it('keeps what was typed when the composer goes away, and writes nothing more until it is back', () => {
    const draft = mount()
    composer(false)
    draft.type('Skrevet')
    composer(undefined)
    expect(readHomeDraft(keyA)).toBe('Skrevet')

    draft.type('Skrevet og mer')
    wait(HOME_DRAFT_DEBOUNCE_MS * 2)
    expect(readHomeDraft(keyA)).toBe('Skrevet')
    composer(false)
    wait(HOME_DRAFT_DEBOUNCE_MS)
    expect(readHomeDraft(keyA)).toBe('Skrevet og mer')
  })

  it('leaves a full or blocked storage alone without losing the text on screen', () => {
    const draft = mount()
    composer(false)
    vi.spyOn(window.sessionStorage, 'setItem').mockImplementation(() => { throw new DOMException('full', 'QuotaExceededError') })
    draft.type('Hei')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    expect(draft.message()).toBe('Hei')
    expect(readHomeDraft(keyA)).toBe('')
  })
})

describe('restoring the draft', () => {
  it('shows the stored draft on the very first read', () => {
    writeHomeDraft(keyA, 'Fra forrige gang')
    const draft = mount()
    expect(draft.message()).toBe('Fra forrige gang')
  })

  it('does not write back what it restored', () => {
    writeHomeDraft(keyA, 'Fra forrige gang')
    const write = writes()
    const draft = mount()
    composer(false)
    wait(HOME_DRAFT_DEBOUNCE_MS * 2)
    expect(draft.message()).toBe('Fra forrige gang')
    expect(write).not.toHaveBeenCalled()
  })

  it('starts empty when nothing valid is stored', () => {
    window.sessionStorage.setItem(keyA, '{"text":42}')
    expect(mount().message()).toBe('')
    window.sessionStorage.setItem(keyA, 'not json')
    expect(mount().message()).toBe('')
  })

  it('restores once the identity becomes known, when nothing has been typed', () => {
    writeHomeDraft(keyA, 'Fra forrige gang')
    signIn(null, null)
    const draft = mount()
    expect(draft.message()).toBe('')
    signIn('user-a', 'org-a')
    expect(draft.message()).toBe('Fra forrige gang')
  })

  it('never overwrites text typed before the identity was known', () => {
    writeHomeDraft(keyA, 'Eldre utkast')
    signIn('user-a', null)
    const draft = mount()
    composer(false)
    draft.type('Skrevet nå')
    signIn('user-a', 'org-a')
    expect(draft.message()).toBe('Skrevet nå')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    expect(readHomeDraft(keyA)).toBe('Skrevet nå')
  })

  it('never overwrites text set before the composer reported, such as a card prompt', () => {
    writeHomeDraft(keyA, 'Eldre utkast')
    const draft = mount()
    draft.type('Skriv et sammendrag')
    composer(false)
    expect(draft.message()).toBe('Skriv et sammendrag')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    expect(readHomeDraft(keyA)).toBe('Skriv et sammendrag')
  })
})

describe('when the message is sent', () => {
  it('forgets the stored draft and any write that was waiting', () => {
    const draft = mount()
    composer(false)
    draft.type('Send meg')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    draft.type('Send meg nå')
    draft.discard()
    expect(window.sessionStorage.getItem(keyA)).toBeNull()
    wait(HOME_DRAFT_DEBOUNCE_MS * 2)
    expect(window.sessionStorage.getItem(keyA)).toBeNull()
  })

  it('writes again for the next thing that is typed', () => {
    const draft = mount()
    composer(false)
    draft.type('Første')
    draft.discard()
    draft.type('')
    draft.type('Neste')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    expect(readHomeDraft(keyA)).toBe('Neste')
  })
})

describe('Temporary chat (zero retention)', () => {
  it('never writes a draft typed while the next turn is zero-retention', () => {
    const draft = mount()
    composer(true)
    const write = writes()
    draft.type('Hemmelig')
    wait(HOME_DRAFT_DEBOUNCE_MS * 3)
    draft.type('Hemmelig og mer')
    window.dispatchEvent(new Event('pagehide'))
    wait(HOME_DRAFT_DEBOUNCE_MS * 3)
    draft.unmount()
    expect(write).not.toHaveBeenCalled()
    expect(window.sessionStorage.getItem(keyA)).toBeNull()
    expect(window.localStorage.getItem(keyA)).toBeNull()
  })

  it('removes a stored draft, and the write waiting for it, when the person switches to Temporary chat', () => {
    const draft = mount()
    composer(false)
    draft.type('Vanlig utkast')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    draft.type('Vanlig utkast, endret')
    expect(readHomeDraft(keyA)).toBe('Vanlig utkast')

    composer(true)
    expect(window.sessionStorage.getItem(keyA)).toBeNull()
    wait(HOME_DRAFT_DEBOUNCE_MS * 3)
    expect(window.sessionStorage.getItem(keyA)).toBeNull()
    expect(draft.message()).toBe('Vanlig utkast, endret')
  })

  it('does not store text that was in a zero-retention composer when Temporary chat is switched off again', () => {
    const draft = mount()
    composer(true)
    draft.type('Skrevet midlertidig')
    composer(false)
    wait(HOME_DRAFT_DEBOUNCE_MS * 2)
    draft.type('Skrevet midlertidig, og litt til')
    wait(HOME_DRAFT_DEBOUNCE_MS * 2)
    expect(window.sessionStorage.getItem(keyA)).toBeNull()

    // Emptied (sent), it is an ordinary composer again.
    draft.type('')
    draft.type('Ny vanlig melding')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    expect(readHomeDraft(keyA)).toBe('Ny vanlig melding')
  })

  it('stores the next text normally after a Temporary chat toggle over an empty composer', () => {
    const draft = mount()
    composer(false)
    composer(true)
    composer(false)
    draft.type('Vanlig')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    expect(readHomeDraft(keyA)).toBe('Vanlig')
  })

  it('fails closed: a draft already on screen is not stored by a composer that never reports', () => {
    const draft = mount()
    draft.type('Uten rapport')
    const write = writes()
    wait(HOME_DRAFT_DEBOUNCE_MS * 5)
    window.dispatchEvent(new Event('pagehide'))
    draft.unmount()
    expect(write).not.toHaveBeenCalled()
  })
})

// Home has no Temporary chat switch, so its composer always reports an ordinary
// turn. The zero-retention case that can occur there is the workspace's: the
// server's verdict (chat-retention), which closes the gate whatever the composer says.
describe('A zero-data-retention workspace', () => {
  it('never writes a draft, although the composer reports an ordinary turn', () => {
    applyServerRetention(true)
    const draft = mount()
    composer(false)
    const write = writes()
    draft.type('Kundens kontrakt')
    wait(HOME_DRAFT_DEBOUNCE_MS * 3)
    draft.type('Kundens kontrakt og mer')
    document.dispatchEvent(new Event('visibilitychange'))
    window.dispatchEvent(new Event('pagehide'))
    wait(HOME_DRAFT_DEBOUNCE_MS * 3)
    draft.unmount()
    expect(write).not.toHaveBeenCalled()
    expect(window.sessionStorage.getItem(keyA)).toBeNull()
    expect(window.localStorage.getItem(keyA)).toBeNull()
  })

  it('removes a stored draft, and the write waiting for it, the moment the verdict arrives; the text on screen stays', () => {
    const draft = mount()
    composer(false)
    draft.type('Vanlig utkast')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    draft.type('Vanlig utkast, endret')
    expect(readHomeDraft(keyA)).toBe('Vanlig utkast')

    applyServerRetention(true)

    expect(window.sessionStorage.getItem(keyA)).toBeNull()
    wait(HOME_DRAFT_DEBOUNCE_MS * 3)
    window.dispatchEvent(new Event('pagehide'))
    draft.unmount()
    expect(window.sessionStorage.getItem(keyA)).toBeNull()
    expect(draft.message()).toBe('Vanlig utkast, endret')
  })

  it('does not store what was on screen when the verdict lifts, until the composer has been emptied', () => {
    const draft = mount()
    composer(false)
    applyServerRetention(true)
    draft.type('Skrevet under forbudet')
    applyServerRetention(false)
    wait(HOME_DRAFT_DEBOUNCE_MS * 2)
    draft.type('Skrevet under forbudet, og litt til')
    wait(HOME_DRAFT_DEBOUNCE_MS * 2)
    expect(window.sessionStorage.getItem(keyA)).toBeNull()

    draft.type('')
    draft.type('Ny vanlig melding')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    expect(readHomeDraft(keyA)).toBe('Ny vanlig melding')
  })

  it('stores drafts again for text typed after the workspace allows local copies', () => {
    const draft = mount()
    composer(false)
    applyServerRetention(true)
    applyServerRetention(false)
    draft.type('Vanlig igjen')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    expect(readHomeDraft(keyA)).toBe('Vanlig igjen')
  })

  it('does not store text that was already on screen when the verdict arrived, once it lifts, until the composer has been emptied', () => {
    const draft = mount()
    composer(false)
    draft.type('Kundens kontrakt')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    expect(readHomeDraft(keyA)).toBe('Kundens kontrakt')

    applyServerRetention(true)
    applyServerRetention(false)
    draft.type('Kundens kontrakt, endret')
    wait(HOME_DRAFT_DEBOUNCE_MS * 2)
    expect(window.sessionStorage.getItem(keyA)).toBeNull()

    draft.type('')
    draft.type('Ny melding')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    expect(readHomeDraft(keyA)).toBe('Ny melding')
  })

  it('restores nothing while the workspace denies local copies, even from a leftover in storage', () => {
    applyServerRetention(true)
    writeHomeDraft(keyA, 'Igjen fra før')
    const draft = mount()
    expect(draft.message()).toBe('')
    signIn('user-b', 'org-a')
    signIn('user-a', 'org-a')
    expect(draft.message()).toBe('')
  })

  it('keeps a draft on screen that was restored before the verdict arrived, and stores nothing more', () => {
    writeHomeDraft(keyA, 'Fra forrige side')
    const draft = mount()
    composer(false)
    expect(draft.message()).toBe('Fra forrige side')

    applyServerRetention(true)
    draft.type('Fra forrige side, endret')
    wait(HOME_DRAFT_DEBOUNCE_MS * 3)

    expect(draft.message()).toBe('Fra forrige side, endret')
    expect(window.sessionStorage.getItem(keyA)).toBeNull()
  })
})

describe('identity', () => {
  it('reads and writes nothing until both the user and the workspace are known', () => {
    writeHomeDraft(keyA, 'Fra forrige gang')
    const read = vi.spyOn(window.sessionStorage, 'getItem')
    const write = writes()
    const remove = vi.spyOn(window.sessionStorage, 'removeItem')
    signIn('user-a', null)
    const draft = mount()
    composer(false)
    draft.type('Hei')
    wait(HOME_DRAFT_DEBOUNCE_MS * 2)
    draft.type('')
    expect(draft.message()).toBe('')
    for (const spy of [read, write, remove]) expect(spy).not.toHaveBeenCalled()
    signIn(null, 'org-a')
    draft.type('Hei igjen')
    wait(HOME_DRAFT_DEBOUNCE_MS * 2)
    expect(write).not.toHaveBeenCalled()
  })

  it('shows the new user their own draft, saves the previous one first, and never crosses them', () => {
    writeHomeDraft(keyB, 'Brukerens eget utkast')
    const draft = mount()
    composer(false)
    draft.type('Utkast fra A')

    signIn('user-b', 'org-a')
    expect(readHomeDraft(keyA)).toBe('Utkast fra A')
    expect(draft.message()).toBe('Brukerens eget utkast')

    draft.type('Utkast fra B')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    expect(readHomeDraft(keyB)).toBe('Utkast fra B')
    expect(readHomeDraft(keyA)).toBe('Utkast fra A')

    signIn('user-a', 'org-a')
    expect(draft.message()).toBe('Utkast fra A')
    wait(HOME_DRAFT_DEBOUNCE_MS * 2)
    expect(readHomeDraft(keyB)).toBe('Utkast fra B')
  })

  it('does the same when the workspace changes, and starts empty where nothing is stored', () => {
    const draft = mount()
    composer(false)
    draft.type('Utkast i første')
    signIn('user-a', 'org-b')
    expect(draft.message()).toBe('')
    expect(readHomeDraft(keyA)).toBe('Utkast i første')
    draft.type('Utkast i andre')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    expect(readHomeDraft(keyAOrgB)).toBe('Utkast i andre')
    expect(readHomeDraft(keyA)).toBe('Utkast i første')
  })

  it('does not write a change to the previous identity after the switch', () => {
    const draft = mount()
    composer(false)
    draft.type('Fra A')
    wait(HOME_DRAFT_DEBOUNCE_MS)
    signIn('user-b', 'org-a')
    draft.type('Fra B')
    signIn('user-b', 'org-b')
    wait(HOME_DRAFT_DEBOUNCE_MS * 2)
    expect(readHomeDraft(keyA)).toBe('Fra A')
    expect(readHomeDraft(keyB)).toBe('Fra B')
  })

  it('keeps text that was never stored on screen across a switch, and does not store it for the new identity', () => {
    writeHomeDraft(keyB, 'Brukerens eget utkast')
    const draft = mount()
    composer(true)
    draft.type('Midlertidig tekst')
    signIn('user-b', 'org-a')
    expect(draft.message()).toBe('Midlertidig tekst')
    composer(false)
    wait(HOME_DRAFT_DEBOUNCE_MS * 2)
    expect(readHomeDraft(keyB)).toBe('Brukerens eget utkast')
    expect(readHomeDraft(keyA)).toBe('')
  })

  it('drops a write that was waiting at sign-out instead of bringing the draft back', () => {
    const draft = mount()
    composer(false)
    draft.type('Skrevet like før')
    clearHomeDrafts()
    signIn(null, null)
    wait(HOME_DRAFT_DEBOUNCE_MS * 2)
    window.dispatchEvent(new Event('pagehide'))
    draft.unmount()
    expect(window.sessionStorage.getItem(keyA)).toBeNull()
  })

  it('drops a write scheduled before the drafts were cleared, even if the identity has not changed yet', () => {
    const draft = mount()
    composer(false)
    draft.type('Skrevet like før')
    clearHomeDrafts()
    wait(HOME_DRAFT_DEBOUNCE_MS * 2)
    draft.unmount()
    expect(window.sessionStorage.getItem(keyA)).toBeNull()
  })
})
