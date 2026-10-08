// @vitest-environment jsdom
import { createRoot, createStore, flush, untrack } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))
import { __resetRetentionForTests, applyServerRetention } from '@/features/chat/lib/chat-retention'
import {
  WEB_HISTORY_CARD_ROWS,
  WEB_HISTORY_MAX_ENTRIES,
  WEB_HISTORY_MAX_TEXT,
  WEB_HISTORY_MAX_URL,
  addWebHistory,
  cleanWebHistoryText,
  cleanWebHistoryUrl,
  clearWebHistory,
  readWebHistory,
  recordWebPage,
  recordWebQuery,
  removeWebHistory,
  useWebHistory,
  webHistoryCardRows,
  webHistoryHost,
  webHistoryStorageKey,
  type WebHistoryEntry,
} from './web-history'

type Identity = { user: { id: string } | null; activeOrg: { id: string } | null }
const [identity, setIdentity] = createStore<Identity>({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const signIn = (user: string | null, org: string | null) => {
  setIdentity((state) => { state.user = user ? { id: user } : null; state.activeOrg = org ? { id: org } : null })
  flush()
}
const keyA = webHistoryStorageKey('user-a', 'org-a')!
const keyB = webHistoryStorageKey('user-b', 'org-a')!
const stored = (key: string) => JSON.parse(localStorage.getItem(key) ?? 'null')
const at = (minute: number) => new Date(Date.UTC(2026, 8, 29, 10, minute))
const query = (text: string, minute = 0) => addWebHistory(keyA, { kind: 'query', text }, at(minute))
const page = (url: string, title: string | null = null, minute = 0) => addWebHistory(keyA, { kind: 'page', text: title, url }, at(minute))
const summary = (entries: WebHistoryEntry[]) => entries.map((entry) => (entry.kind === 'query' ? `q:${entry.text}` : `p:${entry.url}`))
const roots: Array<() => void> = []
// The test setup swaps in plain in-memory storage objects, so spies go on those objects (both backends are written).
const spyOnStorage = (method: 'getItem' | 'setItem') => {
  const spies = [vi.spyOn(localStorage, method), vi.spyOn(sessionStorage, method)]
  return { calls: () => spies.flatMap((spy) => spy.mock.calls) }
}

beforeEach(() => {
  localStorage.clear(); sessionStorage.clear()
  signIn('user-a', 'org-a')
})
afterEach(() => {
  vi.restoreAllMocks()
  vi.useRealTimers()
  for (const dispose of roots.splice(0)) dispose()
  __resetRetentionForTests()
})

describe('web history storage key', () => {
  it('is scoped by workspace and user, like the chat organizer', () => {
    expect(keyA).toBe('verevon.home-web-history.v1:org-a:user-a')
    expect(webHistoryStorageKey('a:b/c', 'o rg')).toBe('verevon.home-web-history.v1:o%20rg:a%3Ab%2Fc')
  })

  it('exists only when both the user and the workspace are known', () => {
    for (const [user, org] of [[undefined, 'org-a'], ['user-a', undefined], [undefined, undefined], ['', 'org-a'], ['user-a', ''], ['  ', 'org-a'], ['user-a', ' ']] as const) {
      expect(webHistoryStorageKey(user, org)).toBeNull()
    }
  })
})

describe('cleaning a stored URL', () => {
  it('keeps only absolute http and https URLs', () => {
    expect(cleanWebHistoryUrl('https://example.com/a?x=1#top')).toBe('https://example.com/a?x=1#top')
    expect(cleanWebHistoryUrl('  http://example.com  ')).toBe('http://example.com/')
    for (const bad of ['javascript:alert(1)', 'data:text/html,<b>x</b>', 'file:///etc/passwd', 'ftp://example.com/f', 'mailto:a@example.com', 'blob:https://example.com/1', '/relative/path', 'example.com', '//example.com', 'https://', '', '   ', 'not a url']) {
      expect(cleanWebHistoryUrl(bad), bad).toBeNull()
    }
    for (const bad of [undefined, null, 42, {}, ['https://example.com']]) expect(cleanWebHistoryUrl(bad)).toBeNull()
  })

  it('strips a user name and password from the address', () => {
    expect(cleanWebHistoryUrl('https://alice:secret@example.com/private?q=1')).toBe('https://example.com/private?q=1')
    expect(cleanWebHistoryUrl('http://token@example.com/')).toBe('http://example.com/')
  })

  it('drops an over-long URL instead of cutting it into another page', () => {
    const long = `https://example.com/${'a'.repeat(WEB_HISTORY_MAX_URL)}`
    expect(cleanWebHistoryUrl(long)).toBeNull()
    const fits = `https://example.com/${'a'.repeat(WEB_HISTORY_MAX_URL - 'https://example.com/'.length)}`
    expect(cleanWebHistoryUrl(fits)).toBe(fits)
  })
})

describe('cleaning stored text', () => {
  it('trims, joins lines and removes control and invisible characters', () => {
    expect(cleanWebHistoryText('  hello \n\t world  ')).toBe('hello world')
    expect(cleanWebHistoryText('a\u0000b\u0007c')).toBe('a b c')
    expect(cleanWebHistoryText('pay\u202Epal\u200Bx\u2028y\uFEFFz')).toBe('pay pal x y z')
    expect(cleanWebHistoryText('   ')).toBe('')
  })

  it('cuts to the limit and never ends on half of a surrogate pair', () => {
    expect(cleanWebHistoryText('x'.repeat(WEB_HISTORY_MAX_TEXT + 50))).toHaveLength(WEB_HISTORY_MAX_TEXT)
    const cut = cleanWebHistoryText(`${'x'.repeat(WEB_HISTORY_MAX_TEXT - 1)}😀tail`)
    expect(cut).toBe('x'.repeat(WEB_HISTORY_MAX_TEXT - 1))
    expect(cut.charCodeAt(cut.length - 1)).toBeLessThan(0xd800)
  })

  it('names a host without www.', () => {
    expect(webHistoryHost('https://www.example.com/a')).toBe('example.com')
    expect(webHistoryHost('not a url')).toBe('not a url')
  })
})

describe('recording searches and pages', () => {
  it('puts the newest entry first and stamps it with an ISO time', () => {
    query('first', 1); query('second', 2)
    page('https://example.com/a', 'Example A', 3)
    const entries = readWebHistory(keyA)
    expect(summary(entries)).toEqual(['p:https://example.com/a', 'q:second', 'q:first'])
    expect(entries[0]).toEqual({ kind: 'page', text: 'Example A', url: 'https://example.com/a', at: at(3).toISOString() })
    expect(entries[2]).toEqual({ kind: 'query', text: 'first', at: at(1).toISOString() })
  })

  it('moves a repeated search to the top, ignoring case and spacing, and keeps the latest wording', () => {
    query('Quarterly report', 1); query('other', 2)
    const next = query('  quarterly   REPORT ', 3)
    expect(summary(next)).toEqual(['q:quarterly REPORT', 'q:other'])
    expect(next[0]!.at).toBe(at(3).toISOString())
  })

  it('moves a repeated page to the top and takes its newest title', () => {
    page('https://example.com/a', 'Old title', 1); query('between', 2)
    const next = page('https://example.com/a', 'New title', 3)
    expect(summary(next)).toEqual(['p:https://example.com/a', 'q:between'])
    expect(next[0]).toMatchObject({ text: 'New title' })
  })

  it('treats a search and a page as different entries even when the text matches', () => {
    query('https://example.com/a', 1)
    const next = page('https://example.com/a', 'https://example.com/a', 2)
    expect(next).toHaveLength(2)
  })

  it('keeps at most ten entries and drops the oldest', () => {
    for (let index = 1; index <= WEB_HISTORY_MAX_ENTRIES + 3; index += 1) query(`search ${index}`, index)
    const entries = readWebHistory(keyA)
    expect(entries).toHaveLength(WEB_HISTORY_MAX_ENTRIES)
    expect(entries[0]).toMatchObject({ text: `search ${WEB_HISTORY_MAX_ENTRIES + 3}` })
    expect(entries.at(-1)).toMatchObject({ text: 'search 4' })
    expect(stored(keyA)).toHaveLength(WEB_HISTORY_MAX_ENTRIES)
  })

  it('does not let a repeat take a second slot at the cap', () => {
    for (let index = 1; index <= WEB_HISTORY_MAX_ENTRIES; index += 1) query(`search ${index}`, index)
    query('search 1', 20)
    const entries = readWebHistory(keyA)
    expect(entries).toHaveLength(WEB_HISTORY_MAX_ENTRIES)
    expect(entries[0]).toMatchObject({ text: 'search 1' })
    expect(summary(entries).filter((entry) => entry === 'q:search 1')).toHaveLength(1)
  })

  it('rejects a page whose URL is not http or https and leaves the record as it was', () => {
    query('kept', 1)
    const before = localStorage.getItem(keyA)
    for (const url of ['javascript:alert(1)', 'data:text/html,x', 'file:///c:/secret', '/relative', 'ftp://example.com/x', '']) {
      expect(summary(page(url, 'Bad page', 2)), url).toEqual(['q:kept'])
    }
    expect(localStorage.getItem(keyA)).toBe(before)
  })

  it('rejects an empty search', () => {
    query('kept', 1)
    const before = localStorage.getItem(keyA)
    expect(summary(query('   ', 2))).toEqual(['q:kept'])
    expect(summary(query('\u0000\u200b', 3))).toEqual(['q:kept'])
    expect(localStorage.getItem(keyA)).toBe(before)
  })

  it('strips credentials from a page before it is stored', () => {
    page('https://alice:hunter2@example.com/inbox?tab=1', 'Inbox', 1)
    expect(localStorage.getItem(keyA)).not.toContain('hunter2')
    expect(localStorage.getItem(keyA)).not.toContain('alice')
    expect(readWebHistory(keyA)[0]).toMatchObject({ kind: 'page', url: 'https://example.com/inbox?tab=1' })
  })

  it('treats the same page with and without credentials as one entry', () => {
    page('https://example.com/a', 'A', 1)
    expect(page('https://bob:pw@example.com/a', 'A again', 2)).toHaveLength(1)
  })

  it('limits the stored text and names a page by its host when it has no title', () => {
    expect(query('q'.repeat(WEB_HISTORY_MAX_TEXT + 80), 1)[0]!.text).toHaveLength(WEB_HISTORY_MAX_TEXT)
    expect(page('https://www.example.com/deep/path', null, 2)[0]).toMatchObject({ text: 'example.com' })
    expect(page('https://www.example.org/x', '   ', 3)[0]).toMatchObject({ text: 'example.org' })
    expect(page('https://example.net/x', 'T'.repeat(WEB_HISTORY_MAX_TEXT + 5), 4)[0]!.text).toHaveLength(WEB_HISTORY_MAX_TEXT)
  })

  it('writes nothing without a key', () => {
    const write = spyOnStorage('setItem')
    expect(addWebHistory(null, { kind: 'query', text: 'x' })).toEqual([])
    expect(write.calls()).toEqual([])
  })
})

describe('reading the stored record', () => {
  const good = { kind: 'query', text: 'ok', at: at(1).toISOString() }

  it('is empty when nothing is stored, and does not touch storage without a key', () => {
    expect(readWebHistory(keyA)).toEqual([])
    const read = spyOnStorage('getItem')
    expect(readWebHistory(null)).toEqual([])
    expect(read.calls()).toEqual([])
  })

  it('reads a corrupt or non-list record as empty', () => {
    for (const raw of ['{broken', '"text"', '42', 'null', JSON.stringify({ kind: 'query', text: 'x', at: at(1).toISOString() })]) {
      localStorage.setItem(keyA, raw)
      expect(readWebHistory(keyA), raw).toEqual([])
    }
  })

  it('skips invalid entries and keeps the valid ones in order', () => {
    localStorage.setItem(keyA, JSON.stringify([
      good,
      { kind: 'other', text: 'x', at: at(1).toISOString() },
      { kind: 'query', at: at(1).toISOString() },
      { kind: 'query', text: '   ', at: at(1).toISOString() },
      { kind: 'query', text: 'no time' },
      { kind: 'query', text: 'bad time', at: 'yesterday' },
      { kind: 'page', text: 'no url', at: at(1).toISOString() },
      { kind: 'page', text: 'script', url: 'javascript:alert(1)', at: at(1).toISOString() },
      { kind: 'page', text: 'data', url: 'data:text/html,x', at: at(1).toISOString() },
      'a string', 7, null, [], { kind: 'query', text: 5, at: at(1).toISOString() },
      { kind: 'page', text: 'fine', url: 'https://example.com/fine', at: at(2).toISOString() },
    ]))
    expect(summary(readWebHistory(keyA))).toEqual(['q:ok', 'p:https://example.com/fine'])
  })

  it('cleans what a hand-edited record holds: credentials, control characters, length, time format', () => {
    localStorage.setItem(keyA, JSON.stringify([
      { kind: 'page', text: 'Bad\u0000 title', url: 'https://u:p@example.com/x', at: '2026-09-29T10:00:00+02:00' },
      { kind: 'query', text: 'y'.repeat(500), at: at(1).toISOString() },
    ]))
    const [first, second] = readWebHistory(keyA) as [WebHistoryEntry, WebHistoryEntry]
    expect(first).toEqual({ kind: 'page', text: 'Bad title', url: 'https://example.com/x', at: '2026-09-29T08:00:00.000Z' })
    expect(second.text).toHaveLength(WEB_HISTORY_MAX_TEXT)
  })

  it('applies the duplicate and size rules to a hand-edited record too', () => {
    const many = Array.from({ length: 40 }, (_, index) => ({ kind: 'query', text: `q${index % 20}`, at: at(1).toISOString() }))
    localStorage.setItem(keyA, JSON.stringify(many))
    const entries = readWebHistory(keyA)
    expect(entries).toHaveLength(WEB_HISTORY_MAX_ENTRIES)
    expect(new Set(summary(entries)).size).toBe(WEB_HISTORY_MAX_ENTRIES)
  })

  it('only looks at the head of an oversized record', () => {
    localStorage.setItem(keyA, JSON.stringify([...Array.from({ length: 100 }, () => 'junk'), good]))
    expect(readWebHistory(keyA)).toEqual([])
  })

  it('does not render an entry that another build wrote with a script URL', () => {
    localStorage.setItem(keyA, JSON.stringify([{ kind: 'page', text: 'x', url: 'javascript:alert(document.cookie)', at: at(1).toISOString() }]))
    expect(readWebHistory(keyA)).toEqual([])
  })
})

describe('recording for the signed-in person', () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['Date'] })
    vi.setSystemTime(at(5))
  })

  it('stores a search and a page under the current identity', () => {
    recordWebQuery('agent status')
    recordWebPage({ url: 'https://example.com/status', title: 'Status' })
    expect(summary(readWebHistory(keyA))).toEqual(['p:https://example.com/status', 'q:agent status'])
    expect(stored(keyA)[0]).toMatchObject({ at: at(5).toISOString() })
  })

  it('does nothing until both the user and the workspace are known', () => {
    const write = spyOnStorage('setItem')
    for (const [user, org] of [[null, 'org-a'], ['user-a', null], [null, null]] as const) {
      signIn(user, org)
      recordWebQuery('too early')
      recordWebPage({ url: 'https://example.com/early', title: 'Early' })
      clearWebHistory()
    }
    expect(write.calls()).toEqual([])
    expect(localStorage.length).toBe(0)
  })

  it('ignores a page that is not http or https, and an empty search', () => {
    recordWebQuery('   ')
    recordWebPage({ url: 'javascript:alert(1)', title: 'Bad' })
    recordWebPage({ url: '/relative', title: 'Bad' })
    expect(localStorage.length).toBe(0)
  })

  it('writes to the identity that is current when it happens, never an earlier one', () => {
    recordWebQuery('as user a')
    signIn('user-b', 'org-a')
    recordWebQuery('as user b')
    signIn('user-a', 'org-b')
    recordWebQuery('as user a in org b')
    expect(summary(readWebHistory(keyA))).toEqual(['q:as user a'])
    expect(summary(readWebHistory(keyB))).toEqual(['q:as user b'])
    expect(summary(readWebHistory(webHistoryStorageKey('user-a', 'org-b')))).toEqual(['q:as user a in org b'])
  })

  it('clears only the current identity\'s record', () => {
    recordWebQuery('mine')
    signIn('user-b', 'org-a')
    recordWebQuery('theirs')
    clearWebHistory()
    expect(localStorage.getItem(keyB)).toBeNull()
    expect(summary(readWebHistory(keyA))).toEqual(['q:mine'])
  })

  it('removeWebHistory needs a key', () => {
    query('kept', 1)
    removeWebHistory(null)
    expect(localStorage.getItem(keyA)).not.toBeNull()
    removeWebHistory(keyA)
    expect(localStorage.getItem(keyA)).toBeNull()
  })

  it('survives storage that refuses writes', () => {
    for (const storage of [localStorage, sessionStorage]) vi.spyOn(storage, 'setItem').mockImplementation(() => { throw new Error('quota') })
    expect(() => { recordWebQuery('x'); recordWebPage({ url: 'https://example.com/x' }) }).not.toThrow()
  })
})

