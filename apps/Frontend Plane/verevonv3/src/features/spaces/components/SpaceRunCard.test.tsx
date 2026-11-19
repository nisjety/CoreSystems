import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/api-error'
import type { RoomRunStepClass, RoomRunStepState, SpaceRunWork } from '@/shared/api/spaces-client/run-work'
import { emptySpaceRunView, type SpaceRunView } from '../lib/space-run-view'

// WS09-S5: the live card for somebody else's run (its steps and headline, the streamed text or its progress, the
// collapsed summary after the run) and WS09-S8: the manager's Stop on that card. The stop call is stubbed at the
// module boundary. No I18nProvider wraps these renders, so the copy is the Norwegian default.

const stop = vi.hoisted(() => ({ stopSpaceRun: vi.fn() }))
vi.mock('@/shared/api/spaces-client/run-stop', () => stop)

const { SpaceRunCard, SpaceRunWorkSummary } = await import('./SpaceRunCard')

function view(overrides: Partial<SpaceRunView> = {}): SpaceRunView {
  return {
    ...emptySpaceRunView('run-1'),
    status: 'live',
    threadId: 't1',
    startedBy: 'editor-1',
    agentName: 'Lagerhjelpen',
    ...overrides,
  }
}

function step(id: string, cls: RoomRunStepClass, state: RoomRunStepState) {
  return { id, class: cls, state }
}

function renderCard(
  runView: SpaceRunView,
  options: { readonly startedByName?: string; readonly stop?: { readonly spaceRef: string; readonly runId: string } } = {},
) {
  return render(() => (
    <SpaceRunCard view={runView} startedByName={options.startedByName ?? 'Ola Hansen'} stop={options.stop} />
  ))
}

const stopButton = (container: HTMLElement) => container.querySelector('.verevon-run-card__stop-button') as HTMLButtonElement | null
const target = { spaceRef: 'room-1', runId: 'run-1' }

beforeEach(() => {
  stop.stopSpaceRun.mockReset()
})

afterEach(() => cleanup())

