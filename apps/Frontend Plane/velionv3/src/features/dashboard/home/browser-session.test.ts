import { describe, expect, it } from 'vitest'
import type { BrowserSessionResponse } from '@/shared/api/browser-client'
import {
  artifactsForTimelineEntry,
  attachBrowserSession,
  browserSessionFromPreview,
  describeTimelineDelta,
  evidenceIsEphemeral,
  rationaleForStep,
  timelineDetail,
  withStepRationale,
  type BrowserStepRationale,
  type BrowserTimelineViewEntry,
} from './browser-session'
import { createBrowserLoopController, isBrowserLoopRunning, type BrowserLoopState } from './browser-loop'
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
        timeline: [
          {
            screenshotArtifactId: 'artifact-shot',
            screenshotUrl: '/api/v1/browser/sessions/run-1/artifacts/artifact-shot',
            step: 0,
            title: 'Rendered TriodeLab',
            url: 'https://triodelab.no/',
            visualObservationArtifactId: 'art_vision_1',
            visualObservationUrl: '/api/v1/browser/sessions/run-1/artifacts/art_vision_1',
          },
        ],
        title: 'Live title',
        url: 'https://triodelab.no/',
        visual: {
          observationArtifactId: 'art_vision_1',
          observationUrl: '/api/v1/browser/sessions/run-1/artifacts/art_vision_1',
        },
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
        visual_observation_artifact_id: 'art_vision_1',
      },
    }

    const model = browserSessionFromPreview(attachBrowserSession(preview(), response))

    expect(model.renderMode).toBe('chromium')
    expect(model.status).toBe('live')
    expect(model.title).toBe('Rendered TriodeLab')
    expect(model.frameUrl).toBe('/api/v1/browser/sessions/run-1/artifacts/artifact-shot')
    expect(model.sourceLabel).toBe('Chromium frame')
    expect(model.screenshotArtifactId).toBe('artifact-shot')
    expect(model.visualObservationArtifactId).toBe('art_vision_1')
    expect(model.visualObservationUrl).toBe('/api/v1/browser/sessions/run-1/artifacts/art_vision_1')
    expect(model.timeline).toHaveLength(1)
    expect(model.timeline[0]?.screenshotUrl).toBe('/api/v1/browser/sessions/run-1/artifacts/artifact-shot')
    expect(model.timeline[0]?.visualObservationUrl).toBe('/api/v1/browser/sessions/run-1/artifacts/art_vision_1')
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

  it('builds a visual observation URL from raw Quarry observation metadata', () => {
    const response: BrowserSessionResponse = {
      session: {
        capabilities: ['navigate', 'visual_observation'],
        frame: null,
        id: 'run-1',
        leaseId: null,
        profile: { scope: 'run_scoped', storage: 'isolated' },
        renderMode: 'chromium',
        status: 'live',
        title: 'Live title',
        url: 'https://triodelab.no/',
        viewport: { width: 1280, height: 800 },
      },
      observation: {
        run_id: 'run-1',
        step: 0,
        title: 'Rendered TriodeLab',
        url: 'https://triodelab.no/',
        visual_observation_artifact_id: 'art_01JZ9XM7EXAMPLEVISION0001',
      },
    }

    const model = browserSessionFromPreview(attachBrowserSession(preview(), response))

    expect(model.visualObservationArtifactId).toBe('art_01JZ9XM7EXAMPLEVISION0001')
    expect(model.visualObservationUrl).toBe(
      '/api/v1/browser/sessions/run-1/artifacts/art_01JZ9XM7EXAMPLEVISION0001',
    )
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
        visual: {
          observationArtifactId: 'art_vision_1',
          observationUrl: '/api/v1/browser/sessions/run-1/artifacts/art_vision_1',
        },
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
    expect(model.profileId).toBe('prof_01JZ9XM7EXAMPLEPROFILE0001')
    expect(model.profileScope).toBe('user_private')
    expect(model.profileStorage).toBe('persistent')
    expect(model.visualObservationArtifactId).toBe('art_vision_1')
    expect(next.browserSession?.session.leaseId).toBe('lease-1')
  })

  it('surfaces the gateway ZDR marker and per-step evidence on timeline entries', () => {
    const response: BrowserSessionResponse = {
      session: {
        capabilities: ['navigate'],
        frame: null,
        id: 'run-1',
        leaseId: null,
        profile: { scope: 'run_scoped', storage: 'isolated' },
        renderMode: 'chromium',
        status: 'live',
        timeline: [
          {
            consoleSummary: [{ level: 'error', text: 'boom' }],
            domInteractiveCount: 3,
            domNodeCount: 41,
            networkSummary: [{ method: 'GET', status: 404, url: 'https://triodelab.no/x' }],
            policyDenials: ['Blocked download'],
            step: 1,
            title: 'Steg 1',
            url: 'https://triodelab.no/kontakt',
          },
        ],
        title: 'Live title',
        url: 'https://triodelab.no/',
        viewport: { width: 1280, height: 800 },
        zdr: true,
      },
      observation: null,
    }

    const model = browserSessionFromPreview(attachBrowserSession(preview(), response))

    expect(model.zdr).toBe(true)
    expect(evidenceIsEphemeral(model)).toBe(true)
    expect(model.timeline[0]?.consoleSummary).toEqual([{ level: 'error', text: 'boom' }])
    expect(model.timeline[0]?.networkSummary?.[0]?.status).toBe(404)
    expect(model.timeline[0]?.policyDenials).toEqual(['Blocked download'])
    expect(model.timeline[0]?.domNodeCount).toBe(41)
    expect(model.timeline[0]?.domInteractiveCount).toBe(3)
  })
})

