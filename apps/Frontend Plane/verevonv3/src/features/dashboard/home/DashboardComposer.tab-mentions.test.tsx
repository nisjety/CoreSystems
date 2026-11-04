// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from 'vitest'

// E3.3-c: the `browserContext` a send carries. It names the session and the
// `@tab` tokens exactly as typed, and is a hint only: the gateway re-resolves
// every mention against Quarry's live tabs before anything reaches a model.

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: vi.fn().mockResolvedValue([]),
    getModelEntitlementMeta: vi.fn(() => undefined),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'member' } }),
}))

import { composerBrowserContext, tabMentionChipsEnabled } from './DashboardComposer'

afterEach(() => {
  vi.unstubAllEnvs()
})

describe('tabMentionChipsEnabled', () => {
  it('is off unless VITE_BROWSER_TAB_MENTIONS is exactly 1', () => {
    vi.stubEnv('VITE_BROWSER_TAB_MENTIONS', '')
    expect(tabMentionChipsEnabled()).toBe(false)
    vi.stubEnv('VITE_BROWSER_TAB_MENTIONS', 'true')
    expect(tabMentionChipsEnabled()).toBe(false)
    vi.stubEnv('VITE_BROWSER_TAB_MENTIONS', '1')
    expect(tabMentionChipsEnabled()).toBe(true)
  })
})

describe('composerBrowserContext', () => {
  it('carries each token as typed with the tab id it spells', () => {
    expect(composerBrowserContext('compare @tab with @TAB:tab_2, please', ' run-1 ', false)).toEqual({
      sessionId: 'run-1',
      tabMentions: [
        { token: '@tab', tabId: null },
        { token: '@TAB:tab_2', tabId: 'tab_2' },
      ],
    })
  })

  it('is omitted without a session, on a zero-retention turn, or without a token', () => {
    expect(composerBrowserContext('see @tab', null, false)).toBeUndefined()
    expect(composerBrowserContext('see @tab', '   ', false)).toBeUndefined()
    expect(composerBrowserContext('see @tab', 'run-1', true)).toBeUndefined()
    expect(composerBrowserContext('mail me@tab.example', 'run-1', false)).toBeUndefined()
    expect(composerBrowserContext('no mention here', 'run-1', false)).toBeUndefined()
  })
})
