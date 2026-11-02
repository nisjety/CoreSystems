/**
 * The tender case (anbud-miljodokumentasjon): its frames, its gate and both decisions,
 * the document validity against the fixture's dates, its trigger terms and its visible
 * text. Pure functions, no browser.
 */
import { describe, expect, it } from 'vitest'
import {
  anbudAnswerChunks,
  anbudApproval,
  anbudArtifactMarkdown,
  anbudArtifactTitle,
  anbudFollowChunks,
  anbudReplyStep,
  anbudRequirementStep,
  anbudSources,
  anbudSuggestion,
  anbudTitle,
  anbudValidityStep,
} from '../../fixtures/fjordform-anbud'
import { reklamasjonSuggestion } from '../../fixtures/fjordform-reklamasjon'
import { tilbudSuggestion } from '../../fixtures/fjordform-tilbud'
import { chunkTextOf } from '../frames'
import { EXACT_SCORE, matchScenario, normalizeText, scoreTriggers } from '../match'
import type { Gate, StreamFrame, TurnContext, TurnScript } from '../scenario-types'
import { DEMO_NOW_ISO } from '../shared-types'
import { anbudScenario } from './anbud-miljodokumentasjon'

const ctx: TurnContext = {
  requestId: 'req_anbud',
  threadId: 'thr_anbud',
  runId: 'run_anbud',
  approvalId: 'apr_anbud',
  artifactId: 'art_anbud',
  planMode: false,
  model: 'nordisk-standard',
  firstTurn: true,
}

const turn = anbudScenario.turn(ctx)

function gateOf(script: TurnScript): Gate {
  if (!script.gate) throw new Error('the anbud turn has no gate')
  return script.gate
}

function framesOf(frames: readonly StreamFrame[], event: string): StreamFrame[] {
  return frames.filter((item) => item.event === event)
}

function contentOf(item: StreamFrame | undefined): string {
  return String(item?.data.content ?? '')
}

function lastChunkAt(frames: readonly StreamFrame[]): number {
  return Math.max(...framesOf(frames, 'chunk').map((item) => item.at))
}

function sortedByTime(frames: readonly StreamFrame[]): boolean {
  const times = frames.map((item) => item.at)
  return times.every((time, index) => index === 0 || (times[index - 1] ?? 0) <= time)
}

const MONTHS: Record<string, number> = { mars: 3, sept: 9, september: 9, okt: 10, oktober: 10 }

type Validity = { status: 'gyldig' | 'utlopt'; date: string }

/** A statement such as «gyldig til 31. okt 2026» or «Utløpt 1. september 2026», read as a status and an ISO date. */
function validityOf(text: string): Validity | undefined {
  const match = text.match(/(gyldig til|utløpt) (\d{1,2})\. ([a-zæøå]+) (\d{4})/i)
  if (!match) return undefined
  const month = MONTHS[normalizeText(match[3] ?? '')]
  if (!month) return undefined
  return {
    status: /^utl/i.test(match[1] ?? '') ? 'utlopt' : 'gyldig',
    date: `${match[4]}-${String(month).padStart(2, '0')}-${(match[2] ?? '').padStart(2, '0')}`,
  }
}

function daysBetween(from: string, to: string): number {
  return (Date.parse(to) - Date.parse(from)) / 86_400_000
}

describe('anbudScenario', () => {
  it('is an Ask turn with the case id, title, label and receipt suggestion, and no seeded thread', () => {
    expect(anbudScenario.id).toBe('anbud-miljodokumentasjon')
    expect(anbudScenario.mode).toBe('ask')
    expect(anbudScenario.title).toBe(anbudTitle)
    expect(anbudScenario.label).toBe('Dokumentasjon til Nordhavn kommune')
    expect(anbudScenario.suggestion).toEqual({
      text: anbudSuggestion.text,
      source: anbudSuggestion.source,
      time: anbudSuggestion.time,
      icon: 'receipt',
    })
    expect(anbudScenario.seedThreadId).toBeUndefined()
  })
})