describe('SpaceRunCard: steps and headline (WS09-S5)', () => {
  it('names the newest running step in the headline, which is the one polite live region', () => {
    const { container } = renderCard(view({ steps: [step('s1', 'web', 'ok'), step('s2', 'code', 'running')] }))
    const headline = container.querySelector('.verevon-run-card__headline') as HTMLElement
    expect(headline.textContent).toBe('Kjører kode')
    expect(container.querySelectorAll('[aria-live="polite"]')).toHaveLength(1)
    expect(container.querySelector('[role="group"]')?.getAttribute('aria-label')).toBe('Agentens arbeid')
  })

  it('reads each step as its kind and its state in words, and lists only the newest three', () => {
    const { container } = renderCard(
      view({
        steps: [step('s1', 'web', 'ok'), step('s2', 'code', 'error'), step('s3', 'lookup', 'denied'), step('s4', 'memory', 'running')],
      }),
    )
    const lines = [...container.querySelectorAll('.verevon-run-card__steps .verevon-run-card__step')]
    expect(lines.map((line) => line.textContent)).toEqual(['Kjører kode, feilet', 'Slår opp, avslått', 'Bruker minnet, pågår'])
    expect(lines.map((line) => line.getAttribute('data-state'))).toEqual(['error', 'denied', 'running'])
    expect(container.querySelector('.verevon-run-card__steps')?.getAttribute('aria-label')).toBe('Siste steg')
    expect(container.querySelector('.verevon-run-card__more')?.textContent).toBe('1 steg til')
  })

  it('shows every step on the toggle and folds them back, with aria-expanded following', async () => {
    const steps = Array.from({ length: 5 }, (_, index) => step(`s${index}`, 'web', 'ok'))
    const { container } = renderCard(view({ steps }))
    const toggle = () => container.querySelector('.verevon-run-card__more') as HTMLButtonElement
    expect(toggle().textContent).toBe('2 steg til')
    expect(toggle().getAttribute('aria-expanded')).toBe('false')
    toggle().click()
    await waitFor(() => expect(container.querySelectorAll('.verevon-run-card__steps .verevon-run-card__step')).toHaveLength(5))
    expect(toggle().textContent).toBe('Vis færre')
    expect(toggle().getAttribute('aria-expanded')).toBe('true')
    toggle().click()
    await waitFor(() => expect(container.querySelectorAll('.verevon-run-card__steps .verevon-run-card__step')).toHaveLength(3))
    expect(toggle().getAttribute('aria-expanded')).toBe('false')
  })

  it('writes the streamed text as a live-off paragraph, and marks a text that was cut short', () => {
    const { container } = renderCard(
      view({ text: { length: 11, value: 'Hei, verden', truncated: true, visible: true, progressChars: 11 } }),
    )
    const text = container.querySelector('.verevon-run-card__text') as HTMLElement
    expect(text.getAttribute('aria-live')).toBe('off')
    expect(text.textContent).toBe('Hei, verden…')
    expect(container.querySelector('.verevon-run-card__progress')).toBeNull()
  })

  it('says how many characters are written when the text is not shown to members', () => {
    const { container } = renderCard(view({ text: { length: 0, value: '', truncated: false, visible: false, progressChars: 120 } }))
    expect(container.querySelector('.verevon-run-card__progress')?.textContent).toBe('Skriver svaret … 120 tegn')
    expect(container.querySelector('.verevon-run-card__text')).toBeNull()
  })

  it('says nothing about the answer before any character has been written', () => {
    const { container } = renderCard(view({ text: { length: 0, value: '', truncated: false, visible: false, progressChars: 0 } }))
    expect(container.querySelector('.verevon-run-card__progress')).toBeNull()
    expect(container.querySelector('.verevon-run-card__text')).toBeNull()
  })

  it('names who started the run and how many steers the owner sent', () => {
    const { container } = renderCard(view({ steer: 2 }), { startedByName: 'Kari Nordmann' })
    const head = container.querySelector('.verevon-run-card__head')?.textContent ?? ''
    expect(head).toContain('Startet av Kari Nordmann')
    expect(head).toContain('2 styringer fra eieren')
  })

  it('shows the time since the start the server gave', () => {
    const { container } = renderCard(view({ startedAt: Math.floor(Date.now() / 1000) - 65 }))
    expect(container.querySelector('.verevon-run-card__head')?.textContent).toMatch(/1 min \d+ s/)
  })
})

describe('SpaceRunCard: after the run (WS09-S5)', () => {
  it('collapses to a disclosure that says how many tools were used, with the steps inside it', () => {
    const { container } = renderCard(
      view({
        steps: [step('s1', 'web', 'ok'), step('s2', 'code', 'ok'), step('s3', 'lookup', 'error')],
        finished: { outcome: 'completed', reason: null },
      }),
    )
    expect(container.querySelector('.verevon-run-card')?.getAttribute('data-ended')).toBe('true')
    expect(container.querySelector('details.verevon-run-work > summary')?.textContent).toBe('Brukte 3 verktøy')
    expect([...container.querySelectorAll('.verevon-run-work__step')].map((line) => line.textContent)).toEqual([
      'Søker på nettet, ferdig',
      'Kjører kode, ferdig',
      'Slår opp, feilet',
    ])
    expect(container.querySelector('.verevon-run-card__headline')).toBeNull()
  })

  it('says a run that used no tools used none', () => {
    const { container } = renderCard(view({ steps: [], finished: { outcome: 'failed', reason: 'model_unavailable' } }))
    expect(container.querySelector('details.verevon-run-work > summary')?.textContent).toBe('Brukte ingen verktøy')
  })
})

