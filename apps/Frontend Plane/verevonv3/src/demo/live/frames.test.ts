/**
 * Frame builders and the scripts of every case: the chat starts with
 * `connected` and ends with `done`, every frame carries the turn's ids, the
 * gate and the run events are where the product looks for them, and nothing a
 * visitor can read leaves the fictional world. Pure functions, no browser.
 */
import { describe, expect, it } from 'vitest'
import { chunkTextOf, DELTA_MS, encodeFrame, lastAt, paced } from './frames'
import type { StreamFrame, TurnContext } from './scenario-types'
import { scenarios } from './scenarios'
import { fallbackAnswer, fallbackTurn } from './scenarios/fallback'

/**
 * What a visitor must never read in the demo: the retired cases' customer, carrier
 * and numbers, a delivery delay, an invoice deviation, and the real companies' names.
 * A production deviation («avvik på limfuge») is the reklamasjon case's own content, not an invoice one.
 */
const FORBIDDEN_IN_VISIBLE_TEXT = /nordlys|\bbring\b|4821|20471|forsink|faktura\s*avvik|avvik\w*\s+(på\s+|i\s+)?faktura|aquatiq|velion|\bima\b/i

function contextFor(overrides: Partial<TurnContext> = {}): TurnContext {
  return {
    requestId: 'req_test',
    threadId: 'thr_test',
    runId: 'run_test',
    approvalId: 'apr_test',
    artifactId: 'art_test',
    planMode: false,
    model: 'nordisk-standard',
    firstTurn: true,
    ...overrides,
  }
}

function eventsOf(frames: readonly StreamFrame[], name: string): StreamFrame[] {
  return frames.filter((item) => item.event === name)
}

describe('paced', () => {
  it('splits text into two-word deltas that concatenate back to the exact text', () => {
    const text = 'Svaret er klart.\n\n> **Utkast:** Hei  der'
    const frames = paced(100, text, 'req_test')
    expect(frames.map((item) => item.data.delta).join('')).toBe(text)
    expect(frames.every((item) => item.event === 'chunk' && item.data.request_id === 'req_test')).toBe(true)
  })

  it('streams one delta every DELTA_MS, from the given time', () => {
    const frames = paced(1_000, 'a b c d e f', 'req_test')
    expect(frames.map((item) => item.at)).toEqual([1_000, 1_000 + DELTA_MS, 1_000 + 2 * DELTA_MS])
    expect(frames.map((item) => item.data.delta)).toEqual(['a b ', 'c d ', 'e f'])
  })

  it('returns nothing for empty text', () => {
    expect(paced(0, '', 'req_test')).toEqual([])
  })
})

describe('encodeFrame', () => {
  it('writes the SSE wire form: event, data, blank line', () => {
    expect(encodeFrame({ event: 'chunk', data: { delta: 'Hei' } })).toBe('event: chunk\ndata: {"delta":"Hei"}\n\n')
  })

  it('writes an id line when the frame has one', () => {
    expect(encodeFrame({ event: 'x', data: {}, id: '7' })).toBe('event: x\nid: 7\ndata: {}\n\n')
  })
})

describe('chunkTextOf and lastAt', () => {
  it('joins the chunk deltas and ignores other events', () => {
    const frames: StreamFrame[] = [
      { at: 0, event: 'chunk', data: { delta: 'Hei ' } },
      { at: 5, event: 'done', data: {} },
      { at: 9, event: 'chunk', data: { delta: 'der' } },
    ]
    expect(chunkTextOf(frames)).toBe('Hei der')
    expect(lastAt(frames)).toBe(9)
    expect(lastAt([], 4)).toBe(4)
  })
})

for (const scenario of scenarios) {
  for (const planMode of [false, true]) {
    describe(`scenario ${scenario.id} (plan mode ${planMode})`, () => {
      const ctx = contextFor({ planMode })
      const script = scenario.turn(ctx)
      const chat = [...script.chat].sort((a, b) => a.at - b.at)

      it('opens with connected and ends with done', () => {
        expect(chat[0]?.event).toBe('connected')
        expect(chat.at(-1)?.event).toBe('done')
        expect(eventsOf(script.chat, 'done')).toHaveLength(1)
      })

      it('carries the turn’s own ids on every frame that names one', () => {
        const connectedFrame = eventsOf(script.chat, 'connected')[0]
        expect(connectedFrame?.data.thread_id).toBe(ctx.threadId)
        expect(connectedFrame?.data.run_id).toBe(ctx.runId)
        for (const item of script.chat) {
          if ('request_id' in item.data) expect(item.data.request_id).toBe(ctx.requestId)
        }
      })

      it('answers with the text its chunks stream, and sends the title only on the first turn', () => {
        expect(script.answer).toBe(chunkTextOf(script.chat))
        expect(script.answer.length).toBeGreaterThan(20)
        expect(eventsOf(script.chat, 'title')).toHaveLength(1)
        const later = scenario.turn(contextFor({ planMode, firstTurn: false }))
        expect(eventsOf(later.chat, 'title')).toHaveLength(0)
      })

      it('has a gate with an approve and a reject outcome, each closing the run', () => {
        if (!script.gate) return
        expect(script.gate.approve.runOutcome).toBe('completed')
        expect(script.gate.reject.runOutcome).toBe('cancelled')
        expect(script.gate.approve.text).toContain('**Godkjent.**')
        expect(script.gate.reject.text).toContain('**Avvist.**')
        expect(chunkTextOf(script.gate.approve.chat)).toContain(script.gate.approve.text)
        expect(chunkTextOf(script.gate.reject.chat)).toContain(script.gate.reject.text)
      })
    })
  }
}

