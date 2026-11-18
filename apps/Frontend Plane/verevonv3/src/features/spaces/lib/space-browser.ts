import { createEffect, createMemo, createSignal, untrack } from 'solid-js'

import {
  readSpaceBrowserTimeline,
  type SpaceBrowserEvent,
  type SpaceBrowserRunHeader,
} from '@/shared/api/spaces-client/browser-runs'

/**
 * The room browser run's presentation rules (WS20-S5). Receipts only: the labels name a closed kind, a host, a closed
 * status or a closed risk word, never page content. Presentation only; the gateway decides what a caller may read.
 */

/** A live card polls the timeline this often while it is visible (plan WS20-S5: 2.5 s). */
export const SPACE_BROWSER_POLL_MS = 2500
/** Pages read in one poll at most, so a long run cannot hold the poll open. */
const MAX_PAGES_PER_POLL = 10

/** The phases after which a run is over. The projection's closed words; `finishedAt` is the other signal. */
const TERMINAL_PHASES = new Set([
  'completed',
  'failed',
  'cancelled',
  'stopped',
  'timed_out',
  'blocked_by_site',
  'handoff_declined',
  'handoff_timeout',
])

/** Whether the run is still going, so its card keeps polling. */
export function isSpaceBrowserRunLive(run: Pick<SpaceBrowserRunHeader, 'phase' | 'finishedAt'>): boolean {
  return run.finishedAt === null && !TERMINAL_PHASES.has(run.phase)
}

type Tr = (noText: string, enText: string) => string

/** The run's phase as a person reads it. An unknown phase says so rather than guessing. */
export function spaceBrowserPhaseLabel(phase: string, tr: Tr): string {
  switch (phase) {
    case 'queued':
      return tr('Venter på å starte', 'Waiting to start')
    case 'running':
    case 'executing':
      return tr('Kjører', 'Running')
    case 'completed':
      return tr('Fullført', 'Completed')
    case 'failed':
      return tr('Mislyktes', 'Failed')
    case 'cancelled':
    case 'stopped':
      return tr('Stoppet', 'Stopped')
    case 'timed_out':
      return tr('Tidsavbrudd', 'Timed out')
    case 'blocked_by_site':
      return tr('Blokkert av nettstedet', 'Blocked by the site')
    case 'handoff_declined':
      return tr('Overtakelse avslått', 'Hand-off declined')
    case 'handoff_timeout':
      return tr('Overtakelse utløpt', 'Hand-off expired')
    default:
      return tr('Ukjent status', 'Unknown status')
  }
}

const ACTION_VERBS: Readonly<Record<string, [string, string]>> = {
  navigate: ['Åpnet side', 'Opened page'],
  goto: ['Åpnet side', 'Opened page'],
  click: ['Klikket', 'Clicked'],
  type: ['Skrev', 'Typed'],
  fill: ['Skrev', 'Typed'],
  scroll: ['Blar', 'Scrolled'],
  extract: ['Leste', 'Read'],
  read: ['Leste', 'Read'],
  screenshot: ['Tok skjermbilde', 'Took a screenshot'],
}

/** One timeline row as a line of text. Host and closed words only. */
export function spaceBrowserEventLabel(event: SpaceBrowserEvent, tr: Tr): string {
  switch (event.kind) {
    case 'action': {
      const verb = event.actionType ? ACTION_VERBS[event.actionType] : undefined
      const what = verb ? tr(verb[0], verb[1]) : tr('Handling', 'Action')
      return event.host ? `${what} · ${event.host}` : what
    }
    case 'observation':
      return event.host ? `${tr('Så siden', 'Saw the page')} · ${event.host}` : tr('Så siden', 'Saw the page')
    case 'paused':
      return tr('Pauset', 'Paused')
    case 'resumed':
      return tr('Fortsatte', 'Resumed')
    case 'approval_required':
      return tr('Venter på godkjenning', 'Waiting for approval')
    case 'approval_decided':
      return tr('Godkjenning avgjort', 'Approval decided')
    case 'handoff_requested':
      return event.host
        ? tr(`Et verifiseringstrinn blokkerer siden på ${event.host}`, `A verification step blocks the page on ${event.host}`)
        : tr('Et verifiseringstrinn blokkerer siden', 'A verification step blocks the page')
    case 'handoff_resolved':
      return tr('Overtakelsen er avsluttet', 'The hand-off is over')
    case 'result':
      return tr('Resultat', 'Result')
    default:
      return tr('Hendelse', 'Event')
  }
}

