// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import {
  cleanBookmarkUrl,
  readBookmarks,
  toggleBookmark,
} from './browser-bookmarks'

describe('cleanBookmarkUrl', () => {
  it('keeps plain http(s) URLs and drops credentials', () => {
    expect(cleanBookmarkUrl('https://example.com/path?q=1')).toBe('https://example.com/path?q=1')
    expect(cleanBookmarkUrl('https://user:pw@example.com/')).toBe('https://example.com/')
  })

  it('refuses everything else', () => {
    for (const value of ['', 'not a url', 'javascript:alert(1)', 'ftp://example.com/', 'http://', 42, null]) {
      expect(cleanBookmarkUrl(value), String(value)).toBeNull()
    }
  })
})

describe('toggleBookmark', () => {
  it('saves then removes the same page', () => {
    const key = 'verevon.browser-bookmarks.v1:org-a:user-1'
    const saved = toggleBookmark(key, { url: 'https://example.com/', label: 'Example' })
    expect(saved.map((bookmark) => bookmark.url)).toEqual(['https://example.com/'])
    expect(readBookmarks(key)).toEqual(saved)
    expect(toggleBookmark(key, { url: 'https://example.com/' })).toEqual([])
    expect(readBookmarks(key)).toEqual([])
  })

  it('keeps a corrupt record from breaking the list', () => {
    const key = 'verevon.browser-bookmarks.v1:org-a:user-2'
    localStorage.setItem(key, JSON.stringify(['bogus', { url: 'javascript:alert(1)', label: 'x', at: '2026-11-06T00:00:00Z' }]))
    expect(readBookmarks(key)).toEqual([])
  })
})