describe('the frames of the turn', () => {
  it('are ordered by time, and so are both decisions', () => {
    expect(sortedByTime(turn.chat)).toBe(true)
    expect(sortedByTime(gateOf(turn).approve.chat)).toBe(true)
    expect(sortedByTime(gateOf(turn).reject.chat)).toBe(true)
  })

  it('are unique: no frame is sent twice, and no settled event repeats its payload', () => {
    const frames = turn.chat.map((item) => `${item.at}|${item.event}|${JSON.stringify(item.data)}`)
    expect(new Set(frames).size).toBe(frames.length)
    const settled = turn.chat
      .filter((item) => item.event !== 'chunk')
      .map((item) => `${item.event}|${JSON.stringify(item.data)}`)
    expect(new Set(settled).size).toBe(settled.length)
  })

  it('has exactly one done, and it is the last frame', () => {
    expect(framesOf(turn.chat, 'done')).toHaveLength(1)
    expect(turn.chat[turn.chat.length - 1]?.event).toBe('done')
  })

  it('opens with the connection that names the run, and sends the title on the first turn only', () => {
    expect(turn.chat[0]?.event).toBe('connected')
    expect(turn.chat[0]?.data).toMatchObject({
      request_id: ctx.requestId,
      thread_id: ctx.threadId,
      model_used: ctx.model,
      run_id: ctx.runId,
    })
    expect(framesOf(turn.chat, 'title').map((item) => item.data.title)).toEqual([anbudTitle])
    expect(framesOf(anbudScenario.turn({ ...ctx, firstTurn: false }).chat, 'title')).toHaveLength(0)
  })

  it('shows four citations, 120 ms apart, in the fixture order', () => {
    const citations = framesOf(turn.chat, 'citation')
    expect(citations.map((item) => item.data.id)).toEqual(anbudSources.map((source) => source.id))
    expect(citations.map((item) => item.at)).toEqual([270, 390, 510, 630])
  })

  it('streams the answer, with the requirement and validity steps done in that order', () => {
    const steps = framesOf(turn.chat, 'step_update')
    const requirement = steps.findIndex((item) => item.data.id === anbudRequirementStep.id)
    const validity = steps.findIndex((item) => item.data.id === anbudValidityStep.id)
    expect(requirement).toBeGreaterThanOrEqual(0)
    expect(validity).toBeGreaterThan(requirement)
    expect(steps[requirement]?.data).toMatchObject({
      title: anbudRequirementStep.title,
      detail: anbudRequirementStep.detail,
      status: 'done',
    })
    expect(steps[validity]?.data).toMatchObject({
      title: anbudValidityStep.title,
      detail: anbudValidityStep.detail,
      status: 'done',
    })
    expect(framesOf(turn.chat, 'chunk')[0]?.at).toBe(1_010)
  })

  it('streams the answer text exactly: the chunks are the answer and then the second paragraph', () => {
    expect(turn.answer).toBe(chunkTextOf(turn.chat))
    expect(turn.answer).toBe([...anbudAnswerChunks, ...anbudFollowChunks].join(''))
  })

  it('finishes the answer before the gate opens', () => {
    const gateStep = framesOf(turn.chat, 'step_update').find((item) => item.data.status === 'awaiting_approval')
    expect(gateStep).toBeDefined()
    expect(lastChunkAt(turn.chat)).toBeLessThan(gateStep?.at ?? 0)
  })

  it('times the streams as the header says: the answer 1010 to 1685, the second paragraph 2500 to 3220', () => {
    const chunks = framesOf(turn.chat, 'chunk')
    const answer = chunks.filter((item) => item.at < 2_000)
    const second = chunks.filter((item) => item.at >= 2_000)
    expect([answer[0]?.at, answer[answer.length - 1]?.at]).toEqual([1_010, 1_685])
    expect([second[0]?.at, second[second.length - 1]?.at]).toEqual([2_500, 3_220])
  })

  it('revises the document three times, each under its own version: the check, the actions, the reply', () => {
    const revisions = framesOf(turn.chat, 'artifact')
    expect(revisions.map((item) => item.data.version)).toEqual([1, 2, 3])
    expect(revisions.map(contentOf)).toEqual(([1, 2, 3] as const).map((stage) => anbudArtifactMarkdown(stage)))
    for (const item of revisions) {
      expect(item.data).toMatchObject({ id: ctx.artifactId, kind: 'markdown', title: anbudArtifactTitle })
    }
  })

  it('grows the document from one revision to the next', () => {
    const contents = framesOf(turn.chat, 'artifact').map(contentOf)
    expect(contents).toHaveLength(3)
    const [first = '', second = '', third = ''] = contents
    expect(second.startsWith(first)).toBe(true)
    expect(third.startsWith(second)).toBe(true)
    expect(first.length).toBeLessThan(second.length)
    expect(second.length).toBeLessThan(third.length)
  })

  it('runs the reply step under the run id, then waits at the gate under the same id', () => {
    const reply = framesOf(turn.chat, 'step_update').filter((item) => item.data.id === ctx.runId)
    expect(reply.map((item) => item.data.status)).toEqual(['running', 'awaiting_approval'])
    expect(reply[0]?.data).toMatchObject({ title: anbudReplyStep.title, detail: anbudReplyStep.detail })
    expect(reply[1]?.at ?? 0).toBeGreaterThan(lastChunkAt(turn.chat))
    expect(reply[1]?.at ?? 0).toBeLessThan(turn.chat[turn.chat.length - 1]?.at ?? 0)
  })
})

