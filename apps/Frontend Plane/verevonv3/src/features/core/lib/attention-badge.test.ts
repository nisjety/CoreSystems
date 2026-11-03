import { afterEach, describe, expect, it, vi } from 'vitest'
import { applyAttentionBadge, attentionTitle, waitingThreadCount } from './attention-badge'

type BadgeNavigator = { setAppBadge?: unknown; clearAppBadge?: unknown }

describe('attention badge', () => {
  afterEach(() => {
    document.title = ''
    document.head.querySelectorAll('link[rel="icon"]').forEach((link) => link.remove())
    delete (navigator as BadgeNavigator).setAppBadge
    delete (navigator as BadgeNavigator).clearAppBadge
  })

  it('counts each waiting thread once and never a working, failed or unknown one', () => {
    expect(waitingThreadCount([
      { threadId: 'a', state: 'needs_approval' },
      { threadId: 'a', state: 'needs_approval' },
      { threadId: 'b', state: 'plan_ready' },
      { threadId: 'c', state: 'needs_input' },
      { threadId: 'd', state: 'working' },
      { threadId: 'e', state: 'failed' },
      { threadId: 'f' },
    ])).toBe(3)
    expect(waitingThreadCount([])).toBe(0)
  })

  it('puts the count in front of the title and replaces a count already there', () => {
    expect(attentionTitle('Verevon', 2)).toBe('(2) Verevon')
    expect(attentionTitle('(2) Verevon', 5)).toBe('(5) Verevon')
    expect(attentionTitle('(2) Verevon', 0)).toBe('Verevon')
    expect(attentionTitle('Verevon', 0)).toBe('Verevon')
  })

  it('writes the count to the tab title and clears it again when nothing waits', () => {
    document.title = 'Verevon'
    applyAttentionBadge(1)
    expect(document.title).toBe('(1) Verevon')
    applyAttentionBadge(1)
    expect(document.title).toBe('(1) Verevon')
    applyAttentionBadge(0)
    expect(document.title).toBe('Verevon')
  })

  it('uses the app badge where the browser has one, and works without it', () => {
    const setAppBadge = vi.fn().mockResolvedValue(undefined)
    const clearAppBadge = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'setAppBadge', { value: setAppBadge, configurable: true })
    Object.defineProperty(navigator, 'clearAppBadge', { value: clearAppBadge, configurable: true })

    applyAttentionBadge(3)
    expect(setAppBadge).toHaveBeenCalledWith(3)
    applyAttentionBadge(0)
    expect(clearAppBadge).toHaveBeenCalledTimes(1)

    delete (navigator as BadgeNavigator).setAppBadge
    delete (navigator as BadgeNavigator).clearAppBadge
    expect(() => applyAttentionBadge(2)).not.toThrow()
  })

  it('keeps the favicon as it was when nothing waits', () => {
    const link = document.createElement('link')
    link.rel = 'icon'
    link.href = '/favicon.svg'
    document.head.appendChild(link)

    applyAttentionBadge(0)

    expect(link.getAttribute('href')).toBe('/favicon.svg')
  })
})