describe('SpaceRunCard: Stop (WS09-S8)', () => {
  const running = () => view({ steps: [step('s1', 'web', 'running')] })

  it('offers no Stop unless the room has said the reader may stop the run', () => {
    const { container } = renderCard(running())
    expect(stopButton(container)).toBeNull()
  })

  it('sends one stop with one key, says it was sent, and holds the control until the run ends', async () => {
    stop.stopSpaceRun.mockResolvedValue({ runId: 'run-1', status: 'cancelled', cancelled: true, receiptId: 'r-1' })
    const { container } = renderCard(running(), { stop: target })
    const button = stopButton(container) as HTMLButtonElement
    expect(button.textContent).toBe('Stopp kjøringen')
    button.click()
    await waitFor(() => expect(stop.stopSpaceRun).toHaveBeenCalledTimes(1))
    const [spaceRef, runId, key] = stop.stopSpaceRun.mock.calls[0] ?? []
    expect([spaceRef, runId]).toEqual(['room-1', 'run-1'])
    expect(String(key)).toMatch(/^stop-/)
    expect(await screen.findByText('Stopp sendt …')).toBeTruthy()
    expect(stopButton(container)?.disabled).toBe(true)
    stopButton(container)?.click()
    expect(stop.stopSpaceRun).toHaveBeenCalledTimes(1)
  })

  it('says the organisation has not allowed stopping other members runs when the server refuses for that reason', async () => {
    stop.stopSpaceRun.mockRejectedValue(new ApiError('forbidden', 403, 'space_run_control_not_authorized'))
    const { container } = renderCard(running(), { stop: target })
    ;(stopButton(container) as HTMLButtonElement).click()
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBe("Organisasjonen din har ikke slått på stopping av andre medlemmers kjøringer.")
    expect(screen.queryByText('Stopp sendt …')).toBeNull()
  })

  it('says a failed stop could not be sent, and lets the reader try again with the same key', async () => {
    stop.stopSpaceRun
      .mockRejectedValueOnce(new ApiError('unavailable', 503, 'run_control_unavailable'))
      .mockResolvedValueOnce({ runId: 'run-1', status: 'cancelled', cancelled: true, receiptId: 'r-2' })
    const { container } = renderCard(running(), { stop: target })
    ;(stopButton(container) as HTMLButtonElement).click()
    expect((await screen.findByRole('alert')).textContent).toBe('Kunne ikke stoppe kjøringen. Prøv igjen.')
    expect(stopButton(container)?.disabled).toBe(false)
    stopButton(container)?.click()
    expect(await screen.findByText('Stopp sendt …')).toBeTruthy()
    expect(stop.stopSpaceRun).toHaveBeenCalledTimes(2)
    expect(stop.stopSpaceRun.mock.calls[1]?.[2]).toBe(stop.stopSpaceRun.mock.calls[0]?.[2])
  })

  it('offers no Stop once the run has ended, even to a manager', () => {
    const { container } = renderCard(view({ steps: [], finished: { outcome: 'stopped', reason: null } }), { stop: target })
    expect(stopButton(container)).toBeNull()
  })
})

describe('SpaceRunWorkSummary: the stored summary of a turn (WS09-S3, WS09-S5)', () => {
  it('draws nothing when the turn carries no steps and no steer', () => {
    const { container } = render(() => <SpaceRunWorkSummary work={{ v: 1, steps: [], steer: 0 }} />)
    expect(container.querySelector('details')).toBeNull()
    expect(container.textContent).toBe('')
  })

  it('says what the agent used, in time when the steps carry it, and lists the steps by kind and outcome', () => {
    const work: SpaceRunWork = {
      v: 1,
      steps: [
        { id: 'a', class: 'web', state: 'ok', ms: 4200 },
        { id: 'b', class: 'code', state: 'error' },
      ],
      steer: 0,
    }
    const { container } = render(() => <SpaceRunWorkSummary work={work} />)
    expect(container.querySelector('summary')?.textContent).toBe('Brukte 2 verktøy på 4 s')
    expect([...container.querySelectorAll('li')].map((line) => line.textContent)).toEqual([
      'Søker på nettet, ferdig',
      'Kjører kode, feilet',
    ])
  })

  it('says the steer when the steer is the whole story', () => {
    const { container } = render(() => <SpaceRunWorkSummary work={{ v: 1, steps: [], steer: 1 }} />)
    expect(container.querySelector('summary')?.textContent).toBe('Brukte ingen verktøy, 1 styring fra eieren')
  })
})
