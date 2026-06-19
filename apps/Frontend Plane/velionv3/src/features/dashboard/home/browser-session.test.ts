import { describe, expect, it } from 'vitest'
import type { BrowserSessionResponse } from '@/shared/api/browser-client'
import { attachBrowserSession, browserSessionFromPreview } from './browser-session'
import type { ScrapePreview } from './knowledge-preview'

function preview(): ScrapePreview {
  return {
    blocks: [
      { heading: true, raw: '# TriodeLab', text: 'TriodeLab' },
      { heading: false, raw: 'Digital transformasjon.', text: 'Digital transformasjon.' },
    ],
    charCount: 32,
    description: 'Digital rådgivning.',
    markdown: '# TriodeLab\n\nDigital transformasjon.',
    source: 'extract',
    title: 'TriodeLab',
    url: 'https://triodelab.no/',
  }
}

describe('browser session view model', () => {
  it('marks scrape-only previews as readability fallbacks', () => {
    const model = browserSessionFromPreview(preview())

    expect(model.renderMode).toBe('readability_fallback')
    expect(model.status).toBe('degraded')
    expect(model.domNodes.map((node) => node.text)).toEqual(['TriodeLab', 'Digital transformasjon.'])
  })

  it('prefers live Quarry browser observations when present', () => {
    const response: BrowserSessionResponse = {
      session: {
        capabilities: ['navigate', 'click', 'annotate'],
        frame: {
          artifactId: 'artifact-shot',
          kind: 'screenshot',
          mediaType: 'image/png',
          url: '/api/v1/browser/sessions/run-1/artifacts/artifact-shot',
        },
        id: 'run-1',
        leaseId: 'lease-1',
        profile: { scope: 'run_scoped', storage: 'isolated' },
        renderMode: 'chromium',
        status: 'live',
        title: 'Live title',
        url: 'https://triodelab.no/',
        viewport: { width: 1280, height: 800 },
      },
      observation: {
        console_summary: [
          { level: 'warning', text: 'Third-party script blocked' },
        ],
        dom_summary: {
          interactive_elements: [
            { selector: 'a[href="/kontakt"]', tag: 'a', text: 'Kontakt oss' },
          ],
          node_count: 42,
        },
        network_summary: [
          { content_type: 'text/html', method: 'GET', status: 200, url: 'https://triodelab.no/' },
        ],
        policy_denials: ['Blocked navigation to unknown.example'],
        run_id: 'run-1',
        screenshot_artifact_id: 'artifact-shot',
        step: 0,
        title: 'Rendered TriodeLab',
        url: 'https://triodelab.no/',
      },
    }

    const model = browserSessionFromPreview(attachBrowserSession(preview(), response))

    expect(model.renderMode).toBe('chromium')
    expect(model.status).toBe('live')
    expect(model.title).toBe('Rendered TriodeLab')
    expect(model.frameUrl).toBe('/api/v1/browser/sessions/run-1/artifacts/artifact-shot')
    expect(model.sourceLabel).toBe('Chromium frame')
    expect(model.screenshotArtifactId).toBe('artifact-shot')
    expect(model.nodeCount).toBe(42)
    expect(model.consoleEntries).toEqual([
      { level: 'warning', text: 'Third-party script blocked' },
    ])
    expect(model.networkEntries).toEqual([
      { content_type: 'text/html', method: 'GET', status: 200, url: 'https://triodelab.no/' },
    ])
    expect(model.policyDenials).toEqual(['Blocked navigation to unknown.example'])
    expect(model.domNodes).toEqual([
      { id: 'a-0', kind: 'a', selector: 'a[href="/kontakt"]', text: 'Kontakt oss' },
    ])
  })

  it('preserves browser profile metadata across action responses', () => {
    const current = attachBrowserSession(preview(), {
      session: {
        capabilities: ['navigate', 'back'],
        frame: null,
        id: 'run-1',
        leaseId: 'lease-1',
        profile: { id: 'prof_01JZ9XM7EXAMPLEPROFILE0001', scope: 'user_private', storage: 'persistent' },
        renderMode: 'chromium',
        status: 'live',
        title: 'Live title',
        url: 'https://triodelab.no/',
        viewport: { width: 1280, height: 800 },
      },
      observation: null,
    })

    const next = attachBrowserSession(current, {
      session: {
        capabilities: ['navigate', 'back', 'forward'],
        frame: null,
        id: 'run-1',
        leaseId: null,
        profile: { scope: 'run_scoped', storage: 'isolated' },
        renderMode: 'chromium',
        status: 'live',
        title: 'Next title',
        url: 'https://triodelab.no/kontakt',
        viewport: { width: 1280, height: 800 },
      },
      observation: null,
    })

    const model = browserSessionFromPreview(next)

    expect(model.profileLabel).toBe('prof_01JZ9XM7EXAMPLEPROFILE0001 · persistent')
    expect(next.browserSession?.session.leaseId).toBe('lease-1')
  })
})
