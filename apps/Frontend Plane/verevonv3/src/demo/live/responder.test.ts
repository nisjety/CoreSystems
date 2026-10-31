/**
 * The responder with a manual clock. Time is advanced by hand, and the channels
 * are the real SSE streams, read back as text: the tests see the bytes the
 * product would receive. No browser, no real timers.
 */
import { afterEach, describe, expect, it } from 'vitest'
import { createStreamChannelFactory, type LiveChannel, type Scheduler } from './channel'
import { scenarioById, scenarios } from './scenarios'
import { createResponder, type Responder } from './responder'
import { createThreadStore, type ThreadStore } from './thread-store'
import type { ThreadRow } from './shared-types'
import type { Scenario, TurnContext, TurnScript } from './scenario-types'

const FIXED_NOW = Date.parse('2026-10-09T12:00:00+02:00')
/** Long enough for the longest decision text to stream and its channel to close. */
const SETTLE_MS = 10_000
const SEED: ThreadRow[] = [{
  threadId: 'thr_seed_1',
  title: 'Prisliste for 2027',
  preview: 'Utkast til prisliste ligger klart til gjennomgang.',
  updatedAt: '2026-10-08T09:00:00.000Z',
  pinned: false,
  personaEnabled: true,
}]

const FREE_QUESTION = 'Hva er været i Bergen i morgen?'

function caseOf(id: string): Scenario {
  const found = scenarioById(id)
  if (!found) throw new Error(`no case ${id}`)
  return found
}

const TILBUD = caseOf('tilbud-solhaug')
const TILBUD_QUESTION = TILBUD.suggestion.text

/** A case's turn as the responder would build it. Its gate and answer do not depend on the ids. */
function turnOf(scenario: Scenario, planMode = false): TurnScript {
  const ctx: TurnContext = {
    requestId: 'req_probe',
    threadId: 'thr_probe',
    runId: 'run_probe',
    approvalId: 'apr_probe',
    artifactId: 'art_probe',
    planMode,
    model: 'nordisk-standard',
    firstTurn: true,
  }
  return scenario.turn(ctx)
}

/** A scheduler whose time moves only when a test advances it. */
function manualClock(start = 1_000) {
  let clock = start
  let sequence = 0
  const timers = new Map<number, { due: number; callback: () => void }>()
  const scheduler: Scheduler = {
    now: () => clock,
    setTimeout(callback, ms) {
      sequence += 1
      timers.set(sequence, { due: clock + Math.max(0, ms), callback })
      return sequence
    },
    clearTimeout(handle) {
      timers.delete(handle as number)
    },
  }
  return {
    scheduler,
    now: () => clock,
    advance(ms: number) {
      const end = clock + ms
      for (;;) {
        let next: { id: number; due: number; callback: () => void } | undefined
        for (const [id, timer] of timers) {
          const earlier = !next || timer.due < next.due || (timer.due === next.due && id < next.id)
          if (timer.due <= end && earlier) next = { id, ...timer }
        }
        if (!next) break
        timers.delete(next.id)
        clock = next.due
        next.callback()
      }
      clock = end
    },
  }
}

type SseFrame = { event: string; data: Record<string, unknown> }

/** Parses an SSE body into its frames. */
function parseSse(text: string): SseFrame[] {
  return text
    .split('\n\n')
    .filter((block) => block.trim().length > 0)
    .map((block) => {
      const lines = block.split('\n')
      const event = lines.find((line) => line.startsWith('event: '))?.slice('event: '.length) ?? ''
      const data = lines.find((line) => line.startsWith('data: '))?.slice('data: '.length) ?? '{}'
      return { event, data: JSON.parse(data) as Record<string, unknown> }
    })
}

/** The text a list of frames streams: its chunk deltas, joined. */
function textOf(events: readonly SseFrame[]): string {
  return events
    .filter((item) => item.event === 'chunk')
    .map((item) => String(item.data.delta ?? ''))
    .join('')
}

