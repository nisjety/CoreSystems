import { describe, expect, it, vi } from 'vitest'
import type { DesktopBrowserBridge, DesktopBrowserSnapshot } from '@/features/browser/desktop/desktop-browser'
import { createAssistantDesktopAgent } from './assistant-desktop-agent'

const url = 'https://example.com/'
const target = 't1234567890abcdef'
const snapshot: DesktopBrowserSnapshot = { url, title: 'Example', heading: 'Example', text: 'Example', targets: [{ id: target, role: 'link', label: 'More information' }], canGoBack: false, canGoForward: false }
type Suggest = NonNullable<Parameters<typeof createAssistantDesktopAgent>[0]['suggest']>

function setup(suggest: Suggest) {
  const getState = vi.fn(async () => ({ url: '', title: '', isLoading: false, canGoBack: false, canGoForward: false }))
  const bridge = {
    getState,
    setViewport: async () => {},
    navigate: async () => snapshot,
    command: getState,
    startRun: vi.fn(async () => 'lease'),
    execute: vi.fn<DesktopBrowserBridge['execute']>(async () => snapshot),
    endRun: vi.fn(async () => {}),
    reset: async () => {},
    subscribe: () => () => {},
  } satisfies DesktopBrowserBridge
  const onChange = vi.fn()
  const agent = createAssistantDesktopAgent({ bridge, scope: () => 'org/user', onChange, tr: (no) => no, suggest })
  return { agent, bridge, onChange }
}

describe('local assistant browser task', () => {
  it('uses the same native lease for each model proposal and hands back the tab before publishing the answer', async () => {
    const suggest = vi.fn<Suggest>()
      .mockResolvedValueOnce({ done: false, action: { kind: 'click', target } })
      .mockResolvedValueOnce({ done: true, action: null, outcome: 'completed', answer: 'Opened the link.' })
    const { agent, bridge, onChange } = setup(suggest)
    await agent.send('Open the link', url)
    await vi.waitFor(() => expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ status: 'completed', result: 'Opened the link.' })))
    expect(bridge.execute.mock.calls).toEqual([['lease', { kind: 'navigate', url }], ['lease', { kind: 'click', target }]])
    expect(bridge.endRun).toHaveBeenCalledExactlyOnceWith('lease')
    expect(bridge.endRun.mock.invocationCallOrder[0]).toBeLessThan(onChange.mock.invocationCallOrder.at(-1)!)
  })

  it('stops the native lease and ignores a model step that arrives after takeover', async () => {
    let resolve!: (result: Awaited<ReturnType<Suggest>>) => void
    const response = new Promise<Awaited<ReturnType<Suggest>>>(callback => { resolve = callback })
    const suggest = vi.fn(() => response)
    const { agent, bridge, onChange } = setup(suggest)
    await agent.send('Open the link', url)
    await vi.waitFor(() => expect(suggest).toHaveBeenCalled())
    agent.stop()
    await vi.waitFor(() => expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ status: 'stopped' })))
    resolve({ done: false, action: { kind: 'click', target } })
    await response
    expect(bridge.execute).toHaveBeenCalledExactlyOnceWith('lease', { kind: 'navigate', url })
    expect(bridge.endRun).toHaveBeenCalledExactlyOnceWith('lease')
  })
})
