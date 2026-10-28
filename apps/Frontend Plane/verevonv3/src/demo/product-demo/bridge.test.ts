import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest'
import { announceReady, startBridge } from './bridge'

// The bridge is the demo's end of the iframe protocol (bridge.ts). jsdom's window is its own parent,
// so each case puts a stand-in for the embedding page in window.parent.

const SOURCE = 'verevon-demo'

type FakeParent = { postMessage: ReturnType<typeof vi.fn> }

let parent: FakeParent
let restoreParent: () => void
let stopBridge: () => void
let reload: Mock<() => void>

function setParent(value: unknown): void {
  Object.defineProperty(window, 'parent', { configurable: true, get: () => value })
}

/** A message from the host. The browser reports the sender as `source`, which jsdom does not set for us. */
function siteMessage(data: unknown, source: unknown = parent): void {
  const event = new MessageEvent('message', { data })
  Object.defineProperty(event, 'source', { value: source })
  window.dispatchEvent(event)
}

/** Everything the demo has posted to the embedding page. */
function posted(): unknown[] {
  return parent.postMessage.mock.calls.map((call) => call[0])
}

function demoState(): string | undefined {
  return document.documentElement.dataset.demoState
}

beforeEach(() => {
  const original = Object.getOwnPropertyDescriptor(window, 'parent')
  restoreParent = () => {
    if (original) Object.defineProperty(window, 'parent', original)
    else delete (window as unknown as { parent?: unknown }).parent
  }
  parent = { postMessage: vi.fn() }
  setParent(parent)
  reload = vi.fn<() => void>()
  document.documentElement.removeAttribute('data-demo-state')
  stopBridge = startBridge({ reload })
})

afterEach(() => {
  stopBridge()
  restoreParent()
})

describe('announceReady', () => {
  it('posts ready to the embedding page, with the protocol envelope', () => {
    announceReady()
    expect(parent.postMessage).toHaveBeenCalledWith({ source: SOURCE, v: 1, type: 'ready' }, '*')
  })

  it('posts nothing when the demo is not framed', () => {
    setParent(window)
    announceReady()
    expect(parent.postMessage).not.toHaveBeenCalled()
  })
})

describe('messages from the host', () => {
  it('start inert when framed and move through the states the host asks for', () => {
    expect(demoState()).toBe('inactive')
    expect(document.documentElement.inert).toBe(true)
    siteMessage({ source: SOURCE, v: 1, type: 'activate' })
    expect(demoState()).toBe('active')
    expect(document.documentElement.inert).toBe(false)
    siteMessage({ source: SOURCE, v: 1, type: 'pause' })
    expect(demoState()).toBe('paused')
    expect(document.documentElement.inert).toBe(true)
    siteMessage({ source: SOURCE, v: 1, type: 'resume' })
    expect(demoState()).toBe('active')
    siteMessage({ source: SOURCE, v: 1, type: 'deactivate' })
    expect(demoState()).toBe('inactive')
    expect(document.documentElement.inert).toBe(true)
  })

  it('keep a demo the visitor has not activated inert when the stage resumes or pauses it', () => {
    siteMessage({ source: SOURCE, v: 1, type: 'resume' })
    expect(demoState()).toBe('inactive')
    expect(document.documentElement.inert).toBe(true)
    siteMessage({ source: SOURCE, v: 1, type: 'pause' })
    expect(demoState()).toBe('inactive')
    siteMessage({ source: SOURCE, v: 1, type: 'activate' })
    siteMessage({ source: SOURCE, v: 1, type: 'deactivate' })
    siteMessage({ source: SOURCE, v: 1, type: 'resume' })
    expect(demoState()).toBe('inactive')
  })

  it('stay active when the demo is opened on its own, not framed', () => {
    stopBridge()
    setParent(window)
    stopBridge = startBridge({ reload })
    expect(demoState()).toBe('active')
    expect(document.documentElement.inert).toBe(false)
  })

  it('reset reloads the document', () => {
    siteMessage({ source: SOURCE, v: 1, type: 'reset' })
    expect(reload).toHaveBeenCalledTimes(1)
  })

  it('from any source other than the embedding page are ignored', () => {
    siteMessage({ source: SOURCE, v: 1, type: 'pause' }, {})
    siteMessage({ source: SOURCE, v: 1, type: 'reset' }, {})
    expect(demoState()).toBe('inactive')
    expect(reload).not.toHaveBeenCalled()
  })

  it('with another sender, another protocol version or no envelope are ignored', () => {
    siteMessage({ source: 'other-app', v: 1, type: 'pause' })
    siteMessage({ source: SOURCE, v: 2, type: 'pause' })
    siteMessage('pause')
    siteMessage(null)
    expect(demoState()).toBe('inactive')
  })
})

describe('messages to the host', () => {
  it('Escape in the demo is forwarded', () => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    expect(posted()).toEqual([{ source: SOURCE, v: 1, type: 'escape' }])
  })

  it('Escape that a component already used is not forwarded', () => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', cancelable: true }))
    expect(posted()).toHaveLength(1)
    parent.postMessage.mockClear()
    const used = new KeyboardEvent('keydown', { key: 'Escape', cancelable: true })
    used.preventDefault()
    window.dispatchEvent(used)
    expect(parent.postMessage).not.toHaveBeenCalled()
  })

  it('other keys stay in the demo', () => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }))
    expect(posted()).toEqual([])
  })

  it('an uncaught error is reported as its kind only, never with its details', () => {
    window.dispatchEvent(new ErrorEvent('error', { message: 'a detail of the demo page' }))
    window.dispatchEvent(new Event('unhandledrejection'))
    expect(posted()).toEqual([
      { source: SOURCE, v: 1, type: 'error', message: 'uncaught error' },
      { source: SOURCE, v: 1, type: 'error', message: 'unhandled rejection' },
    ])
  })
})

describe('stopping the bridge', () => {
  it('stops listening once the returned function has run', () => {
    stopBridge()
    siteMessage({ source: SOURCE, v: 1, type: 'pause' })
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    expect(demoState()).toBe('inactive')
    expect(posted()).toEqual([])
  })
})