/** Reads a channel's whole stream. Only call it once the channel has closed. */
async function readAll(channel: LiveChannel): Promise<SseFrame[]> {
  if (!channel.response) throw new Error('the channel has no stream')
  return parseSse(await channel.response.text())
}

type Harness = { clock: ReturnType<typeof manualClock>; store: ThreadStore; engine: Responder }

function harness(): Harness {
  const clock = manualClock()
  const store = createThreadStore(SEED)
  const engine = createResponder({
    store,
    scenarios,
    scheduler: clock.scheduler,
    openChannel: createStreamChannelFactory(clock.scheduler),
    wallNow: () => FIXED_NOW + (clock.now() - 1_000),
  })
  return { clock, store, engine }
}

let active: Harness | null = null

function start(): Harness {
  active = harness()
  return active
}

afterEach(() => {
  active?.engine.dispose()
  active = null
})

describe.each(scenarios.map((scenario) => [scenario.id, scenario] as const))('a case with a gate (%s)', (_id, scenario) => {
  const question = scenario.suggestion.text
  const gate = turnOf(scenario).gate!

  it('answers as an ask run, waits at the gate, and «Godkjenn» sends the outcome on the open chat', async () => {
    const { clock, store, engine } = start()
    const chat = engine.stream({ content: question, plan_mode: false, attachments: [] })
    clock.advance(4_000)
    expect(chat.isOpen).toBe(true)
    expect(engine.approvals('run_demo_1')).toEqual([{
      id: 'apr_demo_1',
      run_id: 'run_demo_1',
      kind: gate.kind,
      status: 'APPROVAL_STATE_REQUESTED',
      detail: gate.detail,
    }])
    expect(engine.run('run_demo_1')?.status).toBe('running')
    expect(engine.run('run_demo_1')?.mode).toBe('ask')

    expect(engine.decide('apr_demo_1', 'approve')?.approval.status).toBe('APPROVAL_STATE_GRANTED')
    clock.advance(SETTLE_MS)
    expect(chat.isOpen).toBe(false)

    const events = await readAll(chat)
    const doneAt = events.findIndex((item) => item.event === 'done')
    expect(doneAt).toBeGreaterThan(0)
    const afterDone = events.slice(doneAt + 1)
    expect(afterDone.some((item) => item.event === 'step_update' && item.data.status === 'done')).toBe(true)
    expect(textOf(afterDone)).toContain('**Godkjent.**')
    expect(engine.run('run_demo_1')?.status).toBe('completed')
    expect(store.get('thr_demo_1')?.messages.at(-1)?.content).toContain('**Godkjent.**')
  })

  it('«Avvis» says nothing was prepared or sent, and the run ends cancelled', async () => {
    const { clock, engine } = start()
    const chat = engine.stream({ content: question })
    clock.advance(4_000)
    expect(engine.decide('apr_demo_1', 'reject')?.approval.status).toBe('APPROVAL_STATE_DENIED')
    clock.advance(SETTLE_MS)
    const text = textOf(await readAll(chat))
    expect(text).toContain('**Avvist.**')
    expect(text).not.toContain('**Godkjent.**')
    expect(engine.run('run_demo_1')?.status).toBe('cancelled')
  })

  it('a second decision changes nothing and sends nothing more', async () => {
    const { clock, store, engine } = start()
    engine.stream({ content: question })
    clock.advance(4_000)
    engine.decide('apr_demo_1', 'approve')
    expect(engine.decide('apr_demo_1', 'reject')?.approval.status).toBe('APPROVAL_STATE_GRANTED')
    clock.advance(SETTLE_MS)
    const outcome = store.get('thr_demo_1')?.messages.at(-1)?.content ?? ''
    expect(outcome.match(/\*\*Godkjent\.\*\*/g)).toHaveLength(1)
    expect(outcome).not.toContain('**Avvist.**')
  })

  it('an unknown approval is refused, and reading approvals of an unknown run is empty', () => {
    const { engine } = start()
    expect(engine.decide('apr_missing', 'approve')).toBeUndefined()
    expect(engine.approvals('run_missing')).toEqual([])
    expect(engine.resume('run_missing')).toBe(false)
  })

  it('the run answers resume once it exists, and cancelling it ends its pending approval as denied', () => {
    const { clock, engine } = start()
    engine.stream({ content: question })
    clock.advance(4_000)
    expect(engine.resume('run_demo_1')).toBe(true)
    expect(engine.cancelRun('run_demo_1')).toEqual({ run_id: 'run_demo_1', cancelled: true, status: 'cancelled' })
    expect(engine.approvals('run_demo_1')[0]?.status).toBe('APPROVAL_STATE_DENIED')
    expect(engine.cancelRun('run_missing')).toBeUndefined()
  })

  it('the gate stays open for a long while; the newest three stay open when more pile up', () => {
    const { clock, engine } = start()
    const streams: LiveChannel[] = []
    for (let index = 0; index < 4; index += 1) {
      streams.push(engine.stream({ content: question }))
      clock.advance(4_000)
    }
    expect(streams[0]?.isOpen).toBe(false)
    expect(streams[3]?.isOpen).toBe(true)
    expect(engine.decide('apr_demo_1', 'approve')?.approval.status).toBe('APPROVAL_STATE_GRANTED')
  })

  it('starts its own run, whose goal is the question, with the gate of its case', () => {
    const { clock, engine } = start()
    engine.stream({ content: question })
    clock.advance(4_000)
    expect(engine.run('run_demo_1')?.goal).toBe(question)
    expect(engine.approvals('run_demo_1')[0]?.kind).toBe(gate.kind)
  })

  it('the thread store holds the question, the answer and the outcome, under the case title', async () => {
    const { clock, store, engine } = start()
    const chat = engine.stream({ content: question })
    clock.advance(4_000)
    engine.decide('apr_demo_1', 'approve')
    clock.advance(SETTLE_MS)
    await readAll(chat)
    const thread = store.get('thr_demo_1')
    expect(thread?.title).toBe(scenario.title)
    expect(thread?.messages.map((item) => item.role)).toEqual(['user', 'assistant'])
    expect(thread?.messages[0]?.content).toBe(question)
    expect(thread?.messages[1]?.content).toContain(turnOf(scenario).answer)
    expect(thread?.messages[1]?.content).toContain('**Godkjent.**')
    expect(thread?.preview.length).toBeGreaterThan(0)
  })
})