describe('the approval gate and its decisions', () => {
  const gate = gateOf(turn)

  it('shows the card with what is saved and created, and says nothing is sent before a yes', () => {
    expect(gate.kind).toBe(anbudApproval.kind)
    expect(gate.detail).toBe(anbudApproval.detail)
    expect(gate.detail).toContain('Ingenting sendes før du godkjenner')
  })

  it('approve completes the run: a line that starts with a paragraph break, a draft to the municipality, a task for purchasing, nothing sent', () => {
    const { approve } = gate
    expect(approve.text.startsWith('\n\n')).toBe(true)
    expect(approve.text).toContain('utkast til Nordhavn kommune')
    expect(approve.text).toContain('«Forny EPD for FS-310» er opprettet for innkjøp')
    expect(approve.text).toContain('Ingenting er sendt')
    expect(chunkTextOf(approve.chat)).toBe(approve.text)
    expect(approve.runOutcome).toBe('completed')
    expect(approve.run).toEqual([])
  })

  it('approve closes the gate step as done, then adds a final note as revision 4, after the line', () => {
    const { chat } = gate.approve
    expect(chat[0]).toMatchObject({ event: 'step_update', at: 120, data: { id: ctx.runId, status: 'done' } })
    const notes = framesOf(chat, 'artifact')
    expect(notes).toHaveLength(1)
    expect(notes[0]?.data.version).toBe(4)
    const content = contentOf(notes[0])
    expect(content.startsWith(anbudArtifactMarkdown(3))).toBe(true)
    expect(content).toContain('Ikke sendt.')
    expect(notes[0]?.at ?? 0).toBeGreaterThan(lastChunkAt(chat))
  })

  it('reject saves and creates nothing: the step closes as done, the line says so, and one revision marks the reply as discarded', () => {
    const { reject } = gate
    expect(reject.text.startsWith('\n\n')).toBe(true)
    expect(reject.text).toContain('Ingenting er lagret')
    expect(reject.text).toContain('ingen oppgave er opprettet')
    expect(reject.text).not.toContain('Godkjent')
    expect(chunkTextOf(reject.chat)).toBe(reject.text)
    expect(reject.chat[0]).toMatchObject({ event: 'step_update', data: { id: ctx.runId, status: 'done' } })
    const notes = framesOf(reject.chat, 'artifact')
    expect(notes).toHaveLength(1)
    expect(notes[0]?.data.version).toBe(4)
    const content = contentOf(notes[0])
    expect(content.startsWith(anbudArtifactMarkdown(3))).toBe(true)
    expect(content).toContain('Svaret er forkastet.')
    expect(content).toContain('Ingenting er lagret eller sendt.')
    expect(notes[0]?.at ?? 0).toBeGreaterThan(lastChunkAt(reject.chat))
    expect(reject.runOutcome).toBe('cancelled')
    expect(reject.run).toEqual([])
  })

  it('streams each line from 260 ms after the click: the approval line ends at 890, the reject line at 620', () => {
    expect(framesOf(gate.approve.chat, 'chunk')[0]?.at).toBe(260)
    expect(framesOf(gate.reject.chat, 'chunk')[0]?.at).toBe(260)
    expect(lastChunkAt(gate.approve.chat)).toBe(890)
    expect(lastChunkAt(gate.reject.chat)).toBe(620)
  })
})

describe('the document validity, against the dates of the fixture header', () => {
  // Fixture header: today is Friday 9 October 2026, the deadline is 16 October 2026, the
  // FSC certificate is valid to 31 October 2026, the table EPD to 14 March 2029, and the
  // chair EPD ran out on 1 September 2026.
  const FACTS = {
    today: DEMO_NOW_ISO.slice(0, 10),
    deadline: '2026-10-16',
    fsc: '2026-10-31',
    tableEpd: '2029-03-14',
    chairEpd: '2026-09-01',
  }

  const snippetOf = (id: string): string => anbudSources.find((source) => source.id === id)?.snippet ?? ''

  it('reads the source snippets and the document lines as the fixture dates and statuses', () => {
    expect(validityOf(snippetOf('src_anbud_fsc'))).toEqual({ status: 'gyldig', date: FACTS.fsc })
    expect(validityOf(snippetOf('src_anbud_epd_bord'))).toEqual({ status: 'gyldig', date: FACTS.tableEpd })
    expect(validityOf(snippetOf('src_anbud_epd_stol'))).toEqual({ status: 'utlopt', date: FACTS.chairEpd })
    expect(anbudArtifactMarkdown(1).split('\n').map(validityOf)).toEqual([
      { status: 'gyldig', date: FACTS.fsc },
      { status: 'gyldig', date: FACTS.tableEpd },
      { status: 'utlopt', date: FACTS.chairEpd },
    ])
  })

  it('agrees with the demo clock: each statement says valid exactly when its date is not yet past', () => {
    const statements = [
      ...anbudSources.map((source) => validityOf(source.snippet)),
      ...anbudArtifactMarkdown(1).split('\n').map(validityOf),
    ].filter((item): item is Validity => item !== undefined)
    expect(statements).toHaveLength(6)
    for (const { status, date } of statements) {
      expect(status === 'gyldig').toBe(date >= FACTS.today)
    }
  })

  it('names the same dates in the answer and in the validity step', () => {
    expect(turn.answer).toContain('gyldig til 31. oktober')
    expect(turn.answer).toContain('gyldig til 2029')
    expect(turn.answer).toContain('utløp 1. september')
    expect(anbudValidityStep.detail).toContain('Stol-EPD utløp 1. september')
  })

  it('keeps the FSC certificate valid through the deadline, 22 days from today', () => {
    expect(FACTS.today).toBe('2026-10-09')
    expect(FACTS.fsc >= FACTS.deadline).toBe(true)
    expect(daysBetween(FACTS.today, FACTS.fsc)).toBe(22)
    expect(daysBetween(FACTS.today, FACTS.deadline)).toBe(7)
  })
})