describe('the gate of each case', () => {
  it('without «Utfør» the gate is the run’s own step, awaiting approval, on the chat stream, with no run events or tool call', () => {
    for (const scenario of scenarios) {
      const ctx = contextFor({ planMode: false })
      const script = scenario.turn(ctx)
      const gateStep = eventsOf(script.chat, 'step_update').find((item) => item.data.status === 'awaiting_approval')
      expect(gateStep?.data.id, scenario.id).toBe(ctx.runId)
      expect(script.gate, scenario.id).toBeDefined()
      expect(script.run, scenario.id).toEqual([])
      expect(eventsOf(script.chat, 'tool_call'), scenario.id).toHaveLength(0)
    }
  })

  it('without «Utfør» the decisions send no run events, so the chat stream carries the whole outcome', () => {
    for (const scenario of scenarios) {
      const gate = scenario.turn(contextFor({ planMode: false })).gate!
      expect(gate.approve.run, scenario.id).toEqual([])
      expect(gate.reject.run, scenario.id).toEqual([])
    }
  })

  it('with «Utfør» a Do case sends its sub-tasks in one batch, no browser step, and pauses for this turn’s approval', () => {
    const doCases = scenarios.filter((item) => item.mode === 'do')
    expect(doCases.length).toBeGreaterThan(0)
    for (const scenario of doCases) {
      const ctx = contextFor({ planMode: true })
      const script = scenario.turn(ctx)
      const attached = eventsOf(script.run, 'subagent_attached')
      expect(attached.length, scenario.id).toBeGreaterThanOrEqual(2)
      expect(new Set(attached.map((item) => item.data.batch_id)).size, scenario.id).toBe(1)
      expect(eventsOf(script.run, 'browser_action_dispatched'), scenario.id).toHaveLength(0)
      expect(eventsOf(script.run, 'browser_observation_received'), scenario.id).toHaveLength(0)
      const paused = eventsOf(script.run, 'run_paused_for_approval')
      expect(paused, scenario.id).toHaveLength(1)
      expect(paused[0]?.data.approval_id, scenario.id).toBe(ctx.approvalId)
      expect(eventsOf(script.chat, 'tool_call')[0]?.data.name, scenario.id).toBe('3_deloppgaver')
    }
  })

  it('with «Utfør» a Do case resumes its run on approval only, and the approval is granted or denied on the run channel', () => {
    for (const scenario of scenarios.filter((item) => item.mode === 'do')) {
      const gate = scenario.turn(contextFor({ planMode: true })).gate!
      expect(eventsOf(gate.approve.run, 'run_resumed_after_approval'), scenario.id).toHaveLength(1)
      expect(eventsOf(gate.reject.run, 'run_resumed_after_approval'), scenario.id).toHaveLength(0)
      expect(eventsOf(gate.approve.run, 'approval_state_changed')[0]?.data.to, scenario.id).toBe('APPROVAL_STATE_GRANTED')
      expect(eventsOf(gate.reject.run, 'approval_state_changed')[0]?.data.to, scenario.id).toBe('APPROVAL_STATE_DENIED')
    }
  })
})

describe('the sources of each case', () => {
  for (const scenario of scenarios) {
    for (const planMode of [false, true]) {
      it(`${scenario.id} (plan mode ${planMode}) cites at least three sources in one group, before its answer streams`, () => {
        const script = scenario.turn(contextFor({ planMode }))
        const citations = eventsOf(script.chat, 'citation')
        expect(citations.length, scenario.id).toBeGreaterThanOrEqual(3)
        expect(new Set(citations.map((item) => item.data.source_group_id)).size, scenario.id).toBe(1)
        expect(
          citations.every((item) => typeof item.data.title === 'string' && typeof item.data.url === 'string'),
          scenario.id,
        ).toBe(true)
        const firstChunk = eventsOf(script.chat, 'chunk')[0]
        expect(citations.every((item) => item.at < (firstChunk?.at ?? 0)), scenario.id).toBe(true)
      })
    }
  }
})