describe('the Do case run with «Utfør» (tilbud)', () => {
  const question = TILBUD_QUESTION

  it('a run panel that connects late receives the frames it missed at once, then the rest on schedule', async () => {
    const { clock, engine } = start()
    const chat = engine.stream({ content: question, plan_mode: true })
    clock.advance(500)
    const panel = engine.runEvents('run_demo_1')
    expect(panel).toBeDefined()
    clock.advance(4_000)
    chat.close()
    panel!.close()
    const names = (await readAll(panel!)).map((item) => item.event)
    expect(names.filter((name) => name === 'subagent_attached')).toHaveLength(3)
    expect(names.filter((name) => name === 'subagent_stopped')).toHaveLength(3)
    expect(names).not.toContain('browser_action_dispatched')
    expect(names.indexOf('subagent_attached')).toBeLessThan(names.indexOf('subagent_stopped'))
    expect(names.at(-1)).toBe('run_paused_for_approval')
  })

  it('a decision appends its run events to the open panel, and the run completes', async () => {
    const { clock, engine } = start()
    engine.stream({ content: question, plan_mode: true })
    clock.advance(4_000)
    const panel = engine.runEvents('run_demo_1')!
    clock.advance(100)
    engine.decide('apr_demo_1', 'approve')
    clock.advance(500)
    panel.close()
    const names = (await readAll(panel)).map((item) => item.event)
    expect(names.slice(-2)).toEqual(['approval_state_changed', 'run_resumed_after_approval'])
    expect(engine.run('run_demo_1')?.status).toBe('completed')
    expect(engine.run('run_demo_1')?.mode).toBe('plan')
  })

  it('«Avvis» cancels the run: the panel sees the denial, and the run is not resumed', async () => {
    const { clock, engine } = start()
    engine.stream({ content: question, plan_mode: true })
    clock.advance(4_000)
    const panel = engine.runEvents('run_demo_1')!
    clock.advance(100)
    engine.decide('apr_demo_1', 'reject')
    clock.advance(500)
    panel.close()
    const names = (await readAll(panel)).map((item) => item.event)
    expect(names.slice(-1)).toEqual(['approval_state_changed'])
    expect(names).not.toContain('run_resumed_after_approval')
    expect(engine.run('run_demo_1')?.status).toBe('cancelled')
  })

  it('without «Utfør» the same case is an ask run: it has no run events, and its gate is on the chat stream', async () => {
    const { clock, engine } = start()
    const chat = engine.stream({ content: question, plan_mode: false })
    clock.advance(4_000)
    expect(engine.run('run_demo_1')?.mode).toBe('ask')
    const panel = engine.runEvents('run_demo_1')
    expect(panel).toBeDefined()
    panel!.close()
    expect(await readAll(panel!)).toEqual([])
    expect(engine.approvals('run_demo_1')[0]?.status).toBe('APPROVAL_STATE_REQUESTED')
    chat.close()
  })

  it('an unknown run has no panel', () => {
    const { engine } = start()
    expect(engine.runEvents('run_missing')).toBeUndefined()
  })
})

