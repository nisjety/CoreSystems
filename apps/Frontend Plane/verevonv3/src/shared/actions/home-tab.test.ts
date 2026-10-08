import { describe, expect, it } from 'vitest'
import { homeTabActionSchema, homeTabParam, homeTabParamSchema, homeTabSchema, parseHomeTab } from '@/shared/actions/home-tab'

describe('home tab URL contract', () => {
  it('maps the two tool params to their tabs', () => {
    expect(parseHomeTab('web')).toBe('Søk')
    expect(parseHomeTab('extract')).toBe('Crawl')
  })

  it('treats an absent param as Chat', () => {
    expect(parseHomeTab(undefined)).toBe('Chat')
    expect(parseHomeTab(null)).toBe('Chat')
    expect(parseHomeTab('')).toBe('Chat')
  })

  it('treats unknown, cased, internal or repeated values as Chat', () => {
    for (const value of ['chat', 'Chat', 'WEB', 'Web', ' web', 'Søk', 'Crawl', 'search', 'crawl', 'unknown', 0, true, {}, ['web'], ['web', 'extract']]) {
      expect(parseHomeTab(value)).toBe('Chat')
    }
  })

  it('writes the param for a tool and no param for Chat', () => {
    expect(homeTabParam('Søk')).toBe('web')
    expect(homeTabParam('Crawl')).toBe('extract')
    expect(homeTabParam('Chat')).toBeUndefined()
  })

  it('round-trips every tab through its param', () => {
    for (const tab of homeTabSchema.options) expect(parseHomeTab(homeTabParam(tab))).toBe(tab)
    for (const param of homeTabParamSchema.options) expect(homeTabParam(parseHomeTab(param))).toBe(param)
  })

  it('validates the select action', () => {
    expect(homeTabActionSchema.parse({ type: 'home.tab.select', tab: 'Søk' })).toEqual({ type: 'home.tab.select', tab: 'Søk' })
    expect(homeTabActionSchema.safeParse({ type: 'home.tab.select', tab: 'web' }).success).toBe(false)
    expect(homeTabActionSchema.safeParse({ type: 'home.overview.select', tab: 'Chat' }).success).toBe(false)
  })
})