describe('a zero-data-retention workspace', () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['Date'] })
    vi.setSystemTime(at(5))
  })

  it('records no search and no page, and writes nothing at all', () => {
    applyServerRetention(true)
    const write = spyOnStorage('setItem')

    recordWebQuery('salary negotiation lawyer')
    recordWebPage({ url: 'https://example.com/lawyers', title: 'Lawyers' })

    expect(write.calls()).toEqual([])
    expect(localStorage.length + sessionStorage.length).toBe(0)
  })

  it('removes what was recorded, for every account in this browser, the moment the server says so', () => {
    recordWebQuery('before the verdict')
    signIn('user-b', 'org-a')
    recordWebPage({ url: 'https://example.com/other', title: 'Other' })
    expect(localStorage.getItem(keyA)).not.toBeNull()
    expect(localStorage.getItem(keyB)).not.toBeNull()

    applyServerRetention(true)

    expect(localStorage.getItem(keyA)).toBeNull()
    expect(localStorage.getItem(keyB)).toBeNull()
    expect(sessionStorage.getItem(keyA)).toBeNull()
    expect(sessionStorage.getItem(keyB)).toBeNull()
  })

  it('records again once the workspace allows local copies', () => {
    applyServerRetention(true)
    recordWebQuery('during the ban')
    applyServerRetention(false)
    recordWebQuery('after the ban')
    expect(summary(readWebHistory(keyA))).toEqual(['q:after the ban'])
  })

  it('leaves the layout record alone', () => {
    const layoutKey = 'verevon.home-layout.v1:org-a:user-a'
    localStorage.setItem(layoutKey, '{"dockMinimized":true}')
    applyServerRetention(true)
    expect(localStorage.getItem(layoutKey)).toBe('{"dockMinimized":true}')
  })
})