describe('evidence persistence marker', () => {
  it('does not claim ephemeral evidence for regular sessions (artifacts are retained)', () => {
    expect(evidenceIsEphemeral({ zdr: false })).toBe(false)
  })

  it('marks ZDR runs as ephemeral evidence', () => {
    expect(evidenceIsEphemeral({ zdr: true })).toBe(true)
  })
})

function timelineFixture(): BrowserTimelineViewEntry[] {
  return [
    {
      domNodeCount: 40,
      screenshotArtifactId: 'art_shot_0',
      screenshotUrl: '/api/v1/browser/sessions/run-1/artifacts/art_shot_0',
      step: 0,
      title: 'Forside',
      url: 'https://triodelab.no/',
    },
    {
      domNodeCount: 55,
      screenshotArtifactId: 'art_shot_1',
      screenshotUrl: '/api/v1/browser/sessions/run-1/artifacts/art_shot_1',
      step: 1,
      title: 'Kontakt',
      url: 'https://triodelab.no/kontakt',
      visualObservationArtifactId: 'art_vision_1',
      visualObservationUrl: '/api/v1/browser/sessions/run-1/artifacts/art_vision_1',
    },
  ]
}

describe('timeline detail selection', () => {
  it('returns the entry and its predecessor for before/after evidence', () => {
    const detail = timelineDetail(timelineFixture(), 1)

    expect(detail?.entry.step).toBe(1)
    expect(detail?.previous?.step).toBe(0)
  })

  it('has no predecessor for the first entry', () => {
    const detail = timelineDetail(timelineFixture(), 0)

    expect(detail?.entry.step).toBe(0)
    expect(detail?.previous).toBeNull()
  })

  it('returns null for unknown steps and live view', () => {
    expect(timelineDetail(timelineFixture(), 99)).toBeNull()
    expect(timelineDetail(timelineFixture(), null)).toBeNull()
  })

  it('computes deterministic deltas only from returned fields', () => {
    const detail = timelineDetail(timelineFixture(), 1)
    if (!detail) throw new Error('expected detail')

    expect(describeTimelineDelta(detail)).toEqual({
      domNodeDelta: 15,
      titleChanged: true,
      urlChanged: true,
    })
  })

  it('reports null DOM delta when counts are missing', () => {
    const timeline = timelineFixture().map((entry) => ({ ...entry, domNodeCount: null }))
    const detail = timelineDetail(timeline, 1)
    if (!detail) throw new Error('expected detail')

    expect(describeTimelineDelta(detail).domNodeDelta).toBeNull()
  })
})