/** The newest actions of a run, oldest first, at most `count`. */
export function lastSpaceBrowserActions(events: readonly SpaceBrowserEvent[], count: number): SpaceBrowserEvent[] {
  return events.filter((event) => event.kind === 'action').slice(-count)
}

/** The evidence id of the newest row that carries one, or `undefined` when no row does. */
export function latestSpaceBrowserEvidenceId(events: readonly SpaceBrowserEvent[]): string | undefined {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const evidenceId = events[index]?.evidenceId
    if (evidenceId) return evidenceId
  }
  return undefined
}

/** Every evidence id in the rows, newest first, at most `count`. Each one is a screenshot the starter may open. */
export function spaceBrowserEvidenceIds(events: readonly SpaceBrowserEvent[], count: number): string[] {
  const ids: string[] = []
  for (let index = events.length - 1; index >= 0 && ids.length < count; index -= 1) {
    const evidenceId = events[index]?.evidenceId
    if (evidenceId && !ids.includes(evidenceId)) ids.push(evidenceId)
  }
  return ids
}

/**
 * The timeline of one run, polled while the caller says it is visible and the run is live. Each poll reads the rows
 * after the last one it holds, so a poll is one request while nothing has happened.
 */
export function createSpaceBrowserRunFeed(input: {
  readonly spaceRef: () => string
  readonly runId: () => string
  readonly visible: () => boolean
}) {
  const [header, setHeader] = createSignal<SpaceBrowserRunHeader | null>(null)
  const [events, setEvents] = createSignal<readonly SpaceBrowserEvent[]>([])
  const [failed, setFailed] = createSignal(false)
  let cursor: string | undefined
  let heldRunId = ''
  let inFlight = false

  const reset = () => {
    cursor = undefined
    setEvents([])
    setHeader(null)
    setFailed(false)
  }

  const pollOnce = async () => {
    if (inFlight) return
    inFlight = true
    const runId = input.runId()
    try {
      for (let pages = 0; pages < MAX_PAGES_PER_POLL; pages += 1) {
        const page = await readSpaceBrowserTimeline(input.spaceRef(), runId, { after: cursor, limit: 200 })
        if (runId !== input.runId()) return
        if (page.events.length > 0) {
          cursor = page.events[page.events.length - 1]?.eventId
          setEvents((current) => [...current, ...page.events])
        }
        setHeader(page.run)
        setFailed(false)
        if (!page.nextEventId || page.events.length === 0) break
      }
    } catch {
      setFailed(true)
    } finally {
      inFlight = false
    }
  }

  const live = createMemo(() => {
    const run = header()
    return run === null || isSpaceBrowserRunLive(run)
  })
  const active = createMemo(() => input.visible() && live())

  createEffect(
    () => ({ runId: input.runId(), on: active() }),
    ({ runId, on }) => {
      // A different run never shows the previous run's rows.
      if (runId !== heldRunId) {
        heldRunId = runId
        untrack(reset)
      }
      if (!on) return
      untrack(() => void pollOnce())
      const timer = setInterval(() => void pollOnce(), SPACE_BROWSER_POLL_MS)
      // Returned as the effect's cleanup. onCleanup has no owner inside an effect's apply, so it never ran and every
      // re-run left its timer polling.
      return () => clearInterval(timer)
    },
  )

  return { header, events, failed, live }
}
