import { describe, expect, it } from 'vitest'
import type { BrowserTab } from '@/shared/api/browser-client'
import {
  formatTabMention,
  parseTabMentions,
  resolveTabMentions,
  safeTabTarget,
  uniqueTabRefs,
} from './tab-mention'

function tab(overrides: Partial<BrowserTab> & { tabId: string }): BrowserTab {
  return { active: false, title: null, url: null, ...overrides }
}

describe('parseTabMentions', () => {
  it('finds a bare @tab with its span', () => {
    expect(parseTabMentions('@tab what is this?')).toEqual([{ start: 0, end: 4, tabId: null }])
  })

  it('reads an explicit tab id after the colon', () => {
    expect(parseTabMentions('compare @tab:tab-2 and @tab:Main_1')).toEqual([
      { start: 8, end: 18, tabId: 'tab-2' },
      { start: 23, end: 34, tabId: 'Main_1' },
    ])
  })

  it('matches the keyword case-insensitively and keeps the id case', () => {
    expect(parseTabMentions('@Tab:AbC')).toEqual([{ start: 0, end: 8, tabId: 'AbC' }])
  })

  it('accepts whitespace or opening punctuation before the @ and closing punctuation after', () => {
    const found = parseTabMentions('(@tab) "@tab:t1", @tab.')
    expect(found.map((mention) => mention.tabId)).toEqual([null, 't1', null])
    expect(found[0]?.start).toBe(1)
  })

  it('ignores @tabs, @table, emails and empty or invalid ids', () => {
    expect(parseTabMentions('@tabs and @table')).toEqual([])
    expect(parseTabMentions('write to me@tab.no')).toEqual([])
    expect(parseTabMentions('@tab: nothing')).toEqual([])
    expect(parseTabMentions('@tab:a/b')).toEqual([])
    expect(parseTabMentions(`@tab:${'x'.repeat(65)}`)).toEqual([])
  })

  it('accepts a 64 character id and rejects 65', () => {
    expect(parseTabMentions(`@tab:${'x'.repeat(64)}`)).toHaveLength(1)
  })

  it('finds adjacent and repeated mentions in order', () => {
    const found = parseTabMentions('@tab @tab:b @tab')
    expect(found.map((mention) => mention.start)).toEqual([0, 5, 12])
  })

  it('returns nothing for a draft with no token', () => {
    expect(parseTabMentions('')).toEqual([])
    expect(parseTabMentions('plain text with @someone')).toEqual([])
  })
})

describe('formatTabMention', () => {
  it('formats the bare and the explicit token', () => {
    expect(formatTabMention()).toBe('@tab')
    expect(formatTabMention(null)).toBe('@tab')
    expect(formatTabMention('tab-9')).toBe('@tab:tab-9')
  })

  it('refuses an id that could not be parsed back', () => {
    expect(formatTabMention('has space')).toBeNull()
    expect(formatTabMention('')).toBeNull()
    expect(formatTabMention('a'.repeat(65))).toBeNull()
  })

  it('round-trips through the parser', () => {
    const text = formatTabMention('tab-9') ?? ''
    expect(parseTabMentions(text)).toEqual([{ start: 0, end: text.length, tabId: 'tab-9' }])
  })
})