describe('the trigger terms', () => {
  it('are whole words or stems ending in *, in the normalised form the matcher reads', () => {
    for (const group of anbudScenario.triggers) {
      for (const term of group) expect(term).toMatch(/^[a-z0-9]+\*?$/)
    }
  })

  it('name the customer alone, as a stem, and nothing else', () => {
    expect(anbudScenario.triggers).toEqual([['nordhavn*']])
  })

  it('answer the case question and the customer’s name in any form, with or without the Norwegian letters', () => {
    expect(matchScenario(anbudSuggestion.text, [anbudScenario])?.score).toBe(EXACT_SCORE)
    for (const text of [
      'Nordhavn kommune: hva trenger vi til tenderen?',
      'MILJØDOKUMENTASJON til Nordhavn-anbudet',
      'Nordhavns kommune sitt anbud',
      'Hva ma Nordhavn kommune ha av FSC og EPD?',
    ]) {
      expect(matchScenario(text, [anbudScenario])?.scenario.id, text).toBe(anbudScenario.id)
    }
  })

  it('fall back on the topic without the customer, even with FSC, EPD and anbud together', () => {
    for (const text of [
      'FSC EPD anbud',
      'Har dere gyldig FSC og EPD til anbudet?',
      'Hva må vi levere av FSC og EPD til anbud hos Bergen kommune?',
      'Trenger vi miljødokumentasjon til anbudet?',
      'Trenger vi miljodokumentasjon til anbudet?',
      'Har vi gyldige FSC og EPD-bevis til kjøkkenbenken?',
      'Kan dere sende miljødokumentasjon til en anbudskonkurranse?',
    ]) {
      expect(matchScenario(text, [anbudScenario]), text).toBeNull()
    }
  })

  it('answer on the customer’s name alone, and on no other single word of the case', () => {
    expect(matchScenario('Nordhavn', [anbudScenario])?.scenario.id).toBe(anbudScenario.id)
    for (const word of ['kommune', 'FSC', 'EPD', 'anbud', 'dokumentasjon', 'miljødokumentasjon']) {
      expect(matchScenario(word, [anbudScenario]), word).toBeNull()
    }
  })

  it("do not fire on the other two cases' questions", () => {
    for (const text of [tilbudSuggestion.text, reklamasjonSuggestion.text]) {
      expect(scoreTriggers(normalizeText(text), anbudScenario.triggers), text).toBe(0)
    }
  })
})

describe('the visible text', () => {
  it('contains no forbidden real-data word, in the metadata or in either variant of the turn', () => {
    const visible = JSON.stringify([
      anbudScenario.title,
      anbudScenario.label,
      anbudScenario.suggestion,
      turn,
      anbudScenario.turn({ ...ctx, firstTurn: false }),
    ])
    // The retired case's words are left out of tests on purpose (CASES_2026-10-12.md);
    // the bundle-wide forbidden-string scan of the verification step must check them.
    expect(visible).not.toMatch(/\bIma\b|Aquatiq|Velion|Dacosta/i)
  })

  it('names only fjordform.example addresses and hosts', () => {
    const visible = JSON.stringify([turn, gateOf(turn)])
    for (const email of visible.match(/[\w.+-]+@[\w.-]+/g) ?? []) {
      expect(email.endsWith('@fjordform.example')).toBe(true)
    }
    for (const source of anbudSources) {
      expect(new URL(source.url).hostname.endsWith('.fjordform.example')).toBe(true)
    }
  })
})
