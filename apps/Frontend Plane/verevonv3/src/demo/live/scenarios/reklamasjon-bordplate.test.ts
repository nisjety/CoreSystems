/**
 * The reklamasjon case (Ask): its frames, its gate and both decisions, its triggers
 * and its visitor-visible text. Pure functions, no browser, no registry import.
 */
import { describe, expect, it } from 'vitest'
import { anbudQuestion } from '../../fixtures/fjordform-anbud'
import {
  reklamasjonAnswerChunks,
  reklamasjonApproval,
  reklamasjonArtifactMarkdown,
  reklamasjonArtifactTitle,
  reklamasjonDeliveryStep,
  reklamasjonFollowChunks,
  reklamasjonQuestion,
  reklamasjonReplyStep,
  reklamasjonSeriesStep,
  reklamasjonSources,
  reklamasjonSuggestion,
  reklamasjonTitle,
  reklamasjonWarrantyStep,
} from '../../fixtures/fjordform-reklamasjon'
import { tilbudQuestion } from '../../fixtures/fjordform-tilbud'
import { chunkTextOf } from '../frames'
import { matchScenario, normalizeText, scoreTriggers } from '../match'
import type { StreamFrame, TurnContext } from '../scenario-types'
import { reklamasjonScenario } from './reklamasjon-bordplate'

/** Words and numbers the demo must never show (the common list of PLAN_2026-10-11.md and the cases). */
const FORBIDDEN = [
  /\bIma\b/i,
  /aquatiq/i,
  /velion/i,
  /dacosta/i,
  /nordlys/i,
  /\bbring\b/i,
  /ordre\s+4821/i,
  /faktura\s+20471/i,
  /\b4821\b/,
  /\b20471\b/,
  /forsinke/i,
]

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

function stepsOf(frames: readonly StreamFrame[], id: string): StreamFrame[] {
  return eventsOf(frames, 'step_update').filter((item) => item.data.id === id)
}

describe('reklamasjon case: registration', () => {
  it('is an Ask case with its own id, title, label and start-screen card, and no seeded thread', () => {
    expect(reklamasjonScenario.id).toBe('reklamasjon-bordplate')
    expect(reklamasjonScenario.mode).toBe('ask')
    expect(reklamasjonScenario.title).toBe(reklamasjonTitle)
    expect(reklamasjonScenario.label).toBe('Reklamasjon fra Havnebrygga Kafé')
    expect(reklamasjonScenario.suggestion).toEqual({
      text: reklamasjonSuggestion.text,
      source: reklamasjonSuggestion.source,
      time: reklamasjonSuggestion.time,
      icon: 'package',
    })
    expect(reklamasjonScenario.suggestion.text).toBe(reklamasjonQuestion)
    expect('seedThreadId' in reklamasjonScenario).toBe(false)
  })

  it('is deterministic: the same context gives the same turn', () => {
    expect(reklamasjonScenario.turn(contextFor())).toEqual(reklamasjonScenario.turn(contextFor()))
  })
})