describe('the remembered history for the signed-in person', () => {
  function mount() {
    let history!: ReturnType<typeof useWebHistory>
    roots.push(createRoot((dispose) => { history = useWebHistory(); return dispose }))
    return () => summary(untrack(history))
  }

  it('reads a known identity\'s record as soon as it is created', () => {
    query('kept', 1)
    expect(mount()()).toEqual(['q:kept'])
  })

  it('follows what is recorded and cleared, in every mounted reader', () => {
    const first = mount()
    const second = mount()
    expect(first()).toEqual([])
    recordWebQuery('one'); flush()
    recordWebPage({ url: 'https://example.com/two', title: 'Two' }); flush()
    expect(first()).toEqual(['p:https://example.com/two', 'q:one'])
    expect(second()).toEqual(first())
    clearWebHistory(); flush()
    expect(first()).toEqual([])
    expect(second()).toEqual([])
  })

  it('shows each identity its own record when the person or workspace changes', () => {
    query('a search', 1)
    addWebHistory(keyB, { kind: 'query', text: 'b search' }, at(2))
    const read = mount()
    expect(read()).toEqual(['q:a search'])
    signIn('user-b', 'org-a')
    expect(read()).toEqual(['q:b search'])
    signIn('user-a', 'org-b')
    expect(read()).toEqual([])
    signIn(null, null)
    expect(read()).toEqual([])
    signIn('user-a', 'org-a')
    expect(read()).toEqual(['q:a search'])
  })

  it('shows nothing, and reads nothing, while the identity is unknown', () => {
    query('kept', 1)
    signIn(null, 'org-a')
    const read = spyOnStorage('getItem')
    expect(mount()()).toEqual([])
    expect(read.calls()).toEqual([])
  })

  it('sign-out and sign-in leave the record in place, like the other per-account keys', () => {
    query('kept', 1)
    signIn(null, null)
    signIn('user-a', 'org-a')
    expect(mount()()).toEqual(['q:kept'])
    expect(localStorage.getItem(keyA)).not.toBeNull()
  })
})

