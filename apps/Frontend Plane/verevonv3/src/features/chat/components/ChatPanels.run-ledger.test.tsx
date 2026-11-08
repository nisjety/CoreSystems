// @vitest-environment jsdom

import { vi } from 'vitest'

// The run ledger switch is read once, when its module loads, so it is set before the
// panels under test are imported.
vi.hoisted(() => {
  vi.stubEnv('VITE_CHAT_RUN_LEDGER', 'true')
})

import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'
import { flush } from 'solid-js'
import { StepsPanel, TracePanel } from './ChatPanels'
import type { VerevonUiEvent } from '@/shared/chat/verevon-ui-events'

afterEach(() => {
  cleanup()
})

const events: VerevonUiEvent[] = [
  { type: 'run.connected', at: '2026-11-03T10:00:00.000Z', requestId: 'req-1', runId: 'run-1', turnId: 'turn-1' },
  { type: 'tool.call', at: '2026-11-03T10:00:01.000Z', id: 'c1', name: 'web_search', turnId: 'turn-1' },
  { type: 'tool.result', at: '2026-11-03T10:00:02.000Z', id: 'c1', status: 'completed', turnId: 'turn-1' },
]

describe('TracePanel with the run ledger on', () => {
  it('renders the ledger in place of the flat list, under the same header', () => {
    const { container, unmount } = render(() => <TracePanel events={events} runId={null} />)
    expect(container.querySelector('.verevon-chat-ledger')).not.toBeNull()
    expect(container.querySelector('.verevon-chat-trace-log > ol')).toBeNull()
    expect(container.querySelector('.verevon-chat-trace-log > header > span')?.textContent).toBe('3 hendelser')
    unmount()
  })

  it('selects the call an inspect request names when the Trace mounts', () => {
    const { container, unmount } = render(() => (
      <TracePanel events={events} runId={null} inspectRequest={{ callId: 'c1' }} />
    ))
    flush()
    expect(container.querySelector('.verevon-chat-ledger__inspector')?.textContent).toContain('web_search')
    unmount()
  })
})

describe('the Steps tab with the run ledger on', () => {
  it('gives each tool card an Inspect action that names its call', () => {
    const onInspectCall = vi.fn((callId: string) => callId)
    const { container, unmount } = render(() => (
      <StepsPanel
        steps={[{ id: 'search', title: 'Search the web', detail: '', status: 'done', createdAt: '2026-11-03T10:00:00Z' }]}
        toolCalls={[{ id: 'c1', name: 'web_search', status: 'completed', output: '3 results' }]}
        onStopTask={() => {}}
        onInspectCall={onInspectCall}
      />
    ))
    const inspect = container.querySelector<HTMLButtonElement>('.verevon-chat-tool-call__inspect')
    expect(inspect).not.toBeNull()
    if (inspect) fireEvent.click(inspect)
    expect(onInspectCall).toHaveBeenCalledWith('c1')
    unmount()
  })
})
