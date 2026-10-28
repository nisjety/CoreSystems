/**
 * Case 1 · Tilbud til Solhaug Hotell as a turn: the Do run (sub-tasks, sources,
 * the quote in three revisions, the pause), the Ask variant without «Utfør», the
 * two decisions, and the quote's own numbers. No browser step and no screenshot.
 * Pure functions, no browser.
 */
import { describe, expect, it } from 'vitest'
import {
  tilbudAnswerChunks,
  tilbudApproval,
  tilbudArtifactMarkdown,
  tilbudFollowChunks,
  tilbudQuoteStep,
  tilbudSources,
  tilbudSubtasks,
  tilbudTitle,
} from '../../fixtures/fjordform-tilbud'
import { chunkTextOf } from '../frames'
import type { StreamFrame, TurnContext } from '../scenario-types'
import { tilbudScenario } from './tilbud-solhaug'

const STAGES = [1, 2, 3] as const

function contextFor(overrides: Partial<TurnContext> = {}): TurnContext {
  return {
    requestId: 'req_tilbud',
    threadId: 'thr_tilbud',
    runId: 'run_tilbud',
    approvalId: 'apr_tilbud',
    artifactId: 'art_tilbud',
    planMode: true,
    model: 'nordisk-standard',
    firstTurn: true,
    ...overrides,
  }
}

function eventsOf(frames: readonly StreamFrame[], name: string): StreamFrame[] {
  return frames.filter((item) => item.event === name)
}

const doTurn = tilbudScenario.turn(contextFor())
const askTurn = tilbudScenario.turn(contextFor({ planMode: false }))

describe('the case as the start screen offers it', () => {
  it('is the Do case «Tilbud til Solhaug Hotell», with the mail icon, the quote title and no seeded thread', () => {
    expect(tilbudScenario.id).toBe('tilbud-solhaug')
    expect(tilbudScenario.mode).toBe('do')
    expect(tilbudScenario.label).toBe('Tilbud til Solhaug Hotell')
    expect(tilbudScenario.title).toBe(tilbudTitle)
    expect(tilbudScenario.suggestion.icon).toBe('mail')
    expect(tilbudScenario.seedThreadId).toBeUndefined()
  })

  it('is started by the customer’s name alone, as a stem, and by no quote word', () => {
    expect(tilbudScenario.triggers).toEqual([['solhaug*']])
  })
})