describe('the fallback, and the honest edges', () => {
  it('a free-text question gets the demo answer with the three cases, and no gate', async () => {
    const { clock, engine } = start()
    const chat = engine.stream({ content: FREE_QUESTION })
    clock.advance(5_000)
    const events = await readAll(chat)
    expect(textOf(events)).toContain('Dette er en demo med forhåndsdefinerte svar')
    for (const id of ['tilbud-solhaug', 'reklamasjon-bordplate', 'anbud-miljodokumentasjon']) {
      expect(textOf(events)).toContain(`(#demo:${id})`)
    }
    expect(events.at(-1)?.event).toBe('done')
    expect(engine.approvals('run_demo_1')).toEqual([])
    expect(engine.run('run_demo_1')).toBeUndefined()
  })

  it.each([
    'Hva koster et tilbud på bord?',
    'Hva er garantien på bordplater?',
    'Dokumentasjon til anbud for Bergen kommune',
  ])('a near miss gets the same fallback, with the three cases as links and no gate: «%s»', async (question) => {
    const { clock, engine } = start()
    const chat = engine.stream({ content: question })
    clock.advance(5_000)
    const events = await readAll(chat)
    const text = textOf(events)
    expect(text).toContain('Dette er en demo med forhåndsdefinerte svar')
    for (const scenario of scenarios) expect(text).toContain(`[${scenario.label}](#demo:${scenario.id})`)
    expect(events.at(-1)?.event).toBe('done')
    expect(engine.approvals('run_demo_1')).toEqual([])
    expect(engine.run('run_demo_1')).toBeUndefined()
  })

  it('an attachment gets the attachment answer, not a case', async () => {
    const { clock, engine } = start()
    const chat = engine.stream({ content: TILBUD_QUESTION, attachments: [{ name: 'brev.pdf' }] })
    clock.advance(5_000)
    expect(textOf(await readAll(chat))).toContain('Vedlegg leses ikke i demoen')
    expect(engine.run('run_demo_1')).toBeUndefined()
  })

  it('an image request gets the image answer, not a case', async () => {
    const { clock, engine } = start()
    const chat = engine.stream({ content: TILBUD_QUESTION, generate_image: true })
    clock.advance(5_000)
    expect(textOf(await readAll(chat))).toContain('Bildegenerering er ikke med i demoen')
    expect(engine.run('run_demo_1')).toBeUndefined()
  })

  it('a temporary chat is answered, and never stored', async () => {
    const { clock, store, engine } = start()
    const before = store.sessions().length
    const chat = engine.stream({ content: FREE_QUESTION, zdr: true })
    clock.advance(5_000)
    expect(textOf(await readAll(chat))).toContain('Dette er en demo')
    expect(store.sessions()).toHaveLength(before)
    expect(store.get('thr_demo_1')).toBeUndefined()
  })

  it('a mid-run message is delivered while the answer streams, and acknowledged without losing the answer', async () => {
    const { clock, engine } = start()
    const chat = engine.stream({ content: FREE_QUESTION })
    clock.advance(300)
    expect(engine.queue('req_demo_1', 'Og i Oslo?')).toEqual({ queued: true, pending: 1, persisted: true })
    clock.advance(5_000)
    const events = await readAll(chat)
    expect(events.find((item) => item.event === 'queued_input')?.data.messages).toEqual(['Og i Oslo?'])
    const afterQueue = events.slice(events.findIndex((item) => item.event === 'queued_input'))
    expect(textOf(afterQueue)).toContain('Notert.')
    expect(textOf(events)).toContain('Dette er en demo med forhåndsdefinerte svar')
  })

  it('a message to a turn that has finished is sent again as a new turn', () => {
    const { clock, engine } = start()
    engine.stream({ content: FREE_QUESTION })
    clock.advance(5_000)
    expect(engine.queue('req_demo_1', 'Og i Oslo?')).toEqual({ queued: false, resend_as_new_turn: true })
    expect(engine.queue('req_missing', 'Og i Oslo?')).toEqual({ queued: false, resend_as_new_turn: true })
  })

  it('«Stopp svar» sends stopped and closes the stream; unknown turns are reported', async () => {
    const { clock, engine } = start()
    const chat = engine.stream({ content: FREE_QUESTION })
    clock.advance(200)
    expect(engine.cancelInvocation('req_demo_1')).toBe(true)
    expect(chat.isOpen).toBe(false)
    const events = await readAll(chat)
    expect(events.at(-1)).toMatchObject({ event: 'stopped', data: { request_id: 'req_demo_1', reason: 'cancelled' } })
    expect(engine.cancelInvocation('req_missing')).toBe(false)
  })
})

