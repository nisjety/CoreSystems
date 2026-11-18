import { cleanup, fireEvent, render, screen, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { SpaceBrowserRunReceipt } from '@/shared/api/spaces-client/browser-runs'

// The start answer, mocked as the data the fixed client returns. The host rules and the host limit are the real ones.
const browserRuns = vi.hoisted(() => ({ startSpaceBrowserRun: vi.fn() }))
vi.mock('@/shared/api/spaces-client/browser-runs', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client/browser-runs')>()),
  ...browserRuns,
}))

const { SpaceBrowserStartPanel } = await import('./SpaceBrowserStartPanel')

const SPACE = 'room-a'
const THREAD = 'thr_1'
const GOAL_LABEL = 'Hva skal agenten gjøre?'
const HOST_LABEL = 'Domener agenten kan besøke (1 til 8)'
const SUBMIT_LABEL = 'Start nettleserkjøring'

function mountPanel(options: { canStart?: boolean; disabledReason?: string } = {}) {
  const onStarted = vi.fn()
  const onCancel = vi.fn()
  render(() => (
    <SpaceBrowserStartPanel
      spaceRef={SPACE}
      threadId={THREAD}
      canStart={options.canStart ?? true}
      disabledReason={options.disabledReason}
      onStarted={onStarted}
      onCancel={onCancel}
    />
  ))
  flush()
  return { onStarted, onCancel }
}

/** Types into a field and applies the change, so the next read sees it. */
function typeInto(field: HTMLElement, value: string) {
  fireEvent.input(field, { target: { value } })
  flush()
}

/** Adds one host the way a person does: type it, then press Enter. */
function addHost(host: string) {
  const field = screen.getByLabelText(HOST_LABEL)
  typeInto(field, host)
  fireEvent.keyDown(field, { key: 'Enter' })
  flush()
}

function submitButton(): HTMLButtonElement {
  return screen.getByRole('button', { name: SUBMIT_LABEL }) as HTMLButtonElement
}

/** Lets the answer's promise settle, then applies the signal writes it made. */
async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

beforeEach(() => {
  browserRuns.startSpaceBrowserRun.mockReset()
})

afterEach(() => {
  cleanup()
})

describe('SpaceBrowserStartPanel (WS20-S5)', () => {
  it('a viewer sees the disabled reason and no submit', () => {
    const { onStarted } = mountPanel({ canStart: false, disabledReason: 'Rommet er arkivert.' })

    expect(screen.getByRole('status').textContent).toBe('Rommet er arkivert.')
    expect((screen.getByLabelText(GOAL_LABEL) as HTMLTextAreaElement).disabled).toBe(true)
    expect((screen.getByLabelText(HOST_LABEL) as HTMLInputElement).disabled).toBe(true)
    expect(submitButton().disabled).toBe(true)

    // Even a submit that reaches the form is refused: nothing is sent and nothing reports a start.
    fireEvent.submit(submitButton().closest('form') as HTMLFormElement)
    flush()
    expect(browserRuns.startSpaceBrowserRun).not.toHaveBeenCalled()
    expect(onStarted).not.toHaveBeenCalled()
  })

  it('an IP literal shows the hint and is not added', () => {
    mountPanel()
    addHost('192.0.2.10')

    expect(screen.getByText('Bruk et domenenavn, ikke en IP-adresse.')).toBeTruthy()
    expect(within(screen.getByRole('list', { name: 'Valgte domener' })).queryAllByRole('listitem')).toHaveLength(0)
    expect(submitButton().disabled).toBe(true)
  })

  it('the ninth host is refused', () => {
    mountPanel()
    const words = ['one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight']
    for (const word of words) addHost(`${word}.example.test`)
    const chosen = screen.getByRole('list', { name: 'Valgte domener' })
    expect(within(chosen).getAllByRole('listitem')).toHaveLength(8)

    addHost('nine.example.test')
    expect(screen.getByRole('alert').textContent).toBe('Du kan velge høyst åtte domener.')
    expect(within(chosen).getAllByRole('listitem')).toHaveLength(8)
    expect(screen.queryByRole('button', { name: 'Fjern nine.example.test' })).toBeNull()
    expect(browserRuns.startSpaceBrowserRun).not.toHaveBeenCalled()
  })

  it('retries reuse one clientSendId', async () => {
    const receipt: SpaceBrowserRunReceipt = { runId: 'run_1', threadId: THREAD, planId: 'plan_1', viewScope: 'owner' }
    browserRuns.startSpaceBrowserRun
      .mockRejectedValueOnce(new TypeError('Failed to fetch'))
      .mockResolvedValueOnce(receipt)
    const { onStarted } = mountPanel()
    typeInto(screen.getByLabelText(GOAL_LABEL), 'Find the price of the lamp')
    addHost('shop.example.test')

    // The first answer is lost: the panel says so, and the person sends again.
    fireEvent.click(submitButton())
    flush()
    await settle()
    expect(screen.getByRole('alert')).toBeTruthy()
    expect(onStarted).not.toHaveBeenCalled()

    fireEvent.click(submitButton())
    flush()
    await settle()
    expect(browserRuns.startSpaceBrowserRun).toHaveBeenCalledTimes(2)
    const [first, second] = browserRuns.startSpaceBrowserRun.mock.calls.map((call) => call[1])
    expect(first.clientSendId).toMatch(/^[A-Za-z0-9_-]{16,64}$/)
    expect(second.clientSendId).toBe(first.clientSendId)
    expect(second).toMatchObject({
      threadId: THREAD,
      goal: 'Find the price of the lamp',
      allowedDomains: ['shop.example.test'],
      maxRuntimeSeconds: 300,
    })
    // The start answer hands the run to the room, and the earlier failure is cleared.
    expect(onStarted).toHaveBeenCalledTimes(1)
    expect(onStarted).toHaveBeenCalledWith(receipt)
    expect(screen.queryByRole('alert')).toBeNull()
  })
})
