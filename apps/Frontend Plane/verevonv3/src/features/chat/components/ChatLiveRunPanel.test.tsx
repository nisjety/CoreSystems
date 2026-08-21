// @vitest-environment jsdom

import { fireEvent, render, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { beforeEach, describe, expect, it } from 'vitest'
import { vi } from 'vitest'
import type { RunEventHandlers } from '@/shared/api/run-console-client'

const hoisted = vi.hoisted(() => ({
  subscriptions: [] as Array<{ handlers: Record<string, unknown>; runId: string }>,
}))

vi.mock('@/shared/api/run-console-client', () => ({
  streamRunEvents: (runId: string, handlers: Record<string, unknown>) => {
    hoisted.subscriptions.push({ handlers, runId })
    // Never settles: the real stream stays open for the life of the run.
    return new Promise<void>(() => {})
  },
}))

import { ChatLiveRunPanel } from './ChatLiveRunPanel'

const SHOT_URL = '/api/v1/browser/sessions/run_abc123/artifacts/art_shot_1'

function handlers(index = hoisted.subscriptions.length - 1): RunEventHandlers {
  return hoisted.subscriptions[index]?.handlers as RunEventHandlers
}

function railImage(container: HTMLElement): HTMLImageElement | null {
  return container.querySelector('.verevon-chat-run-shot__frame img')
}

beforeEach(() => {
  hoisted.subscriptions.length = 0
})

describe('ChatLiveRunPanel', () => {
  it('stays closed — and never subscribes — while the turn has no run id', () => {
    const { container } = render(() => (
      <ChatLiveRunPanel collapsed={false} onToggleCollapsed={() => undefined} runId={null} zdr={false} />
    ))

    expect(container.querySelector('.verevon-chat-run-panel')).toBeNull()
    expect(hoisted.subscriptions).toHaveLength(0)
  })

  it('opens and attaches to the run event stream once a run id arrives', () => {
    const [runId, setRunId] = createSignal<string | null>(null)
    const { container } = render(() => (
      <ChatLiveRunPanel collapsed={false} onToggleCollapsed={() => undefined} runId={runId()} zdr={false} />
    ))

    expect(container.querySelector('.verevon-chat-run-panel')).toBeNull()

    setRunId('run_abc123')
    flush()

    expect(container.querySelector('.verevon-chat-run-panel')).not.toBeNull()
    expect(hoisted.subscriptions).toHaveLength(1)
    expect(hoisted.subscriptions[0]?.runId).toBe('run_abc123')
    // Nothing has happened on the run yet — an honest "waiting", not a frame.
    expect(screen.getByText('Venter på agenten')).toBeTruthy()
  })

  it('collapses to its rail and restores the body', () => {
    const [collapsed, setCollapsed] = createSignal(false)
    const toggles: boolean[] = []
    const { container } = render(() => (
      <ChatLiveRunPanel
        collapsed={collapsed()}
        onToggleCollapsed={() => setCollapsed((current) => {
          toggles.push(!current)
          return !current
        })}
        runId="run_abc123"
        zdr={false}
      />
    ))

    expect(container.querySelector('.verevon-chat-run-panel__body')).not.toBeNull()

    fireEvent.click(screen.getByLabelText('Skjul live-panelet'))
    flush()

    expect(toggles).toEqual([true])
    expect(container.querySelector('.verevon-chat-run-panel--collapsed')).not.toBeNull()
    expect(container.querySelector('.verevon-chat-run-panel__body')).toBeNull()

    fireEvent.click(screen.getByLabelText('Vis live-panelet'))
    flush()

    expect(toggles).toEqual([true, false])
    expect(container.querySelector('.verevon-chat-run-panel--collapsed')).toBeNull()
    expect(container.querySelector('.verevon-chat-run-panel__body')).not.toBeNull()
    // Collapsing must not drop the subscription and restart the run stream.
    expect(hoisted.subscriptions).toHaveLength(1)
  })

  it('renders a step screenshot from its artifact reference and expands it on click', () => {
    const { container } = render(() => (
      <ChatLiveRunPanel collapsed={false} onToggleCollapsed={() => undefined} runId="run_abc123" zdr={false} />
    ))

    handlers().onBrowserAction?.({ actionId: 'act_0001', actionType: 'goto', url: 'https://example.com' })
    flush()
    handlers().onBrowserObservation?.({
      actionId: 'act_0001',
      pageTitle: 'Example Domain',
      pageUrl: 'https://example.com/',
      screenshotRef: 'art_shot_1',
      status: 'success',
    })
    flush()

    const thumbnail = railImage(container)
    expect(thumbnail?.getAttribute('src')).toBe(SHOT_URL)

    fireEvent.click(container.querySelector('.verevon-chat-run-shot') as HTMLElement)
    flush()

    const expandedImage = container.querySelector('.verevon-chat-run-shot-expanded img')
    expect(expandedImage?.getAttribute('src')).toBe(SHOT_URL)
    expect(screen.getByText('https://example.com/')).toBeTruthy()
  })

  it('reuses BrowserChrome for the live frame instead of a second browser panel', () => {
    const { container } = render(() => (
      <ChatLiveRunPanel collapsed={false} onToggleCollapsed={() => undefined} runId="run_abc123" zdr={false} />
    ))

    handlers().onBrowserObservation?.({
      actionId: 'act_0001',
      pageTitle: 'Example Domain',
      pageUrl: 'https://example.com/',
      screenshotRef: 'art_shot_1',
      status: 'success',
    })
    flush()

    const chrome = container.querySelector('.knowledge-browser-chrome')
    expect(chrome).not.toBeNull()
    expect(chrome?.classList.contains('knowledge-browser-chrome--compact')).toBe(true)
  })

  it('degrades to an honest note — never a broken image — when a screenshot fails to load', () => {
    const { container } = render(() => (
      <ChatLiveRunPanel collapsed={false} onToggleCollapsed={() => undefined} runId="run_abc123" zdr={false} />
    ))

    handlers().onBrowserObservation?.({
      actionId: 'act_0001',
      pageTitle: 'Example Domain',
      pageUrl: 'https://example.com/',
      screenshotRef: 'art_shot_1',
      status: 'success',
    })
    flush()

    const thumbnail = railImage(container)
    expect(thumbnail).not.toBeNull()

    fireEvent.error(thumbnail as HTMLImageElement)
    flush()

    expect(railImage(container)).toBeNull()
    expect(container.querySelector('.knowledge-browser-chrome')).toBeNull()
    expect(screen.getByText('Skjermbildet kunne ikke hentes.')).toBeTruthy()
    // The metadata the observation DID carry is still shown.
    expect(screen.getByText('Example Domain')).toBeTruthy()
  })

  it('shows metadata only, with a ZDR note, for a temporary chat turn', () => {
    const { container } = render(() => (
      <ChatLiveRunPanel collapsed={false} onToggleCollapsed={() => undefined} runId="run_abc123" zdr />
    ))

    // A ZDR run's capture is gated server-side: the observation arrives with no
    // screenshot reference at all.
    handlers().onBrowserObservation?.({
      actionId: 'act_0001',
      pageTitle: 'Intern rapport',
      pageUrl: 'https://example.com/rapport',
      status: 'success',
    })
    flush()

    expect(railImage(container)).toBeNull()
    expect(screen.getByText('Midlertidig samtale – skjermbilder lagres ikke.')).toBeTruthy()
    expect(screen.getByText('Intern rapport')).toBeTruthy()
    expect(screen.getByText('https://example.com/rapport')).toBeTruthy()
  })

  it('resolves the waiting state when the run ends without touching the browser', () => {
    render(() => (
      <ChatLiveRunPanel collapsed={false} onToggleCollapsed={() => undefined} runId="run_abc123" zdr={false} />
    ))

    expect(screen.getByText('Venter på agenten')).toBeTruthy()

    handlers().onDone?.()
    flush()

    expect(screen.queryByText('Venter på agenten')).toBeNull()
    expect(screen.getByText('Ingen nettleseraktivitet')).toBeTruthy()
  })

  it('surfaces a run-stream failure instead of waiting forever', () => {
    render(() => (
      <ChatLiveRunPanel collapsed={false} onToggleCollapsed={() => undefined} runId="run_abc123" zdr={false} />
    ))

    handlers().onError?.(new Error('boom'))
    flush()

    expect(screen.getByText('Kunne ikke lese hendelsesstrømmen for denne kjøringen.')).toBeTruthy()
  })

  it('re-subscribes to the new run and drops the previous run\'s steps', () => {
    const [runId, setRunId] = createSignal<string | null>('run_abc123')
    const { container } = render(() => (
      <ChatLiveRunPanel collapsed={false} onToggleCollapsed={() => undefined} runId={runId()} zdr={false} />
    ))

    handlers().onBrowserObservation?.({
      actionId: 'act_0001',
      pageUrl: 'https://example.com/',
      screenshotRef: 'art_shot_1',
      status: 'success',
    })
    flush()
    expect(railImage(container)).not.toBeNull()

    const stale = handlers()
    setRunId('run_def456')
    flush()

    expect(hoisted.subscriptions).toHaveLength(2)
    expect(container.querySelectorAll('.verevon-chat-run-shot')).toHaveLength(0)

    // A late callback from the aborted stream must not write into the new run.
    stale.onBrowserObservation?.({ actionId: 'act_0009', screenshotRef: 'art_shot_9', status: 'success' })
    flush()
    expect(container.querySelectorAll('.verevon-chat-run-shot')).toHaveLength(0)
  })
})
