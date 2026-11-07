// @vitest-environment jsdom

import { fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { RunEventHandlers } from '@/shared/api/run-console-client'
import type { ChatRunBrowserControl } from '@/features/chat/lib/chat-run-watch'

// EP-1 S1: Take over and Give back of the chat Do run's browser, from the live panel.

const hoisted = vi.hoisted(() => ({
  subscriptions: [] as Array<{ handlers: Record<string, unknown>; runId: string }>,
  changeBrowserControl: vi.fn(),
}))

vi.mock('@/shared/api/run-console-client', () => ({
  listRunEventReplay: async () => ({ events: [], nextEventId: null, truncated: false }),
  streamRunEvents: (runId: string, handlers: Record<string, unknown>) => {
    hoisted.subscriptions.push({ handlers, runId })
    return new Promise<void>(() => {})
  },
}))

vi.mock('@/shared/api/runs-client', () => ({
  getRun: () => new Promise(() => {}),
  watchRun: vi.fn().mockResolvedValue({ watching: true }),
  unwatchRun: vi.fn().mockResolvedValue(undefined),
  getRunWatchStatus: vi.fn().mockResolvedValue({ watching: false }),
}))

vi.mock('@/features/browser/dock/browser-control', () => ({
  STALE_CONTROL_COPY: {
    no: 'Styringen av nettleseren ble endret et annet sted. Oppdatert; prøv igjen.',
    en: 'Browser control changed elsewhere. It has been refreshed; try again.',
  },
  changeBrowserControl: hoisted.changeBrowserControl,
}))

import { ChatLiveRunPanel, GIVE_BACK_NOTE_MAX_CHARS } from './ChatLiveRunPanel'
import { __resetRunNotifyForTests } from './RunNotifyButton'

const TAKE_OVER = 'Ta over nettleserkontrollen manuelt'
const GIVE_BACK = 'Gi nettleserkontroll tilbake til AI-agenten'
const NOTE = 'Beskjed til agenten når du gir tilbake (valgfritt)'

/** The live frame stream is opened by BrowserChrome; jsdom has no EventSource. */
class FakeEventSource {
  addEventListener() {}
  close() {}
}

function handlers(): RunEventHandlers {
  return hoisted.subscriptions[hoisted.subscriptions.length - 1]?.handlers as RunEventHandlers
}

async function tick(times = 1): Promise<void> {
  for (let step = 0; step < times; step += 1) {
    await new Promise<void>((resolve) => setTimeout(resolve, 0))
  }
  flush()
}

async function subscribed(): Promise<void> {
  for (let attempt = 0; attempt < 20 && hoisted.subscriptions.length < 1; attempt += 1) {
    await tick()
  }
  expect(hoisted.subscriptions.length).toBeGreaterThanOrEqual(1)
}

/** One framed step, so the panel mounts BrowserChrome. */
function frame(): void {
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
}

function controlAnswer(mode: 'agent_control' | 'human_takeover', epoch: number, stale = false) {
  return { response: { session: { control: { mode, epoch } } }, stale }
}

async function mount(options: {
  binding?: ChatRunBrowserControl
  zdr?: boolean
  onBrowserGiveBack?: (runId: string) => void
}) {
  const view = render(() => (
    <ChatLiveRunPanel
      collapsed={false}
      onToggleCollapsed={() => undefined}
      orgId="org_1"
      runId="run_abc123"
      zdr={options.zdr ?? false}
      browserBinding={options.binding}
      onBrowserGiveBack={options.onBrowserGiveBack}
    />
  ))
  await subscribed()
  frame()
  return view
}

const bound: ChatRunBrowserControl = { sessionId: 'qrun_1', controlMode: 'agent_control', controlEpoch: 1 }

beforeEach(() => {
  hoisted.subscriptions.length = 0
  hoisted.changeBrowserControl.mockReset()
  __resetRunNotifyForTests()
  vi.stubGlobal('EventSource', FakeEventSource)
  vi.stubEnv('VITE_CHAT_BROWSER_TAKEOVER', 'true')
})

afterEach(() => {
  vi.unstubAllEnvs()
  vi.unstubAllGlobals()
})

describe('ChatLiveRunPanel take over and give back', () => {
  it('keeps the toggle disabled while the switch is off, even for a bound turn', async () => {
    vi.stubEnv('VITE_CHAT_BROWSER_TAKEOVER', 'false')
    await mount({ binding: bound })

    expect((screen.getByLabelText(TAKE_OVER) as HTMLButtonElement).disabled).toBe(true)
  })

  it('keeps the toggle disabled for a zero-retention turn', async () => {
    await mount({ binding: bound, zdr: true })

    expect((screen.getByLabelText(TAKE_OVER) as HTMLButtonElement).disabled).toBe(true)
  })

  it('space_member_sees_no_take_over: a turn without the owner-only binding has no control', async () => {
    await mount({})

    expect((screen.getByLabelText(TAKE_OVER) as HTMLButtonElement).disabled).toBe(true)
    expect(hoisted.changeBrowserControl).not.toHaveBeenCalled()
  })

  it('takes over with the epoch the person saw and shows what Quarry answers', async () => {
    hoisted.changeBrowserControl.mockResolvedValue(controlAnswer('human_takeover', 2))
    const givenBack = vi.fn()
    await mount({ binding: bound, onBrowserGiveBack: givenBack })

    fireEvent.click(screen.getByLabelText(TAKE_OVER))
    await tick(2)

    expect(hoisted.changeBrowserControl).toHaveBeenCalledWith('org_1', 'qrun_1', 'human_takeover', 1)
    expect(screen.getByLabelText(GIVE_BACK)).toBeTruthy()
    expect(givenBack).not.toHaveBeenCalled()
  })

  it('gives back with the current epoch, then asks the turn to spend the takeover approval', async () => {
    hoisted.changeBrowserControl.mockResolvedValue(controlAnswer('agent_control', 3))
    const givenBack = vi.fn()
    await mount({
      binding: { sessionId: 'qrun_1', controlMode: 'human_takeover', controlEpoch: 2 },
      onBrowserGiveBack: givenBack,
    })

    fireEvent.click(screen.getByLabelText(GIVE_BACK))
    await tick(2)

    expect(hoisted.changeBrowserControl).toHaveBeenCalledWith('org_1', 'qrun_1', 'agent_control', 2)
    expect(givenBack).toHaveBeenCalledWith('run_abc123', '')
    expect(screen.getByLabelText(TAKE_OVER)).toBeTruthy()
  })

  // EP-1 S3: the optional hand-back note.

  it('offers the note only while the person holds the browser', async () => {
    await mount({ binding: bound })

    expect(screen.queryByLabelText(NOTE)).toBeNull()
  })

  it('offers no note for a zero-retention turn or with the switch off', async () => {
    const held: ChatRunBrowserControl = { sessionId: 'qrun_1', controlMode: 'human_takeover', controlEpoch: 2 }
    const zdrView = await mount({ binding: held, zdr: true })
    expect(screen.queryByLabelText(NOTE)).toBeNull()
    zdrView.unmount()

    vi.stubEnv('VITE_CHAT_BROWSER_TAKEOVER', 'false')
    await mount({ binding: held })
    expect(screen.queryByLabelText(NOTE)).toBeNull()
  })

  it('hands the turn the trimmed, capped note with the give-back, and clears it', async () => {
    hoisted.changeBrowserControl.mockResolvedValue(controlAnswer('agent_control', 3))
    const givenBack = vi.fn()
    await mount({
      binding: { sessionId: 'qrun_1', controlMode: 'human_takeover', controlEpoch: 2 },
      onBrowserGiveBack: givenBack,
    })

    const note = screen.getByLabelText(NOTE) as HTMLTextAreaElement
    expect(note.maxLength).toBe(GIVE_BACK_NOTE_MAX_CHARS)
    fireEvent.input(note, { target: { value: `  ${'x'.repeat(GIVE_BACK_NOTE_MAX_CHARS + 20)}  ` } })
    flush()
    fireEvent.click(screen.getByLabelText(GIVE_BACK))
    await tick(2)

    expect(givenBack).toHaveBeenCalledWith('run_abc123', 'x'.repeat(GIVE_BACK_NOTE_MAX_CHARS))
    expect(screen.queryByLabelText(NOTE)).toBeNull()
  })

  it('keeps the note when the give-back met a stale epoch', async () => {
    hoisted.changeBrowserControl.mockResolvedValue(controlAnswer('human_takeover', 4, true))
    const givenBack = vi.fn()
    await mount({
      binding: { sessionId: 'qrun_1', controlMode: 'human_takeover', controlEpoch: 2 },
      onBrowserGiveBack: givenBack,
    })

    fireEvent.input(screen.getByLabelText(NOTE), { target: { value: 'Signed in.' } })
    flush()
    fireEvent.click(screen.getByLabelText(GIVE_BACK))
    await tick(2)

    expect(givenBack).not.toHaveBeenCalled()
    expect((screen.getByLabelText(NOTE) as HTMLTextAreaElement).value).toBe('Signed in.')
  })

  it('does not hand the turn a give-back that met a stale epoch', async () => {
    hoisted.changeBrowserControl.mockResolvedValue(controlAnswer('human_takeover', 4, true))
    const givenBack = vi.fn()
    await mount({
      binding: { sessionId: 'qrun_1', controlMode: 'human_takeover', controlEpoch: 2 },
      onBrowserGiveBack: givenBack,
    })

    fireEvent.click(screen.getByLabelText(GIVE_BACK))
    await tick(2)

    expect(givenBack).not.toHaveBeenCalled()
    expect(screen.getByRole('alert').textContent).toContain('Styringen av nettleseren ble endret et annet sted')
  })
})
