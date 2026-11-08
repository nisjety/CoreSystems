// @vitest-environment jsdom

/**
 * One MCP App in the frame (EP-4 S9). The frame names the server it comes from,
 * says when the app wanted network it does not get, draws nothing when there is
 * no app to read, and answers the app's JSON-RPC only from its own window.
 */

import { cleanup, fireEvent, render, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { McpAppResource } from '@/shared/api/chat-client'
import { MCP_APP_METHODS } from '../lib/mcp-app-bridge'
import { McpAppFrame } from './McpAppFrame'
import type { ChatToolCall } from './chat-types'

const APP: McpAppResource = { uri: 'ui://charts/table', html: '<title>Tabell</title><div id="app"></div>', needsNetwork: false }

const CALL: ChatToolCall = {
  id: 'call-1',
  name: 'mcp__srv-erp__show_table',
  args: { region: 'nord' },
  status: 'done',
  output: '3 rader',
}

/** Stands in for the frame's window; the host compares `event.source` with it by identity. */
function frameWindow() {
  return { postMessage: vi.fn() }
}

async function mount(load: () => Promise<McpAppResource | null>, onSelectFollowUp?: (text: string) => void) {
  const view = render(() => (
    <McpAppFrame serverId="srv-erp" tool="show_table" call={CALL} load={load} onSelectFollowUp={onSelectFollowUp} />
  ))
  await Promise.resolve()
  flush()
  return view
}

function post(source: unknown, data: unknown, origin = 'null') {
  const event = new MessageEvent('message', { data, origin })
  Object.defineProperty(event, 'source', { value: source })
  window.dispatchEvent(event)
  flush()
}

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('McpAppFrame', () => {
  it('label names the server', async () => {
    const { container } = await mount(async () => APP)
    await waitFor(() => expect(container.querySelector('iframe')).not.toBeNull())
    const frame = container.querySelector('iframe')!
    expect(container.textContent).toContain('App fra srv-erp')
    expect(frame.getAttribute('title')).toBe('App fra srv-erp')
    // The same isolation as an inline widget: scripts only, never same-origin.
    expect(frame.getAttribute('sandbox')).toBe('allow-scripts')
    expect(frame.getAttribute('referrerpolicy')).toBe('no-referrer')
    expect(frame.getAttribute('srcdoc')).toContain('Content-Security-Policy')
    expect(frame.getAttribute('srcdoc')).toContain(APP.html)
    expect(container.textContent).not.toContain('nettverkstilgang')
  })

  it('network notice shown', async () => {
    const { container } = await mount(async () => ({ ...APP, needsNetwork: true }))
    await waitFor(() => expect(container.querySelector('iframe')).not.toBeNull())
    expect(container.textContent).toContain('Denne appen trenger nettverkstilgang, som ikke er tilgjengelig her')
    // Reported, not honoured: the CSP still allows no connection.
    expect(container.querySelector('iframe')?.getAttribute('srcdoc')).toContain("connect-src 'none'")
  })

  it('fetch failure shows the tool card only', async () => {
    for (const load of [async () => null, () => Promise.reject(new Error('down'))]) {
      const { container, unmount } = await mount(load)
      await Promise.resolve()
      flush()
      expect(container.querySelector('iframe')).toBeNull()
      expect(container.textContent).not.toContain('App fra')
      unmount()
    }
  })

  it('answers the app in its own frame and stages its message', async () => {
    const onSelectFollowUp = vi.fn()
    const { container } = await mount(async () => APP, onSelectFollowUp)
    await waitFor(() => expect(container.querySelector('iframe')).not.toBeNull())
    const win = frameWindow()
    Object.defineProperty(container.querySelector('iframe')!, 'contentWindow', { configurable: true, value: win })

    post(win, { jsonrpc: '2.0', id: 1, method: MCP_APP_METHODS.initialize, params: {} })
    expect(win.postMessage).toHaveBeenCalledWith(expect.objectContaining({ id: 1, result: expect.any(Object) }), '*')

    // Another window's traffic is not this app's: nothing is answered.
    post(frameWindow(), { jsonrpc: '2.0', id: 2, method: 'ping' })
    expect(win.postMessage).toHaveBeenCalledTimes(1)

    post(win, { jsonrpc: '2.0', id: 3, method: MCP_APP_METHODS.message, params: { role: 'user', content: { type: 'text', text: 'Vis som tabell' } } })
    expect(onSelectFollowUp).not.toHaveBeenCalled()
    expect(container.textContent).toContain('Forslag fra appen:')
    fireEvent.click(container.querySelector('.verevon-chat-followup-chip')!)
    flush()
    expect(onSelectFollowUp).toHaveBeenCalledTimes(1)
    expect(onSelectFollowUp).toHaveBeenCalledWith('Vis som tabell')
  })
})