describe('resolveTabMentions', () => {
  const tabs = [
    tab({ tabId: 'a', active: false, title: 'Inbox', url: 'https://mail.example.com/u/1?token=x#top' }),
    tab({ tabId: 'b', active: true, title: '  ', url: 'https://docs.example.com/page' }),
  ]

  it('resolves a bare @tab to the single active tab with no grant', () => {
    const [result] = resolveTabMentions(parseTabMentions('@tab'), tabs)
    expect(result).toEqual({
      status: 'resolved',
      mention: { start: 0, end: 4, tabId: null },
      ref: {
        kind: 'browser-tab',
        tabId: 'b',
        label: 'https://docs.example.com/page',
        target: 'https://docs.example.com/page',
        grant: 'none',
      },
    })
  })

  it('resolves an explicit id and drops the query string and fragment from the target', () => {
    const [result] = resolveTabMentions(parseTabMentions('@tab:a'), tabs)
    expect(result?.status).toBe('resolved')
    if (result?.status !== 'resolved') return
    expect(result.ref.label).toBe('Inbox')
    expect(result.ref.target).toBe('https://mail.example.com/u/1')
  })

  it('reports no-tabs when the list is empty or missing', () => {
    expect(resolveTabMentions(parseTabMentions('@tab'), [])).toMatchObject([{ reason: 'no-tabs' }])
    expect(resolveTabMentions(parseTabMentions('@tab'), null)).toMatchObject([{ reason: 'no-tabs' }])
  })

  it('reports no-active-tab when nothing is active, and ambiguous-tab when two are', () => {
    const none = [tab({ tabId: 'a' }), tab({ tabId: 'b' })]
    expect(resolveTabMentions(parseTabMentions('@tab'), none)).toMatchObject([{ reason: 'no-active-tab' }])
    const two = [tab({ tabId: 'a', active: true }), tab({ tabId: 'b', active: true })]
    expect(resolveTabMentions(parseTabMentions('@tab'), two)).toMatchObject([{ reason: 'ambiguous-tab' }])
  })

  it('reports unknown-tab for an id that is not in the list', () => {
    expect(resolveTabMentions(parseTabMentions('@tab:zzz'), tabs)).toMatchObject([{ reason: 'unknown-tab' }])
  })

  it('reports ambiguous-tab when an explicit id appears twice', () => {
    const dup = [tab({ tabId: 'a' }), tab({ tabId: 'a', active: true })]
    expect(resolveTabMentions(parseTabMentions('@tab:a'), dup)).toMatchObject([{ reason: 'ambiguous-tab' }])
  })

  it('ignores tabs whose id is not usable, so they can never match', () => {
    const bad = [tab({ tabId: 'bad id', active: true }), tab({ tabId: 'ok' })]
    expect(resolveTabMentions(parseTabMentions('@tab'), bad)).toMatchObject([{ reason: 'no-active-tab' }])
    // A token cannot spell "bad id", so build the mention directly to prove the filter.
    const direct = { start: 0, end: 8, tabId: 'bad id' }
    expect(resolveTabMentions([direct], bad)).toMatchObject([{ reason: 'unknown-tab' }])
  })

  it('refuses every mention under zero retention, even with tabs present', () => {
    const results = resolveTabMentions(parseTabMentions('@tab @tab:a'), tabs, { zeroRetention: true })
    expect(results.map((result) => (result.status === 'unresolved' ? result.reason : result.status))).toEqual([
      'zero-retention',
      'zero-retention',
    ])
  })

  it('keeps one result per mention in draft order', () => {
    const results = resolveTabMentions(parseTabMentions('@tab:b @tab:nope @tab:a'), tabs)
    expect(results.map((result) => (result.status === 'resolved' ? result.ref.tabId : result.reason))).toEqual([
      'b',
      'unknown-tab',
      'a',
    ])
  })
})

describe('label fallback', () => {
  it('uses the target when the title is blank, and the tab id when there is no http(s) URL', () => {
    const [byTarget] = resolveTabMentions(parseTabMentions('@tab:t'), [
      tab({ tabId: 't', title: '   ', url: 'https://x.example/p' }),
    ])
    expect(byTarget).toMatchObject({ status: 'resolved', ref: { label: 'https://x.example/p' } })
    const [byId] = resolveTabMentions(parseTabMentions('@tab:t'), [
      tab({ tabId: 't', title: null, url: 'about:blank' }),
    ])
    expect(byId).toMatchObject({ status: 'resolved', ref: { label: 't', target: null } })
  })
})

describe('safeTabTarget', () => {
  it('keeps origin and path only', () => {
    expect(safeTabTarget('https://user:pw@example.com:8443/a/b?q=1#frag')).toBe('https://example.com:8443/a/b')
  })

  it('returns null for non-http schemes, blank and unparsable input', () => {
    expect(safeTabTarget('javascript:alert(1)')).toBeNull()
    expect(safeTabTarget('about:blank')).toBeNull()
    expect(safeTabTarget('  ')).toBeNull()
    expect(safeTabTarget(undefined)).toBeNull()
    expect(safeTabTarget('not a url')).toBeNull()
  })
})

describe('uniqueTabRefs', () => {
  it('returns distinct resolved refs and drops unresolved mentions', () => {
    const tabs = [tab({ tabId: 'a', active: true }), tab({ tabId: 'b' })]
    const results = resolveTabMentions(parseTabMentions('@tab @tab:a @tab:b @tab:zz'), tabs)
    expect(uniqueTabRefs(results).map((ref) => ref.tabId)).toEqual(['a', 'b'])
  })
})