describe('the fallback', () => {
  it('says plainly that this is a demo with predefined answers, and offers the three cases', () => {
    const turn = fallbackTurn({ ctx: contextFor(), reason: 'free-text', question: 'Hva er været?', offers: scenarios })
    expect(turn.answer).toContain('Dette er en demo med forhåndsdefinerte svar')
    for (const scenario of scenarios) expect(turn.answer).toContain(`(#demo:${scenario.id})`)
    expect(turn.gate).toBeUndefined()
    expect(turn.run).toEqual([])
    expect(turn.chat.at(-1)?.event).toBe('done')
  })

  it('offers the three cases in registry order, each under its own label', () => {
    expect(scenarios.map((scenario) => scenario.id)).toEqual(['tilbud-solhaug', 'reklamasjon-bordplate', 'anbud-miljodokumentasjon'])
    expect(scenarios.map((scenario) => scenario.label)).toEqual([
      'Tilbud til Solhaug Hotell',
      'Reklamasjon fra Havnebrygga Kafé',
      'Dokumentasjon til Nordhavn kommune',
    ])
    const answer = fallbackAnswer('free-text', scenarios)
    expect(answer).toContain('- [Tilbud til Solhaug Hotell](#demo:tilbud-solhaug)')
    expect(answer).toContain('- [Reklamasjon fra Havnebrygga Kafé](#demo:reklamasjon-bordplate)')
    expect(answer).toContain('- [Dokumentasjon til Nordhavn kommune](#demo:anbud-miljodokumentasjon)')
  })

  it('the cases carry the start screen’s icons: mail, package and receipt', () => {
    expect(scenarios.map((scenario) => scenario.suggestion.icon)).toEqual(['mail', 'package', 'receipt'])
  })

  it('names attachments and images as not part of the demo, and still offers the cases', () => {
    expect(fallbackAnswer('attachments', scenarios)).toContain('Vedlegg leses ikke i demoen')
    expect(fallbackAnswer('image', scenarios)).toContain('Bildegenerering er ikke med i demoen')
    expect(fallbackAnswer('image', scenarios)).toContain('#demo:tilbud-solhaug')
  })

  it('titles a first turn from the question', () => {
    const turn = fallbackTurn({ ctx: contextFor(), reason: 'free-text', question: 'Hva er været i Bergen?', offers: scenarios })
    expect(eventsOf(turn.chat, 'title')[0]?.data.title).toBe('Hva er været i Bergen?')
  })
})

describe('visitor-visible content', () => {
  it('names no real person, company or domain, and only fictional addresses and hosts', () => {
    const texts: string[] = [fallbackAnswer('free-text', scenarios)]
    for (const scenario of scenarios) {
      texts.push(scenario.suggestion.text, scenario.suggestion.source, scenario.label, scenario.title)
      for (const planMode of [false, true]) {
        const script = scenario.turn(contextFor({ planMode }))
        texts.push(JSON.stringify(script.chat), JSON.stringify(script.run), script.answer)
        if (script.gate) texts.push(JSON.stringify(script.gate))
      }
    }
    const everything = texts.join('\n')
    expect(everything).not.toMatch(/\bIma\b|aquatiq|velion|dacosta/i)
    for (const email of everything.match(/[\w.+-]+@[\w.-]+/g) ?? []) {
      expect(email.endsWith('@fjordform.example')).toBe(true)
    }
    for (const host of everything.match(/https?:\/\/[^/"\s]+/g) ?? []) {
      expect(host).toMatch(/\.fjordform\.example$/)
    }
  })

  it('names none of the retired cases: no customer of them, no order or invoice number, no delivery delay or invoice deviation', () => {
    const texts: string[] = [fallbackAnswer('free-text', scenarios), fallbackAnswer('image', scenarios)]
    for (const scenario of scenarios) {
      texts.push(scenario.suggestion.text, scenario.suggestion.source, scenario.label, scenario.title)
      for (const planMode of [false, true]) {
        const script = scenario.turn(contextFor({ planMode }))
        texts.push(JSON.stringify(script.chat), JSON.stringify(script.run), script.answer)
        if (script.gate) texts.push(JSON.stringify(script.gate))
      }
    }
    expect(texts.join('\n')).not.toMatch(FORBIDDEN_IN_VISIBLE_TEXT)
  })
})