describe('the Do turn with «Utfør»', () => {
  it('attaches the three sub-tasks in one batch on the run channel, before any of them finishes', () => {
    const attached = eventsOf(doTurn.run, 'subagent_attached')
    expect(attached).toHaveLength(3)
    expect(new Set(attached.map((item) => item.data.batch_id)).size).toBe(1)
    expect(attached.every((item) => item.at === 10)).toBe(true)
    expect(attached.map((item) => item.data.goal_summary)).toEqual(tilbudSubtasks.map((task) => task.title))
  })

  it('names each sub-task for the run panel, and never falls back to a generic name', () => {
    const roles = eventsOf(doTurn.run, 'subagent_attached').map((item) => item.data.role)
    expect(roles).toEqual(['Lagerkontroll', 'Prisberegning', 'Monteringsplan'])
  })

  it('finishes the sub-tasks 300 ms apart, from 1 510 ms, and each finishes the child it attached', () => {
    const stopped = eventsOf(doTurn.run, 'subagent_stopped')
    expect(stopped.map((item) => item.at)).toEqual([1_510, 1_810, 2_110])
    expect(stopped.map((item) => item.data.child_run_id)).toEqual(
      eventsOf(doTurn.run, 'subagent_attached').map((item) => item.data.child_run_id),
    )
  })

  it('calls the tool that starts the sub-tasks, with the same count, and reports them started', () => {
    const call = eventsOf(doTurn.chat, 'tool_call')[0]
    expect(call?.data.name).toBe('3_deloppgaver')
    expect(call?.data.args).toEqual({ deloppgaver: 3 })
    expect(eventsOf(doTurn.chat, 'tool_result')[0]?.data.output).toBe('3 deloppgaver startet')
  })

  it('opens the four sources as citations in one group, before the answer streams', () => {
    const citations = eventsOf(doTurn.chat, 'citation')
    expect(citations.map((item) => item.data.title)).toEqual(tilbudSources.map((source) => source.title))
    expect(new Set(citations.map((item) => item.data.source_group_id)).size).toBe(1)
    expect(citations.map((item) => item.at)).toEqual([270, 390, 510, 630])
    expect(citations.every((item) => item.at < 1_010)).toBe(true)
  })

  it('streams the answer and the follow-up exactly as the fixture writes them', () => {
    expect(doTurn.answer).toBe(tilbudAnswerChunks.join('') + tilbudFollowChunks.join(''))
    expect(doTurn.answer).toBe(chunkTextOf(doTurn.chat))
  })

  it('reveals the quote in three revisions of one document, each under its own version, with more of the quote', () => {
    const revisions = eventsOf(doTurn.chat, 'artifact')
    expect(revisions.map((item) => item.at)).toEqual([2_510, 2_760, 3_010])
    expect(revisions.every((item) => item.data.id === 'art_tilbud')).toBe(true)
    expect(revisions.map((item) => item.data.version)).toEqual([...STAGES])
    expect(revisions.map((item) => item.data.content)).toEqual(STAGES.map((stage) => tilbudArtifactMarkdown(stage)))
    expect(String(revisions[0]?.data.content)).not.toContain('100 672')
    expect(String(revisions[1]?.data.content)).toMatch(/100\s672 kr/)
    expect(String(revisions[2]?.data.content)).toContain('## Svar til kunden')
  })

  it('pauses the run at its gate after the answer is done, for this turn’s approval', () => {
    const paused = eventsOf(doTurn.run, 'run_paused_for_approval')
    expect(paused).toHaveLength(1)
    expect(paused[0]?.at).toBe(3_760)
    expect(paused[0]?.at).toBeGreaterThan(eventsOf(doTurn.chat, 'done')[0]?.at ?? 0)
    expect(paused[0]?.data).toMatchObject({ run_id: 'run_tilbud', approval_id: 'apr_tilbud' })
    expect(doTurn.gate?.kind).toBe(tilbudApproval.kind)
    expect(doTurn.gate?.detail).toBe(tilbudApproval.detail)
  })

  it('reserves nothing before approval: the answer and the quote say nothing is reserved or sent', () => {
    expect(doTurn.answer).not.toMatch(/reservert|sendt/i)
    for (const stage of STAGES) expect(tilbudArtifactMarkdown(stage)).not.toMatch(/reservert|sendt/i)
  })
})

describe('the decisions of the Do turn', () => {
  it('«Godkjenn» starts with a paragraph break, says nothing is sent, adds the quote as version 4 and resumes the run', () => {
    const gate = doTurn.gate!
    expect(gate.approve.text.startsWith('\n\n')).toBe(true)
    expect(gate.approve.text).toContain('**Godkjent.**')
    expect(gate.approve.text).toContain('Ingenting er sendt')
    expect(gate.approve.runOutcome).toBe('completed')
    expect(eventsOf(gate.approve.run, 'approval_state_changed')[0]?.data.to).toBe('APPROVAL_STATE_GRANTED')
    expect(eventsOf(gate.approve.run, 'run_resumed_after_approval')).toHaveLength(1)
    expect(gate.approve.chat[0]).toMatchObject({ event: 'step_update', data: { id: 'steg-tilbud', status: 'done' } })
    const revised = eventsOf(gate.approve.chat, 'artifact')[0]
    expect(revised?.data.id).toBe('art_tilbud')
    expect(revised?.data.version).toBe(4)
    const content = String(revised?.data.content)
    expect(content.startsWith(tilbudArtifactMarkdown(3))).toBe(true)
    expect(content).toContain('## Utført')
    expect(content).toContain('- [x] Ikke sendt til Solhaug Hotell AS')
  })

  it('«Avvis» says nothing was reserved or sent, cancels the run, does not resume it, and leaves the quote as it was', () => {
    const gate = doTurn.gate!
    expect(gate.reject.text.startsWith('\n\n')).toBe(true)
    expect(gate.reject.text).toContain('**Avvist.**')
    expect(gate.reject.text).toContain('Ingenting er reservert')
    expect(gate.reject.text).toContain('ingenting er sendt')
    expect(gate.reject.runOutcome).toBe('cancelled')
    expect(eventsOf(gate.reject.run, 'approval_state_changed')[0]?.data.to).toBe('APPROVAL_STATE_DENIED')
    expect(eventsOf(gate.reject.run, 'run_resumed_after_approval')).toHaveLength(0)
    expect(eventsOf(gate.reject.chat, 'artifact')).toHaveLength(0)
  })

  it('the approval is a revision of its own, after the three stages, so it never replaces a stage', () => {
    const stages = eventsOf(doTurn.chat, 'artifact').map((item) => Number(item.data.version))
    const approved = eventsOf(doTurn.gate!.approve.chat, 'artifact').map((item) => Number(item.data.version))
    expect(approved).toEqual([4])
    expect(Math.min(...approved)).toBeGreaterThan(Math.max(...stages))
  })
})