describe('the rows the card shows', () => {
  const entry = (kind: 'query' | 'page', index: number): WebHistoryEntry => (
    kind === 'query'
      ? { kind, text: `query ${index}`, at: at(index).toISOString() }
      : { kind, text: `page ${index}`, url: `https://example.com/${index}`, at: at(index).toISOString() }
  )

  it('shows up to four rows, newest first', () => {
    const entries = Array.from({ length: 8 }, (_, index) => entry(index % 2 ? 'page' : 'query', index))
    expect(WEB_HISTORY_CARD_ROWS).toBe(4)
    expect(webHistoryCardRows(entries)).toEqual(entries.slice(0, 4))
  })

  it('lets a run of pages leave room for the latest searches, and the other way round', () => {
    const pages = Array.from({ length: 6 }, (_, index) => entry('page', index))
    const searches = [entry('query', 10), entry('query', 11), entry('query', 12)]
    expect(webHistoryCardRows([...pages, ...searches]).map((row) => row.text)).toEqual(['page 0', 'page 1', 'query 10', 'query 11'])
    expect(webHistoryCardRows([...searches, ...pages]).map((row) => row.text)).toEqual(['query 10', 'query 11', 'page 0', 'page 1'])
  })

  it('gives the rows one kind leaves unused to the other', () => {
    const pages = Array.from({ length: 6 }, (_, index) => entry('page', index))
    expect(webHistoryCardRows(pages).map((row) => row.text)).toEqual(['page 0', 'page 1', 'page 2', 'page 3'])
    expect(webHistoryCardRows([entry('query', 1)])).toHaveLength(1)
    expect(webHistoryCardRows([])).toEqual([])
    const oneSearch = [...pages.slice(0, 5), entry('query', 9)]
    expect(webHistoryCardRows(oneSearch).map((row) => row.text)).toEqual(['page 0', 'page 1', 'page 2', 'query 9'])
  })
})