describe('artifact descriptors', () => {
  it('maps screenshot and visual observation artifacts with media kinds', () => {
    const [entryWithBoth] = [timelineFixture()[1]]
    if (!entryWithBoth) throw new Error('expected entry')

    const artifacts = artifactsForTimelineEntry(entryWithBoth)

    expect(artifacts).toEqual([
      {
        artifactId: 'art_shot_1',
        kind: 'screenshot',
        label: 'screenshot.png',
        mediaKind: 'image',
        url: '/api/v1/browser/sessions/run-1/artifacts/art_shot_1',
      },
      {
        artifactId: 'art_vision_1',
        kind: 'visual_observation',
        label: 'visual_observation.json',
        mediaKind: 'json',
        url: '/api/v1/browser/sessions/run-1/artifacts/art_vision_1',
      },
    ])
  })

  it('omits artifacts the gateway did not return', () => {
    expect(artifactsForTimelineEntry({ step: 4 })).toEqual([])
  })
})

describe('model rationale records', () => {
  const rationale: BrowserStepRationale = {
    actionType: 'click',
    confidence: 0.8,
    done: false,
    goal: 'Finn kontaktinfo',
    modelUsed: 'claude-x',
    reason: 'Kontaktlenken er synlig i toppmenyen.',
    step: 2,
  }

  it('records and replaces rationales immutably per step', () => {
    const initial = withStepRationale([], rationale)
    const replaced = withStepRationale(initial, { ...rationale, reason: 'Oppdatert.' })

    expect(initial[0]?.reason).toBe('Kontaktlenken er synlig i toppmenyen.')
    expect(replaced).toHaveLength(1)
    expect(replaced[0]?.reason).toBe('Oppdatert.')
    expect(rationaleForStep(replaced, 2)?.actionType).toBe('click')
    expect(rationaleForStep(replaced, 3)).toBeNull()
    expect(rationaleForStep(replaced, null)).toBeNull()
  })
})

describe('browser AI loop controller', () => {
  it('runs idle -> suggesting -> acting and finishes done', async () => {
    const states: BrowserLoopState[] = []
    const loop = createBrowserLoopController((state) => states.push(state))

    expect(loop.begin('Capture evidence')).toBe(true)
    expect(loop.state().status).toBe('suggesting')
    expect(loop.state().goal).toBe('Capture evidence')
    expect(await loop.gate()).toBe('continue')
    loop.markActing()
    expect(loop.state().status).toBe('acting')
    loop.markStepDone()
    expect(loop.state().step).toBe(1)
    loop.finish('done')
    expect(loop.state().status).toBe('done')
    expect(states.map((state) => state.status)).toEqual(['suggesting', 'acting', 'acting', 'done'])
  })

  it('refuses to begin while a run is active and allows restart after done', () => {
    const loop = createBrowserLoopController()

    expect(loop.begin('a')).toBe(true)
    expect(loop.begin('b')).toBe(false)
    loop.finish('done')
    expect(loop.begin('c')).toBe(true)
  })

  it('pauses between steps and resumes where it left off', async () => {
    const loop = createBrowserLoopController()
    loop.begin('a')
    loop.requestPause()

    const pending = loop.gate()
    expect(loop.state().status).toBe('paused')
    expect(isBrowserLoopRunning(loop.state().status)).toBe(true)

    loop.requestResume()
    expect(await pending).toBe('continue')
    loop.markSuggesting()
    expect(loop.state().status).toBe('suggesting')
  })

  it('stop takes effect at the next gate without aborting the in-flight step', async () => {
    const loop = createBrowserLoopController()
    loop.begin('a')
    loop.markActing()
    loop.requestStop()
    expect(loop.state().status).toBe('acting')

    expect(await loop.gate()).toBe('stopped')
    expect(loop.state().status).toBe('stopped')
  })

  it('stop releases a paused loop', async () => {
    const loop = createBrowserLoopController()
    loop.begin('a')
    loop.requestPause()
    const pending = loop.gate()
    expect(loop.state().status).toBe('paused')

    loop.requestStop()
    expect(await pending).toBe('stopped')
    expect(loop.state().status).toBe('stopped')
  })

  it('ignores pause and stop when idle', () => {
    const loop = createBrowserLoopController()
    loop.requestPause()
    loop.requestStop()
    expect(loop.state().status).toBe('idle')

    expect(loop.begin('a')).toBe(true)
    expect(loop.state().status).toBe('suggesting')
  })
})