describe('reklamasjon case: the turn frames', () => {
  const ctx = contextFor()
  const script = reklamasjonScenario.turn(ctx)
  const chat = script.chat

  it('frames are in time order, and no frame is repeated', () => {
    for (let index = 1; index < chat.length; index += 1) {
      expect(chat[index]!.at).toBeGreaterThanOrEqual(chat[index - 1]!.at)
    }
    const keys = chat.map((item) => JSON.stringify(item))
    expect(new Set(keys).size).toBe(keys.length)
  })

  it('opens with connected, ends with exactly one done', () => {
    expect(chat[0]?.event).toBe('connected')
    expect(chat.at(-1)?.event).toBe('done')
    expect(eventsOf(chat, 'done')).toHaveLength(1)
  })

  it('carries the thread and run id, and the request id on every frame that names one', () => {
    const connected = eventsOf(chat, 'connected')[0]
    expect(connected?.data.thread_id).toBe(ctx.threadId)
    expect(connected?.data.run_id).toBe(ctx.runId)
    for (const item of chat) {
      if ('request_id' in item.data) expect(item.data.request_id).toBe(ctx.requestId)
    }
  })

  it('titles the first turn only', () => {
    expect(eventsOf(chat, 'title').map((item) => item.data.title)).toEqual([reklamasjonTitle])
    const later = reklamasjonScenario.turn(contextFor({ firstTurn: false }))
    expect(eventsOf(later.chat, 'title')).toHaveLength(0)
  })

  it('cites the four sources, 120 ms apart, in one group, before the answer streams', () => {
    const citations = eventsOf(chat, 'citation')
    expect(citations.map((item) => item.data.id)).toEqual(reklamasjonSources.map((source) => source.id))
    expect(citations.map((item) => item.at)).toEqual([270, 390, 510, 630])
    expect([...new Set(citations.map((item) => item.data.source_group_id))]).toEqual(['kb:fjordform'])
    const firstChunk = eventsOf(chat, 'chunk')[0]
    expect(citations.every((item) => item.at < (firstChunk?.at ?? 0))).toBe(true)
  })

  it('streams the answer: its chunks read back to the answer text, which is both paragraphs', () => {
    expect(script.answer).toBe(chunkTextOf(chat))
    expect(script.answer).toBe([...reklamasjonAnswerChunks, ...reklamasjonFollowChunks].join(''))
    expect(script.answer.startsWith(reklamasjonAnswerChunks.join(''))).toBe(true)
  })

  it('finishes the three checks in order, and the reply step runs only after the last of them', () => {
    const doneRow = (id: string) => stepsOf(chat, id).find((item) => item.data.status === 'done')
    const warranty = doneRow(reklamasjonWarrantyStep.id)
    const delivery = doneRow(reklamasjonDeliveryStep.id)
    const series = doneRow(reklamasjonSeriesStep.id)
    expect(warranty?.data.title).toBe(reklamasjonWarrantyStep.title)
    expect(delivery?.data.detail).toBe(reklamasjonDeliveryStep.detail)
    expect(series?.data.title).toBe(reklamasjonSeriesStep.title)
    expect(warranty!.at).toBeLessThan(delivery!.at)
    expect(delivery!.at).toBeLessThan(series!.at)
    const running = stepsOf(chat, ctx.runId).find((item) => item.data.status === 'running')
    expect(series!.at).toBeLessThan(running!.at)
  })

  it('revises one artifact three times, the claim, then the remedy, then the reply, as one draft', () => {
    const revisions = eventsOf(chat, 'artifact')
    const stages = [1, 2, 3] as const
    expect(revisions.map((item) => item.data.content)).toEqual(stages.map((stage) => reklamasjonArtifactMarkdown(stage)))
    expect(revisions.every((item) => item.data.id === ctx.artifactId && item.data.version === 1)).toBe(true)
    expect(revisions.every((item) => item.data.title === reklamasjonArtifactTitle && item.data.kind === 'markdown')).toBe(true)
    const lengths = revisions.map((item) => String(item.data.content).length)
    expect(lengths[0]).toBeLessThan(lengths[1]!)
    expect(lengths[1]).toBeLessThan(lengths[2]!)
    const ats = revisions.map((item) => item.at)
    expect(ats[0]).toBeLessThan(ats[1]!)
    expect(ats[1]).toBeLessThan(ats[2]!)
  })

  it('pauses at a gate: the reply step on the run id, awaiting approval, after the second paragraph', () => {
    const awaiting = eventsOf(chat, 'step_update').filter((item) => item.data.status === 'awaiting_approval')
    expect(awaiting).toHaveLength(1)
    expect(awaiting[0]?.data.id).toBe(ctx.runId)
    expect(awaiting[0]?.data.title).toBe(reklamasjonReplyStep.title)
    expect(awaiting[0]?.data.detail).toBe(reklamasjonReplyStep.detail)
    const lastChunk = eventsOf(chat, 'chunk').at(-1)
    expect(awaiting[0]!.at).toBeGreaterThan(lastChunk!.at)
    expect(script.run).toEqual([])
  })

  it('shows the approval card with the case’s own kind and detail', () => {
    expect(script.gate?.kind).toBe(reklamasjonApproval.kind)
    expect(script.gate?.detail).toBe(reklamasjonApproval.detail)
  })
})

