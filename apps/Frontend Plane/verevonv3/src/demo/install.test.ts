/**
 * Scene runtime extras (scene 03): scene routes, run-event streams on the chat
 * POST's clock, and the fixture image mapping for `<img src>` evidence.
 */
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { getSceneLog, installSceneRuntime, setActiveSceneExtras, setActiveSceneStream, setLiveHook } from './install'
import type { DemoRouteContext } from './live/shared-types'

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

  it('maps a registered artifact image path to its fixture asset, and nothing else', () => {
    setActiveSceneExtras({ images: { '/api/v1/browser/sessions/run_x/artifacts/art_x': '/src/demo/fixtures/assets/x.png' } })
    const image = document.createElement('img')
    image.setAttribute('src', '/api/v1/browser/sessions/run_x/artifacts/art_x')
    expect(image.getAttribute('src')).toBe('/src/demo/fixtures/assets/x.png')
    const other = document.createElement('img')
    other.src = '/api/v1/browser/sessions/run_x/artifacts/art_other'
    expect(new URL(other.src).pathname).toBe('/api/v1/browser/sessions/run_x/artifacts/art_other')
  })
})

describe('live hook', () => {
  let sceneFetch: typeof window.fetch
  beforeAll(() => {
    installSceneRuntime()
    sceneFetch = window.fetch
  })
  beforeEach(() => {
    window.fetch = sceneFetch
    setActiveSceneStream(null)
  })
  afterEach(() => {
    setLiveHook(null)
  })

  it('answers from the live routes before the shared fixtures', async () => {
    setLiveHook({ routes: [{ method: 'GET', match: /^\/api\/v1\/chat\/threads$/, respond: () => ({ sessions: [] }) }] })
    const response = await window.fetch('/api/v1/chat/threads')
    expect(await response.json()).toEqual({ sessions: [] })
  })

  it('falls through to the shared fixtures when no live route claims the request', async () => {
    setLiveHook({ routes: [{ method: 'GET', match: /^\/api\/v1\/elsewhere$/, respond: () => ({}) }] })
    const response = await window.fetch('/api/v1/chat/threads')
    expect(await response.json()).toEqual({ threads: [] })
  })

  it('passes the parsed JSON body, the method and the headers to the live route', async () => {
    let seen: DemoRouteContext | undefined
    setLiveHook({
      routes: [{
        method: 'POST',
        match: /^\/api\/v1\/chat\/threads\/thr_x\/messages$/,
        respond: (context) => {
          seen = context
          return { ok: true }
        },
      }],
    })
    await window.fetch('/api/v1/chat/threads/thr_x/messages', {
      method: 'POST',
      headers: { 'X-Demo': 'yes' },
      body: JSON.stringify({ content: 'Hei' }),
    })
    expect(seen?.method).toBe('POST')
    expect(seen?.path).toBe('/api/v1/chat/threads/thr_x/messages')
    expect(seen?.body).toEqual({ content: 'Hei' })
    expect(seen?.headers.get('x-demo')).toBe('yes')
  })

  it('hands a body that is not a JSON object to the route as the raw text', async () => {
    let seen: unknown
    setLiveHook({
      routes: [{
        method: 'PUT',
        match: /^\/api\/v1\/demo\/raw$/,
        respond: (context) => {
          seen = context.body
          return {}
        },
      }],
    })
    await window.fetch('/api/v1/demo/raw', { method: 'PUT', body: 'plain text' })
    expect(seen).toBe('plain text')
  })

  it('returns a Response a live route builds exactly as it is', async () => {
    setLiveHook({
      routes: [{ method: 'GET', match: /^\/api\/v1\/demo\/teapot$/, respond: () => new Response('nei', { status: 418 }) }],
    })
    const response = await window.fetch('/api/v1/demo/teapot')
    expect(response.status).toBe(418)
  })

  it('turns a throwing live route into a Norwegian 500 and records it, so the page keeps running', async () => {
    setLiveHook({
      routes: [{
        method: 'GET',
        match: /^\/api\/v1\/demo\/broken$/,
        respond: () => {
          throw new Error('boom')
        },
      }],
    })
    const response = await window.fetch('/api/v1/demo/broken')
    expect(response.status).toBe(500)
    expect(await response.json()).toMatchObject({ error: { code: 'demo_error' } })
    expect(getSceneLog().some((entry) => entry.kind === 'live-error' && entry.note.includes('boom'))).toBe(true)
  })

  it('maps a live screenshot request to its fixture image, and no other path', () => {
    setLiveHook({
      routes: [],
      resolveImage: (pathname) => (pathname.endsWith('/art_live') ? '/src/demo/fixtures/assets/live.png' : undefined),
    })
    const image = document.createElement('img')
    image.setAttribute('src', '/api/v1/browser/sessions/run_live/artifacts/art_live')
    expect(image.getAttribute('src')).toBe('/src/demo/fixtures/assets/live.png')
    const other = document.createElement('img')
    other.setAttribute('src', '/api/v1/browser/sessions/run_live/artifacts/art_other')
    expect(other.getAttribute('src')).toBe('/api/v1/browser/sessions/run_live/artifacts/art_other')
  })

  it('is a no-op once cleared, and scene mode without a live hook answers as before', async () => {
    setLiveHook({ routes: [{ method: 'GET', match: /^\/api\/v1\/chat\/threads$/, respond: () => ({ sessions: [] }) }] })
    setLiveHook(null)
    const response = await window.fetch('/api/v1/chat/threads')
    expect(await response.json()).toEqual({ threads: [] })
    const stream = await window.fetch('/api/v1/chat/stream', { method: 'POST', body: '{}' })
    expect(stream.status).toBe(503)
  })
})
