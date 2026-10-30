/**
 * Scene runtime extras (scene 03): scene routes, run-event streams on the chat
 * POST's clock, and the fixture image mapping for `<img src>` evidence.
 */
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { getSceneLog, installSceneRuntime, setActiveSceneExtras, setActiveSceneStream, uninstallSceneRuntime } from './install'

async function readAll(response: Response): Promise<string> {
  const reader = response.body!.getReader()
  const decoder = new TextDecoder()
  let text = ''
  for (;;) {
    const { done, value } = await reader.read()
    if (done) break
    text += decoder.decode(value)
  }
  return text
}

function eventNames(body: string): string[] {
  return body.split('\n').filter((line) => line.startsWith('event: ')).map((line) => line.slice(7))
}

describe('scene runtime extras', () => {
  let sceneFetch: typeof window.fetch
  beforeAll(() => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance'] })
    installSceneRuntime()
    sceneFetch = window.fetch
  })
  // The shared test setup resets `fetch` before every test.
  beforeEach(() => {
    window.fetch = sceneFetch
  })
  afterAll(() => {
    setActiveSceneExtras(null)
    setActiveSceneStream(null)
    vi.useRealTimers()
  })

  it('answers a scene route before the shared routes', async () => {
    setActiveSceneExtras({ routes: [{ method: 'GET', match: /^\/api\/v1\/orchestration\/runs\/run_x\/approvals$/, body: { approvals: [{ id: 'apr_x' }] } }] })
    const response = await window.fetch('/api/v1/orchestration/runs/run_x/approvals')
    expect(await response.json()).toEqual({ approvals: [{ id: 'apr_x' }] })
  })

  it('uses fictional fixtures even when an API URL points at another origin', async () => {
    setActiveSceneExtras({ routes: [{ method: 'GET', match: /^\/api\/v1\/scene-test$/, body: { source: 'fictional' } }] })
    const response = await window.fetch('https://gateway.example/api/v1/scene-test')
    expect(await response.json()).toEqual({ source: 'fictional' })
  })

  it('blocks unrelated external fetches while scene mode is active', async () => {
    const response = await window.fetch('https://gateway.example/graphql')
    expect(response.status).toBe(503)
  })

  it('replays missed run frames at once and keeps later ones on the chat clock', async () => {
    setActiveSceneStream({ path: '/api/v1/chat/stream', frames: [{ at: 10, event: 'done', data: {} }], closeAfterMs: 10 })
    setActiveSceneExtras({
      runStreams: [{
        runId: 'run_x',
        closeAfterMs: 0,
        frames: [
          { at: 100, event: 'subagent_attached', data: {} },
          { at: 200, event: 'subagent_attached', data: {} },
          { at: 900, event: 'subagent_stopped', data: {} },
        ],
      }],
    })
    await window.fetch('/api/v1/chat/stream', { method: 'POST' })
    // The run panel subscribes 500 ms after the chat POST.
    await vi.advanceTimersByTimeAsync(500)
    const response = await window.fetch('/api/v1/runs/run_x/events')
    expect(response.headers.get('Content-Type')).toBe('text/event-stream')
    const body = readAll(response)
    await vi.advanceTimersByTimeAsync(0)
    const caughtUp = getSceneLog().filter((entry) => entry.kind === 'run-sse' && entry.note.includes('subagent_attached'))
    expect(caughtUp).toHaveLength(2)
    expect(getSceneLog().some((entry) => entry.note.includes('subagent_stopped'))).toBe(false)
    await vi.advanceTimersByTimeAsync(400)
    expect(eventNames(await body)).toEqual(['subagent_attached', 'subagent_attached', 'subagent_stopped'])
  })

  it('maps registered artifact images and blocks unknown API or external images', () => {
    setActiveSceneExtras({ images: { '/api/v1/browser/sessions/run_x/artifacts/art_x': '/src/demo/fixtures/assets/x.png' } })
    const image = document.createElement('img')
    image.setAttribute('src', '/api/v1/browser/sessions/run_x/artifacts/art_x')
    expect(image.getAttribute('src')).toBe('/src/demo/fixtures/assets/x.png')
    const other = document.createElement('img')
    other.src = '/api/v1/browser/sessions/run_x/artifacts/art_other'
    expect(other.src).toBe('data:,')
    const external = document.createElement('img')
    external.src = 'https://gateway.example/api/v1/private-image'
    expect(external.src).toBe('data:,')
  })

  it('blocks unknown API images before scene-specific fixture images are registered', () => {
    setActiveSceneExtras(null)
    const image = document.createElement('img')
    image.src = '/api/v1/private-image'
    expect(image.src).toBe('data:,')
  })

  it('restores browser request APIs when the scene exits', () => {
    const interceptedFetch = window.fetch
    uninstallSceneRuntime()
    expect(window.fetch).not.toBe(interceptedFetch)
    expect(window.__verevonSceneRequestCounts).toBeUndefined()
    const image = document.createElement('img')
    image.src = '/api/v1/private-image'
    expect(image.src).toBe(`${window.location.origin}/api/v1/private-image`)
    installSceneRuntime()
    sceneFetch = window.fetch
  })
})
