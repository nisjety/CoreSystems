import { describe, expect, it } from 'vitest'
import type { BrowserSessionResponse } from '@/shared/api/browser-client'
import { attachBrowserSession, browserSessionFromPreview } from './browser-session'
import type { ScrapePreview } from './knowledge-preview'

function preview(): ScrapePreview {
  return {
    blocks: [{ heading: true, raw: '# Example', text: 'Example' }],
    charCount: 7,
    description: 'Example page.',
    markdown: '# Example',
    source: 'extract',
    title: 'Example',
    url: 'https://example.com/',
  }
}

function withSession(session: Partial<BrowserSessionResponse['session']>): ScrapePreview {
  return attachBrowserSession(preview(), {
    session: {
      capabilities: ['navigate', 'tabs'],
      id: 'run-1',
      profile: { scope: 'run_scoped', storage: 'isolated' },
      renderMode: 'chromium',
      status: 'live',
      title: 'Example',
      url: 'https://example.com/',
      viewport: { width: 1280, height: 800 },
      ...session,
    },
  })
}

describe('browser view model: control epoch and tab provenance (WS9.I)', () => {
  it('carries the control epoch and each tab opener through to the view', () => {
    const model = browserSessionFromPreview(withSession({
      control: { mode: 'human_takeover', epoch: 2 },
      tabs: [
        { active: true, tabId: 'tab-1', title: 'Example', url: 'https://example.com/', openedBy: 'agent' },
        { active: false, tabId: 'tab-2', title: null, url: 'about:blank', openedBy: 'human' },
        { active: false, tabId: 'tab-3', title: null, url: 'about:blank' },
      ],
    }))

    expect(model.controlMode).toBe('human_takeover')
    expect(model.controlEpoch).toBe(2)
    expect(model.tabs[0]).toMatchObject({ tabId: 'tab-1', openedBy: 'agent' })
    expect(model.tabs[1]).toMatchObject({ tabId: 'tab-2', openedBy: 'human' })
    expect(Object.prototype.hasOwnProperty.call(model.tabs[2] ?? {}, 'openedBy')).toBe(false)
  })

  it('leaves the epoch unset when the projection has none', () => {
    const model = browserSessionFromPreview(withSession({ control: { mode: 'agent_control' } }))

    expect(model.controlMode).toBe('agent_control')
    expect(model.controlEpoch).toBeUndefined()
  })
})