describe('reklamasjon case: the two decisions', () => {
  const ctx = contextFor()
  const gate = reklamasjonScenario.turn(ctx).gate!

  it('approve completes the run: the reply is a draft, nothing is sent, the table top is ordered for Thursday 15 October', () => {
    const { approve } = gate
    expect(approve.runOutcome).toBe('completed')
    expect(approve.run).toEqual([])
    expect(approve.text.startsWith('\n\n**Godkjent.**')).toBe(true)
    expect(approve.text).toContain('bordplate')
    expect(approve.text).toContain('bestilt')
    expect(approve.text).toContain('torsdag 15. oktober')
    expect(approve.text).toContain('utkast')
    expect(chunkTextOf(approve.chat)).toBe(approve.text)
  })

  it('reject ends the run as cancelled: nothing is ordered and the reply is discarded', () => {
    const { reject } = gate
    expect(reject.runOutcome).toBe('cancelled')
    expect(reject.run).toEqual([])
    expect(reject.text.startsWith('\n\n**Avvist.**')).toBe(true)
    expect(reject.text).toContain('Ingen bordplate er bestilt')
    expect(reject.text).toContain('forkastet')
    expect(chunkTextOf(reject.chat)).toBe(reject.text)
  })

  it('both decisions say that nothing was sent', () => {
    for (const decision of [gate.approve, gate.reject]) {
      expect(decision.text).toContain('Ingenting er sendt')
    }
  })

  it('each decision settles the reply step on the run id, and its frames run in time order', () => {
    for (const decision of [gate.approve, gate.reject]) {
      expect(stepsOf(decision.chat, ctx.runId).map((item) => item.data.status)).toEqual(['done'])
      const ats = decision.chat.map((item) => item.at)
      expect(ats).toEqual([...ats].sort((a, b) => a - b))
    }
  })

  it('approve adds version 2 to the artifact: the same reply, with a final note that it is a draft and not sent', () => {
    const revisions = eventsOf(gate.approve.chat, 'artifact')
    expect(revisions).toHaveLength(1)
    expect(revisions[0]?.data.id).toBe(ctx.artifactId)
    expect(revisions[0]?.data.version).toBe(2)
    const content = String(revisions[0]?.data.content)
    expect(content.startsWith(reklamasjonArtifactMarkdown(3))).toBe(true)
    expect(content.trimEnd().split('\n').at(-1)).toMatch(/utkast.*ikke sendt/i)
  })

  it('reject adds version 2 with a note that the reply is discarded and not sent', () => {
    const revisions = eventsOf(gate.reject.chat, 'artifact')
    expect(revisions).toHaveLength(1)
    expect(revisions[0]?.data.id).toBe(ctx.artifactId)
    expect(revisions[0]?.data.version).toBe(2)
    const content = String(revisions[0]?.data.content)
    expect(content.trimEnd().split('\n').at(-1)).toMatch(/forkastet.*ikke sendt/i)
  })
})

describe('reklamasjon case: the triggers', () => {
  const { triggers } = reklamasjonScenario

  it('every term is a word in the normalised form, or a stem with a trailing *', () => {
    for (const group of triggers) {
      for (const term of group) {
        expect(term).toMatch(/^[a-z0-9]+\*?$/)
        const word = term.replace(/\*$/, '')
        expect(normalizeText(word)).toBe(word)
      }
    }
  })

  it('a single term names the case only as a number or as the customer, and any other group has at least two words', () => {
    const customer = 'havnebrygga' // «Havnebrygga Kafé», as the question names it: its first word is the customer
    for (const group of triggers) {
      if (group.length === 1) {
        const term = group[0]!
        expect(/^\d+$/.test(term) || term === customer || term === `${customer}*`, term).toBe(true)
      } else {
        expect(group.length).toBeGreaterThanOrEqual(2)
      }
    }
  })

  it('the case’s own question names the customer, so it matches that group and not the series', () => {
    expect(scoreTriggers(normalizeText(reklamasjonQuestion), triggers)).toBe(1)
  })

  it('a common word, or a series number without the word serie, does not answer the visitor', () => {
    for (const word of ['garanti', 'bordplate', 'sprekk', 'serie', 'reklamasjon', '2609']) {
      expect(scoreTriggers(normalizeText(word), triggers), word).toBe(0)
    }
  })

  it('the questions of the other two cases do not fire these triggers', () => {
    expect(scoreTriggers(normalizeText(tilbudQuestion), triggers)).toBe(0)
    expect(scoreTriggers(normalizeText(anbudQuestion), triggers)).toBe(0)
  })

  it('a customer named alone, or in a damage message, answers the case', () => {
    for (const message of ['Havnebrygga', 'Havnebrygga AS har fått en skade på bordet']) {
      expect(matchScenario(message, [reklamasjonScenario])?.scenario.id, message).toBe('reklamasjon-bordplate')
    }
  })
})

describe('reklamasjon case: visitor-visible text', () => {
  it('names no forbidden word, and only fictional addresses and hosts, in every frame, text and card', () => {
    const script = reklamasjonScenario.turn(contextFor())
    const everything = [
      JSON.stringify(script.chat),
      script.answer,
      JSON.stringify(script.gate),
      reklamasjonScenario.label,
      reklamasjonScenario.title,
      reklamasjonScenario.suggestion.text,
      reklamasjonScenario.suggestion.source,
    ].join('\n')
    for (const pattern of FORBIDDEN) expect(everything, String(pattern)).not.toMatch(pattern)
    for (const email of everything.match(/[\w.+-]+@[\w.-]+/g) ?? []) {
      expect(email.endsWith('@fjordform.example')).toBe(true)
    }
    for (const host of everything.match(/https?:\/\/[^/"\s]+/g) ?? []) {
      expect(host).toMatch(/\.fjordform\.example$/)
    }
  })
})