describe('threads and regeneration', () => {
  it('sends the title on the first turn only, and does not repeat a regenerated question', async () => {
    const { clock, store, engine } = start()
    const first = engine.stream({ content: FREE_QUESTION })
    clock.advance(5_000)
    expect((await readAll(first)).some((item) => item.event === 'title')).toBe(true)

    const again = engine.stream({ content: FREE_QUESTION, thread_id: 'thr_demo_1', regenerated: true })
    clock.advance(5_000)
    expect((await readAll(again)).some((item) => item.event === 'title')).toBe(false)

    const messages = store.get('thr_demo_1')?.messages ?? []
    expect(messages.filter((item) => item.role === 'user')).toHaveLength(1)
    expect(messages.filter((item) => item.role === 'assistant')).toHaveLength(2)
  })

  it('a case appears in the recent list only once a visitor has run it, under its title', () => {
    const { clock, store, engine } = start()
    expect(store.sessions().map((row) => row.title)).toEqual(['Prisliste for 2027'])
    engine.stream({ content: TILBUD_QUESTION })
    clock.advance(4_000)
    expect(store.sessions().map((row) => row.title)).toContain(TILBUD.title)
  })

  it('the plan-gate answer grants the rung as written, and a rejection grants nothing', () => {
    const { engine } = start()
    expect(engine.planApproval({ decision: 'approve', granted_rung: 'workspace_write' })).toEqual({
      granted_rung: 'workspace_write',
      was_in_plan_mode: true,
      persisted: true,
    })
    expect(engine.planApproval({ decision: 'reject' })).toEqual({ was_in_plan_mode: true, persisted: true })
  })

  it('reset restores the seeded threads and forgets every turn, run and approval', () => {
    const { clock, store, engine } = start()
    engine.stream({ content: TILBUD_QUESTION })
    clock.advance(4_000)
    engine.reset()
    expect(store.sessions().map((item) => item.threadId)).toEqual(['thr_seed_1'])
    expect(engine.run('run_demo_1')).toBeUndefined()
    expect(engine.approvals('run_demo_1')).toEqual([])
    expect(engine.decide('apr_demo_1', 'approve')).toBeUndefined()
  })
})