describe('the Ask variant, without «Utfør»', () => {
  it('runs the same work as steps on the chat stream: three running steps, each done with its finding', () => {
    const steps = eventsOf(askTurn.chat, 'step_update')
    const running = steps.filter((item) => item.data.status === 'running')
    expect(running.map((item) => item.data.title)).toEqual(['Lagerkontroll', 'Prisberegning', 'Monteringsplan'])
    const finished = steps.filter((item) => item.data.status === 'done')
    expect(finished.map((item) => item.at)).toEqual([1_510, 1_810, 2_110])
    expect(finished.map((item) => item.data.detail)).toEqual(tilbudSubtasks.map((task) => task.result))
    expect(finished.map((item) => item.data.id)).toEqual(running.map((item) => item.data.id))
  })

  it('has no run events and no tool call, and its gate is the run’s own step, awaiting approval, at 3 560 ms', () => {
    expect(askTurn.run).toEqual([])
    expect(eventsOf(askTurn.chat, 'tool_call')).toHaveLength(0)
    const gateStep = eventsOf(askTurn.chat, 'step_update').find((item) => item.data.status === 'awaiting_approval')
    expect(gateStep?.at).toBe(3_560)
    expect(gateStep?.data).toMatchObject({ id: 'run_tilbud', title: tilbudQuoteStep.title, detail: tilbudQuoteStep.detail })
    expect(askTurn.gate?.kind).toBe(tilbudApproval.kind)
  })

  it('«Godkjenn» sends its step and its text on the chat stream, with no run events', () => {
    const gate = askTurn.gate!
    expect(gate.approve.run).toEqual([])
    expect(gate.approve.chat[0]).toMatchObject({ event: 'step_update', data: { id: 'run_tilbud', status: 'done' } })
    expect(chunkTextOf(gate.approve.chat)).toBe(gate.approve.text)
    expect(eventsOf(gate.approve.chat, 'artifact')[0]?.data.version).toBe(4)
  })

  it('reveals the same quote in the same three revisions, versions 1 to 3, as its stages', () => {
    const revisions = eventsOf(askTurn.chat, 'artifact')
    expect(revisions.map((item) => item.data.version)).toEqual([...STAGES])
    expect(revisions.map((item) => item.data.content)).toEqual(STAGES.map((stage) => tilbudArtifactMarkdown(stage)))
  })
})

describe('no browser step and no screenshot', () => {
  it('sends no browser event in either mode, and names no screenshot, image or browser anywhere in the turn', () => {
    for (const script of [doTurn, askTurn]) {
      expect(script.run.filter((item) => item.event.startsWith('browser_'))).toEqual([])
      expect(JSON.stringify(script)).not.toMatch(/screenshot|\.png|browser/i)
    }
  })
})

describe('the quote’s numbers', () => {
  it('agree: 40 × 1 490 + 10 × 4 200, less 8 %, plus two days of assembly, gives 100 672 kr; VAT 25 168; total 125 840', () => {
    const goods = 40 * 1_490 + 10 * 4_200
    const discount = Math.round(goods * 0.08)
    const assembly = 2 * 3_600
    const exVat = goods - discount + assembly
    const vat = Math.round(exVat * 0.25)
    expect(goods).toBe(101_600)
    expect(discount).toBe(8_128)
    expect(exVat).toBe(100_672)
    expect(vat).toBe(25_168)
    expect(exVat + vat).toBe(125_840)
  })

  it('appear in the quote as the fixture writes them', () => {
    const quote = tilbudArtifactMarkdown(3)
    expect(quote).toMatch(/40 × 1\s490 kr = 59\s600 kr/)
    expect(quote).toMatch(/10 × 4\s200 kr = 42\s000 kr/)
    expect(quote).toContain('8 128 kr')
    expect(quote).toMatch(/Sum eks\. mva:\*\* 100\s672 kr/)
    expect(quote).toMatch(/Mva 25 %:\*\* 25\s168 kr/)
    expect(quote).toMatch(/Totalt:\*\* 125\s840 kr/)
  })

  it('the price finding of the sub-task agrees with the lines and the discount', () => {
    expect(tilbudSubtasks[1]?.result).toMatch(/101\s600 kr på varene, 8 % volumrabatt gir 93\s472 kr\./)
  })
})
