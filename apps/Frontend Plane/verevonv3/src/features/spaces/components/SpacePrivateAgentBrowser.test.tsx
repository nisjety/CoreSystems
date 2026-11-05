// @vitest-environment jsdom
import { cleanup, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { SpacePrivateAgentBrowser } from './SpacePrivateAgentBrowser'

const opened: FakeEventSource[] = []
class FakeEventSource {
  listeners = new Map<string, (event: MessageEvent<string>) => void>()
  closed = false
  url: string
  constructor(url: string) { this.url = url; opened.push(this) }
  addEventListener(type: string, cb: (event: MessageEvent<string>) => void) { this.listeners.set(type, cb) }
  close() { this.closed = true }
  emit(type: string, data = '') { this.listeners.get(type)?.({ data } as MessageEvent<string>); flush() }
}
const receipt = { runId: 'model-1', planId: 'plan-1', attachId: 'attach-1', sessionRunId: 'browser-1', tabId: 'tab-2' }

afterEach(() => { cleanup(); opened.length = 0; vi.unstubAllGlobals() })

describe('SpacePrivateAgentBrowser', () => {
  it('will not open a stream for a receipt belonging to another run or session', () => {
    vi.stubGlobal('EventSource', FakeEventSource)
    render(() => <SpacePrivateAgentBrowser sessionId="another-browser" modelRunId="model-1" receipt={receipt} />)
    expect(opened).toHaveLength(0)
    expect(screen.getByRole('status').textContent).toContain('bekreftet')
  })

  it('pins the matching tab and clears its frame when disconnected', () => {
    vi.stubGlobal('EventSource', FakeEventSource)
    const view = render(() => <SpacePrivateAgentBrowser sessionId="browser-1" modelRunId="model-1" receipt={receipt} />)
    expect(opened).toHaveLength(1)
    expect(opened[0]!.url).toBe('/api/v1/browser/sessions/browser-1/frames/stream?tabId=tab-2')
    opened[0]!.emit('frame', JSON.stringify({ mimeType: 'image/jpeg', dataBase64: 'AA==' }))
    expect(screen.getByRole('img').getAttribute('src')).toBe('data:image/jpeg;base64,AA==')
    opened[0]!.emit('error')
    expect(screen.queryByRole('img')).toBeNull()
    view.unmount()
    expect(opened[0]!.closed).toBe(true)
  })
})
