// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { AssistantLiveBrowser } from './AssistantLiveBrowser'

const receipt = { runId: 'model-1', planId: 'plan-1', attachId: 'attach-1', sessionRunId: 'session-1', tabId: 'tab-1' }
const sources: FakeSource[] = []
class FakeSource {
  url: string
  listeners = new Map<string, (event: MessageEvent<string>) => void>()
  close = vi.fn()
  constructor(url: string) { this.url = url; sources.push(this) }
  addEventListener(type: string, listener: (event: MessageEvent<string>) => void) { this.listeners.set(type, listener) }
  emit(type: string, data = '') { this.listeners.get(type)?.({ data } as MessageEvent<string>); flush() }
}
afterEach(() => { cleanup(); sources.length = 0; vi.unstubAllGlobals(); vi.useRealTimers() })

describe('AssistantLiveBrowser', () => {
  it('opens neither stream for a receipt belonging to a different model run', () => {
    vi.stubGlobal('EventSource', FakeSource)
    render(() => <AssistantLiveBrowser sessionId="session-1" modelRunId="other" receipt={receipt} />)
    expect(sources).toHaveLength(0)
  })

  it('keeps a bounded last frame and clears the cursor on disconnect', () => {
    vi.useFakeTimers()
    vi.stubGlobal('EventSource', FakeSource)
    render(() => <AssistantLiveBrowser sessionId="session-1" modelRunId="model-1" receipt={receipt} />)
    expect(sources[0]!.url).toBe('/api/v1/browser/sessions/session-1/frames/stream?tabId=tab-1')
    expect(sources).toHaveLength(1)
    sources[0]!.emit('frame', JSON.stringify({ mimeType: 'image/jpeg', dataBase64: 'AA==' }))
    expect(screen.getByAltText('Agentens nettleserside')).toBeTruthy()
    expect(screen.queryByLabelText('Verevons handlingsmål')).toBeNull()
    sources[0]!.emit('error')
    expect(screen.queryByLabelText('Verevons handlingsmål')).toBeNull()
    expect(screen.getByAltText('Agentens nettleserside')).toBeTruthy()
    vi.advanceTimersByTime(120_000); flush()
    expect(screen.queryByAltText('Agentens nettleserside')).toBeNull()
  })

  it('retains the last frame and a read-only cursor after a confirmed run ends', () => {
    vi.useFakeTimers()
    vi.stubGlobal('EventSource', FakeSource)
    const [finished, setFinished] = createSignal(false)
    render(() => <AssistantLiveBrowser sessionId="session-1" modelRunId="model-1" receipt={receipt} finished={finished()} />)
    sources[0]!.emit('frame', JSON.stringify({ mimeType: 'image/jpeg', dataBase64: 'AA==', agentCursor: { sessionRunId: 'session-1', attachId: 'attach-1', tabId: 'tab-1', x: 200, y: 300, viewportWidth: 800, viewportHeight: 600 } }))
    sources[0]!.emit('done')
    expect(screen.queryByLabelText('Verevons siste markørposisjon')).toBeNull()
    setFinished(true); flush()
    expect(screen.getByText('Siste visning · opptil 2 minutter')).toBeTruthy()
    expect(screen.getByAltText('Agentens nettleserside')).toBeTruthy()
    const cursor = screen.getByRole('img', { name: 'Verevons siste markørposisjon' }) as HTMLElement
    expect(cursor.style.left).toBe('25%')
    expect(cursor.style.top).toBe('50%')
    expect(screen.queryByRole('button', { name: 'Verevons markør' })).toBeNull()
    vi.advanceTimersByTime(120_000); flush()
    expect(screen.queryByLabelText('Verevons siste markørposisjon')).toBeNull()
  })

  it('renders only a cursor bound to the same actual frame and matching run and attachment', () => {
    vi.useFakeTimers()
    vi.stubGlobal('EventSource', FakeSource)
    render(() => <AssistantLiveBrowser sessionId="session-1" modelRunId="model-1" receipt={receipt} />)
    const frame = { mimeType: 'image/jpeg', dataBase64: 'AA==' }
    const cursor = { sessionRunId: 'session-1', attachId: 'attach-1', tabId: 'tab-1', x: 200, y: 300, viewportWidth: 800, viewportHeight: 600 }
    sources[0]!.emit('frame', JSON.stringify({ ...frame, agentCursor: { ...cursor, attachId: 'other' } }))
    expect(screen.queryByLabelText('Verevons markør')).toBeNull()
    sources[0]!.emit('frame', JSON.stringify({ ...frame, agentCursor: cursor }))
    const marker = screen.getByLabelText('Verevons markør') as HTMLElement
    expect(marker.style.left).toBe('25%')
    expect(marker.style.top).toBe('50%')
    sources[0]!.emit('frame', JSON.stringify(frame))
    expect(screen.getByLabelText('Verevons markør')).toBeTruthy()
    expect(screen.getByText('Verevon · sist sett')).toBeTruthy()
    sources[0]!.emit('frame', JSON.stringify({ ...frame, agentCursor: { ...cursor, tabId: 'other' } }))
    expect(screen.queryByLabelText('Verevons markør')).toBeNull()
  })

  it('does not render out-of-bounds or mismatched cursor telemetry', () => {
    vi.stubGlobal('EventSource', FakeSource)
    render(() => <AssistantLiveBrowser sessionId="session-1" modelRunId="model-1" receipt={receipt} />)
    const frame = { mimeType: 'image/jpeg', dataBase64: 'AA==' }
    const cursor = { modelRunId: 'model-1', sessionRunId: 'session-1', attachId: 'attach-1', tabId: 'tab-1', x: 800, y: 300, viewportWidth: 800, viewportHeight: 600 }
    sources[0]!.emit('frame', JSON.stringify({ ...frame, agentCursor: cursor }))
    expect(screen.queryByLabelText('Verevons markør')).toBeNull()
    sources[0]!.emit('frame', JSON.stringify({ ...frame, agentCursor: { ...cursor, x: 200, tabId: 'other' } }))
    expect(screen.queryByLabelText('Verevons markør')).toBeNull()
  })

  it('keeps the last observed cursor while frames arrive and clears it when disconnected', () => {
    vi.useFakeTimers()
    vi.stubGlobal('EventSource', FakeSource)
    render(() => <AssistantLiveBrowser sessionId="session-1" modelRunId="model-1" receipt={receipt} />)
    sources[0]!.emit('frame', JSON.stringify({ mimeType: 'image/jpeg', dataBase64: 'AA==', agentCursor: { sessionRunId: 'session-1', attachId: 'attach-1', tabId: 'tab-1', x: 200, y: 300, viewportWidth: 800, viewportHeight: 600 } }))
    expect(screen.getByLabelText('Verevons markør')).toBeTruthy()
    for (let i = 0; i < 4; i++) {
      vi.advanceTimersByTime(1500)
      sources[0]!.emit('frame', JSON.stringify({ mimeType: 'image/jpeg', dataBase64: 'AA==' }))
    }
    expect(screen.getByLabelText('Verevons markør')).toBeTruthy()
    expect(screen.getByText('Verevon · sist sett')).toBeTruthy()
    sources[0]!.emit('error')
    expect(screen.queryByLabelText('Verevons markør')).toBeNull()
    expect(screen.getByAltText('Agentens nettleserside')).toBeTruthy()
  })

  it('lets the user stop the actual run from the last observed cursor', () => {
    vi.useFakeTimers()
    vi.stubGlobal('EventSource', FakeSource)
    const stop = vi.fn()
    render(() => <AssistantLiveBrowser sessionId="session-1" modelRunId="model-1" receipt={receipt} onStop={stop} />)
    sources[0]!.emit('frame', JSON.stringify({ mimeType: 'image/jpeg', dataBase64: 'AA==', agentCursor: { sessionRunId: 'session-1', attachId: 'attach-1', tabId: 'tab-1', x: 200, y: 300, viewportWidth: 800, viewportHeight: 600 } }))
    fireEvent.click(screen.getByLabelText('Verevons markør'))
    flush()
    expect(screen.getByRole('group', { name: 'Agentkontroller' })).toBeTruthy()
    vi.advanceTimersByTime(2000); flush()
    expect(screen.getByLabelText('Verevons markør')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Stopp agenten' }))
    flush()
    expect(stop).toHaveBeenCalledOnce()
    expect(screen.queryByRole('group', { name: 'Agentkontroller' })).toBeNull()
  })
})
